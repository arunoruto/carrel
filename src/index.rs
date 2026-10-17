//! Full-text search: an SQLite FTS5 index in the cache directory. It only mirrors the files, so it is
//! brought up to date before every search and can be deleted any time.

use crate::{Field, Paper, SUPPLEMENTARY};
use anyhow::Result;
use rusqlite::{Connection, ErrorCode, TransactionBehavior, params};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS stamps (id TEXT PRIMARY KEY, stamp TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS papers USING fts5(
    id UNINDEXED, title, authors, keywords, ids, abstract, summary, notes, fulltext,
    tokenize = 'porter unicode61 remove_diacritics 2'
);";

/// Column weights, in schema order: a hit in the title counts ten times one in the full text.
const RANK: &str = "bm25(papers, 0, 10, 6, 8, 5, 3, 3, 2, 1)";

/// Ids of the papers matching `query`, best first, each with a snippet of where it matched.
pub fn search(path: &Path, papers: &[Paper], query: &str, limit: usize) -> Result<Vec<(String, String)>> {
    let mut conn = open(path)?;
    sync(&mut conn, papers)?;

    // Each word matches exactly (stemmed) or as a prefix, so results appear while typing.
    // `field:word` looks in one column only.
    let words: Vec<String> = query
        .split_whitespace()
        .flat_map(|token| {
            let field = token.split_once(':').and_then(|(name, rest)| Some((column(name)?, rest)));
            let (column, text) = field.map_or((None, token), |(c, rest)| (Some(c), rest));
            text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(move |w| {
                let word = format!("(\"{w}\" OR \"{w}\"*)");
                column.map_or(word.clone(), |c| format!("{c} : {word}"))
            })
        })
        .collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "SELECT id, snippet(papers, -1, '[', ']', '…', 12) FROM papers WHERE papers MATCH ?1 ORDER BY {RANK} LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let limit = i64::try_from(limit).unwrap_or(-1);
    // Every word must match; if no paper has them all, any word will do.
    for op in [" AND ", " OR "] {
        let rows = stmt.query_map(params![words.join(op), limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        if !hits.is_empty() || words.len() == 1 {
            return Ok(hits);
        }
    }
    Ok(vec![])
}

/// Open the index, rebuilding it when the file is damaged; it is only a cache.
fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let attempt = || -> rusqlite::Result<Connection> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.execute_batch(SCHEMA)?;
        Ok(conn)
    };
    match attempt() {
        Err(rusqlite::Error::SqliteFailure(e, _))
            if matches!(e.code, ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) =>
        {
            for suffix in ["", "-wal", "-shm"] {
                let _ = fs::remove_file(format!("{}{suffix}", path.display()));
            }
            Ok(attempt()?)
        }
        conn => Ok(conn?),
    }
}

/// The index column a query field name searches.
fn column(field: &str) -> Option<&'static str> {
    Some(match field {
        "title" => "title",
        "author" | "authors" => "authors",
        "keyword" | "keywords" => "keywords",
        "year" | "venue" | "journal" | "doi" | "arxiv" | "key" | "id" => "ids",
        "abstract" => "abstract",
        "summary" => "summary",
        "notes" | "note" => "notes",
        "text" | "fulltext" => "fulltext",
        _ => return None,
    })
}

/// Re-read the papers whose files changed since the last search, and forget deleted ones.
fn sync(conn: &mut Connection, papers: &[Paper]) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let known: HashMap<String, String> = {
        let mut stmt = tx.prepare("SELECT id, stamp FROM stamps")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for p in papers {
        let stamp = stamp(p);
        if known.get(&p.id) == Some(&stamp) {
            continue;
        }
        let b = &p.bib;
        let ids = [Some(p.id.clone()), b.venue(), b.doi(), b.arxiv(), b.year().map(|y| y.to_string())];
        tx.execute("DELETE FROM papers WHERE id = ?1", [&p.id])?;
        tx.execute(
            "INSERT INTO papers VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.id,
                b.title(),
                b.authors().join("; "),
                b.keywords().join(", "),
                ids.into_iter().flatten().collect::<Vec<_>>().join(" "),
                b.get("abstract"),
                p.get(Field::Summary),
                p.get(Field::Notes),
                text(p),
            ],
        )?;
        tx.execute("INSERT OR REPLACE INTO stamps VALUES (?1, ?2)", [&p.id, &stamp])?;
    }
    let live: HashSet<&str> = papers.iter().map(|p| p.id.as_str()).collect();
    for id in known.keys().filter(|id| !live.contains(id.as_str())) {
        tx.execute("DELETE FROM papers WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM stamps WHERE id = ?1", [id])?;
    }
    tx.commit()?;
    Ok(())
}

/// The paper's full text followed by the text of its supplementary material.
fn text(p: &Paper) -> String {
    let supplementary = p.supplementary().into_iter().filter(|name| name.ends_with(".md"));
    let texts = supplementary.filter_map(|name| fs::read_to_string(p.dir.join(SUPPLEMENTARY).join(name)).ok());
    p.fulltext().into_iter().chain(texts).collect::<Vec<_>>().join("\n\n")
}

/// Changes whenever one of a paper's indexed files does.
fn stamp(p: &Paper) -> String {
    let supplementary = p.supplementary().into_iter().map(|name| p.dir.join(SUPPLEMENTARY).join(name));
    let files = [p.bib_path(), p.dir.join("summary.md"), p.dir.join("notes.md"), p.dir.join("fulltext.md")];
    let files: Vec<_> = files.into_iter().chain(supplementary).collect();
    let stamps = files.iter().map(|f| fs::metadata(f).map(|m| format!("{}@{:?}", m.len(), m.modified().ok())));
    stamps.map(|s| s.unwrap_or_default()).collect::<Vec<_>>().join("|")
}
