//! The index: an SQLite database in the cache directory with full-text search (FTS5) and each paper's
//! references for the citation graph, and the passages' vectors for semantic search. It only mirrors the files, so it is brought up to date before
//! every use and can be deleted any time.

use crate::embed::{self, Span};
use crate::graph::{self, Reference};
use crate::{Field, Paper, SUPPLEMENTARY};
use anyhow::Result;
use rusqlite::{Connection, ErrorCode, TransactionBehavior, params};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Raised whenever the tables change: an index of another version is rebuilt.
const VERSION: i64 = 3;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS stamps (id TEXT PRIMARY KEY, stamp TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS papers USING fts5(
    id UNINDEXED, title, authors, keywords, ids, abstract, summary, notes, fulltext,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS refs (paper TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS passages (paper TEXT NOT NULL, model TEXT NOT NULL, start INTEGER, length INTEGER NOT NULL, vector BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS passages_paper ON passages (paper);";

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

/// Each paper's references, read from the files only for papers that changed since the last use.
/// A damaged `references.json` counts as none (`carrel doctor` reports it).
pub fn references(path: &Path, papers: &[Paper]) -> Result<BTreeMap<String, Vec<Reference>>> {
    let mut conn = open(path)?;
    sync(&mut conn, papers)?;
    let mut stmt = conn.prepare("SELECT paper, json FROM refs")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut refs = BTreeMap::new();
    for row in rows {
        let (id, json) = row?;
        if let Ok(list) = serde_json::from_str(&json) {
            refs.insert(id, list);
        }
    }
    Ok(refs)
}

/// For each paper with embeddings by `model`, its passage closest to `query` (a unit vector) with the
/// cosine similarity, best first.
pub fn nearest(path: &Path, papers: &[Paper], model: &str, query: &[f32]) -> Result<Vec<(String, Span, f32)>> {
    let mut conn = open(path)?;
    sync(&mut conn, papers)?;
    let mut stmt = conn.prepare("SELECT paper, start, length, vector FROM passages WHERE model = ?1")?;
    let mut rows = stmt.query([model])?;
    let mut best: HashMap<String, (Span, f32)> = HashMap::new();
    // ponytail: every vector is compared, fine for thousands of papers; an ANN index (sqlite-vec) beyond that.
    while let Some(row) = rows.next()? {
        let vector: Vec<u8> = row.get(3)?;
        let score: f32 = vector.as_chunks::<4>().0.iter().zip(query).map(|(b, q)| f32::from_le_bytes(*b) * q).sum();
        let paper: String = row.get(0)?;
        if best.get(&paper).is_none_or(|(_, s)| score > *s) {
            let span = Span {
                offset: row.get::<_, Option<i64>>(1)?.map(|o| o as usize),
                length: row.get::<_, i64>(2)? as usize,
            };
            best.insert(paper, (span, score));
        }
    }
    let mut hits: Vec<_> = best.into_iter().map(|(paper, (span, score))| (paper, span, score)).collect();
    hits.sort_by(|a, b| b.2.total_cmp(&a.2));
    Ok(hits)
}

/// Open the index, rebuilding it when the file is damaged; it is only a cache.
fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let attempt = || -> rusqlite::Result<Connection> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(10))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != VERSION {
            let tables: Vec<String> = conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'papers_%' AND name NOT LIKE 'sqlite_%'")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for table in tables {
                conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\""))?;
            }
            conn.execute_batch(&format!("PRAGMA user_version = {VERSION}"))?;
        }
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
        tx.execute("DELETE FROM refs WHERE paper = ?1", [&p.id])?;
        if let Ok(json) = fs::read_to_string(p.dir.join(graph::FILE)) {
            tx.execute("INSERT INTO refs VALUES (?1, ?2)", [&p.id, &json])?;
        }
        tx.execute("DELETE FROM passages WHERE paper = ?1", [&p.id])?;
        // Damaged embeddings count as none. Outdated ones still serve until `carrel embed` redoes them.
        if let Some(stored) = embed::stored(p)
            && let Ok(vectors) = stored.vectors()
        {
            for (span, vector) in stored.passages.iter().zip(vectors) {
                let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
                tx.execute(
                    "INSERT INTO passages VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![p.id, stored.model, span.offset.map(|o| o as i64), span.length as i64, bytes],
                )?;
            }
        }
        tx.execute("INSERT OR REPLACE INTO stamps VALUES (?1, ?2)", [&p.id, &stamp])?;
    }
    let live: HashSet<&str> = papers.iter().map(|p| p.id.as_str()).collect();
    for id in known.keys().filter(|id| !live.contains(id.as_str())) {
        tx.execute("DELETE FROM papers WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM refs WHERE paper = ?1", [id])?;
        tx.execute("DELETE FROM passages WHERE paper = ?1", [id])?;
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
    let files = [
        p.bib_path(),
        p.dir.join("summary.md"),
        p.dir.join("notes.md"),
        p.dir.join("fulltext.md"),
        p.dir.join(graph::FILE),
        p.dir.join(embed::FILE),
    ];
    let files: Vec<_> = files.into_iter().chain(supplementary).collect();
    let stamps = files.iter().map(|f| fs::metadata(f).map(|m| format!("{}@{:?}", m.len(), m.modified().ok())));
    stamps.map(|s| s.unwrap_or_default()).collect::<Vec<_>>().join("|")
}
