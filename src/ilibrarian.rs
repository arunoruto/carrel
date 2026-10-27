//! A one-time move from I, Librarian. Its data folder holds an SQLite database (`database/main.db`) and the
//! PDFs (`pdfs/000/000/000000123.pdf`). Each item becomes a paper: its tags join the keywords, its notes,
//! highlights and margin notes go into `notes.md`, and I, Librarian's citation key stays as an alias
//! (`ids`), so documents citing it still compile with biber. Projects become projects. Nothing is
//! downloaded: text extraction and references come afterwards (`carrel extract --all`, `carrel refs --all`).

use crate::bib::Entry;
use crate::{Library, citekey, copy_pdf, fetch, same_paper, valid_id};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// What an import did: the papers it made (I, Librarian item id → paper id) and what's worth telling.
pub struct Imported {
    pub papers: HashMap<i64, String>,
    pub messages: Vec<String>,
}

impl Library {
    pub fn import_ilibrarian(&self, data: &Path) -> Result<Imported> {
        let db = data.join("database").join("main.db");
        let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("cannot open {}", db.display()))?;
        let list = |sql: &str, id: i64| -> Result<Vec<String>> {
            let mut stmt = conn.prepare_cached(sql)?;
            let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        let (mut papers, mut messages) = (HashMap::new(), vec![]);
        let mut known = self.papers().0;

        let mut stmt = conn.prepare(
            "SELECT i.id, i.title, i.reference_type, i.bibtex_type, i.bibtex_id, i.publication_date, i.volume, i.issue,
                    i.pages, i.abstract, i.publisher, i.place_published, i.urls, p.primary_title, s.secondary_title,
                    t.tertiary_title
             FROM items i
             LEFT JOIN primary_titles p ON p.id = i.primary_title_id
             LEFT JOIN secondary_titles s ON s.id = i.secondary_title_id
             LEFT JOIN tertiary_titles t ON t.id = i.tertiary_title_id
             ORDER BY i.id",
        )?;
        let items: Vec<(i64, [Option<String>; 15])> = stmt
            .query_map([], |r| {
                let mut row: [Option<String>; 15] = Default::default();
                for (i, cell) in row.iter_mut().enumerate() {
                    *cell = r.get::<_, Option<String>>(i + 1)?.map(|s| tidy(&s)).filter(|s| !s.is_empty());
                }
                Ok((r.get(0)?, row))
            })?
            .collect::<rusqlite::Result<_>>()?;

        for (item, row) in items {
            let [
                title,
                reference_type,
                bibtex_type,
                old_key,
                date,
                volume,
                issue,
                pages,
                abs,
                publisher,
                place,
                urls,
                primary,
                secondary,
                tertiary,
            ] = row;
            let title = latex(&title.unwrap_or_default());
            let kind = match (bibtex_type.as_deref(), reference_type.as_deref()) {
                (Some("conference"), _) => "inproceedings",
                (Some(kind), _) => kind,
                (None, Some("chapter")) => "incollection",
                (None, Some("conference")) => "inproceedings",
                (None, Some("thesis")) => "phdthesis",
                (None, Some(kind @ ("article" | "book"))) => kind,
                _ => "misc",
            };
            let mut bib = Entry::new(kind);
            bib.set("title", Some(&title));
            let names = |table: &str, link: &str, column: &str| -> Result<String> {
                let sql = format!(
                    "SELECT CASE WHEN coalesce(a.first_name, '') = '' THEN a.last_name ELSE a.last_name || ', ' || a.first_name END
                     FROM {link} x JOIN {table} a ON a.id = x.{column} WHERE x.item_id = ?1 ORDER BY x.position"
                );
                Ok(list(&sql, item)?.join(" and "))
            };
            bib.set(
                "author",
                Some(&names("authors", "items_authors", "author_id")?).filter(|a| !a.is_empty()).map(String::as_str),
            );
            bib.set(
                "editor",
                Some(&names("editors", "items_editors", "editor_id")?).filter(|e| !e.is_empty()).map(String::as_str),
            );
            bib.set("year", date.as_deref().and_then(|d| d.get(..4)));
            // The full title is the secondary one; the primary one is its abbreviation.
            let venue = secondary.or(primary);
            let venue_field = match kind {
                "article" => "journal",
                "incollection" | "inproceedings" => "booktitle",
                _ => "series",
            };
            bib.set(venue_field, venue.as_deref());
            if venue_field != "series" {
                bib.set("series", tertiary.as_deref());
            }
            bib.set("volume", volume.as_deref());
            bib.set("number", issue.as_deref());
            bib.set("pages", pages.map(|p| p.replace('-', "--").replace("----", "--")).as_deref());
            bib.set("publisher", publisher.as_deref());
            bib.set("address", place.as_deref());
            bib.set("abstract", abs.map(|a| latex(&a)).as_deref());

            let uids: Vec<String> =
                list("SELECT uid_type || ' ' || uid FROM uids WHERE item_id = ?1 ORDER BY id", item)?;
            for uid in &uids {
                match uid.split_once(' ') {
                    Some(("DOI", doi)) => bib.set("doi", fetch::find_doi(doi).as_deref().or(Some(doi))),
                    Some(("ARXIV", id)) => {
                        bib.set("eprint", Some(&fetch::arxiv_id(id).unwrap_or(id.to_string())));
                        bib.set("archiveprefix", Some("arXiv"));
                    }
                    Some(("ISBN", isbn)) => bib.set("isbn", Some(isbn)),
                    Some(("NASAADS", bibcode)) => {
                        bib.set("adsurl", Some(&format!("https://ui.adsabs.harvard.edu/abs/{bibcode}")))
                    }
                    _ => {}
                }
            }
            let url = urls
                .as_deref()
                .and_then(|u| u.split('|').map(str::trim).find(|u| !u.is_empty() && !u.contains("doi.org")));
            bib.set("url", url);
            let keywords = list(
                "SELECT k.keyword FROM items_keywords x JOIN keywords k ON k.id = x.keyword_id WHERE x.item_id = ?1",
                item,
            )?;
            let tags = list(
                "SELECT t.tag FROM items_tags x JOIN tags t ON t.id = x.tag_id WHERE x.item_id = ?1 ORDER BY t.tag",
                item,
            )?;
            let mut all: Vec<String> = vec![];
            for k in tags.into_iter().chain(keywords) {
                if !all.iter().any(|a| a.eq_ignore_ascii_case(&k)) {
                    all.push(k);
                }
            }
            bib.set("keywords", Some(all.join(", ")).filter(|k| !k.is_empty()).as_deref());
            bib.set("ids", old_key.as_deref());

            if let Some(old) = known.iter().find(|p| same_paper(&p.bib, &bib)) {
                messages.push(format!("{title}: already in the library as {}", old.id));
                papers.insert(item, old.id.clone());
                continue;
            }
            let id = self.unique_id(&citekey(&self.key, &bib)?);
            let dir = self.root.join(&id);
            fs::create_dir_all(&dir)?;
            fs::write(dir.join(format!("{id}.bib")), bib.to_bib(&id))?;
            let name = format!("{item:09}"); // 123 → pdfs/000/000/000000123.pdf
            let pdf = data.join("pdfs").join(&name[..3]).join(&name[3..6]).join(format!("{name}.pdf"));
            if pdf.exists()
                && let Err(e) = copy_pdf(&pdf, &dir.join(format!("{id}.pdf")))
            {
                messages.push(format!("{id}: PDF not copied: {e:#}"));
                let _ = fs::remove_file(dir.join(format!("{id}.pdf")));
            }
            let notes = notes(&conn, item)?;
            if !notes.is_empty() {
                fs::write(dir.join("notes.md"), notes)?;
            }
            known.push(self.get(&id)?);
            papers.insert(item, id);
        }

        let mut stmt = conn.prepare("SELECT id, project FROM projects ORDER BY id")?;
        let projects: Vec<(i64, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        for (project, name) in projects {
            let mut stmt = conn.prepare_cached(
                "SELECT item_id FROM projects_items WHERE project_id = ?1 ORDER BY added_time, item_id",
            )?;
            let items: Vec<i64> = stmt.query_map([project], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let ids: Vec<String> = items.iter().filter_map(|i| papers.get(i).cloned()).collect();
            if ids.is_empty() {
                continue;
            }
            let slug: String =
                name.trim().chars().map(|c| if c.is_whitespace() || "/\\".contains(c) { '-' } else { c }).collect();
            let slug = if valid_id(&slug) {
                slug
            } else {
                deunicode::deunicode(&slug)
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
                    .collect()
            };
            self.add_to_project(&slug, &ids)?;
            if slug != name {
                messages.push(format!("project {name:?} is now {slug}"));
            }
        }
        Ok(Imported { papers, messages })
    }
}

/// Whitespace collapsed: I, Librarian keeps titles wrapped over lines.
fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An item's notes, highlights and margin notes as Markdown, each with its author when several users wrote.
fn notes(conn: &Connection, item: i64) -> Result<String> {
    let user = "(SELECT username FROM users u WHERE u.id = x.user_id)";
    let rows = |sql: &str| -> Result<Vec<(String, i64, i64, String)>> {
        let mut stmt = conn.prepare_cached(sql)?;
        let rows = stmt.query_map([item], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get(1)?,
                r.get(2)?,
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let notes = rows(&format!("SELECT {user}, 0, 0, note FROM item_notes x WHERE item_id = ?1 ORDER BY id"))?;
    let markers: Vec<(String, i64, i64, String, i64, String)> = {
        let sql = format!(
            "SELECT {user}, page, marker_position, marker_text, marker_top, marker_color FROM markers x
             WHERE item_id = ?1 ORDER BY user_id, page, marker_position"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([item], |r| {
            let text = |i| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
            Ok((text(0)?, r.get(1)?, r.get(2)?, text(3)?, r.get(4)?, text(5)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let margin = rows(&format!(
        "SELECT {user}, page, 0, annotation FROM annotations x WHERE item_id = ?1 ORDER BY user_id, page, id"
    ))?;
    let users: std::collections::BTreeSet<&str> =
        notes.iter().chain(&margin).map(|r| r.0.as_str()).chain(markers.iter().map(|m| m.0.as_str())).collect();
    let several = users.len() > 1;
    let by = |user: &str| if several { format!(" ({user})") } else { String::new() };

    let mut out = String::new();
    for (user, _, _, note) in &notes {
        out += &format!("{}{}\n\n", html_to_markdown(note), by(user));
    }
    // Highlights are stored word by word. Words of one colour belong together when they follow each
    // other, or continue on the next line (where the numbering jumps over the other column's words).
    let mut passages: Vec<(String, i64, String)> = vec![];
    let mut last: Option<(String, i64, i64, i64, String)> = None;
    for (user, page, position, word, top, color) in markers {
        let follows = last.as_ref().is_some_and(|(u, p, pos, t, c)| {
            *u == user && *p == page && *c == color && (*pos + 1 == position || (1..=25).contains(&(top - t)))
        });
        match passages.last_mut() {
            Some(passage) if follows => passage.2 = format!("{} {}", passage.2, word.trim()),
            _ => passages.push((user.clone(), page, word.trim().to_string())),
        }
        last = Some((user, page, position, top, color));
    }
    if !passages.is_empty() || !margin.is_empty() {
        out += "## From I, Librarian\n\n";
        for (user, page, text) in passages {
            out += &format!("- p. {page}: \u{201c}{text}\u{201d}{}\n", by(&user));
        }
        for (user, page, _, text) in margin {
            out += &format!("- p. {page}, margin note: {}{}\n", tidy(&text), by(&user));
        }
    }
    Ok(out.trim_end().to_string() + if out.is_empty() { "" } else { "\n" })
}

/// Titles and abstracts with HTML markup (`M<sup>3</sup>`) in LaTeX (`M$^{3}$`); other tags are dropped.
fn latex(text: &str) -> String {
    let mut out = text.to_string();
    for (tag, math) in [("sup", "^"), ("sub", "_")] {
        for case in [tag.to_string(), tag.to_uppercase()] {
            while let Some(start) = out.find(&format!("<{case}>")) {
                let Some(end) = out[start..].find(&format!("</{case}>")).map(|e| start + e) else { break };
                let inner = out[start + case.len() + 2..end].to_string();
                out.replace_range(start..end + case.len() + 3, &format!("${math}{{{inner}}}$"));
            }
        }
    }
    if out.contains('<') { fetch::strip_tags(&out) } else { out }
}

/// I, Librarian's notes are HTML: paragraphs become lines and links Markdown links.
fn html_to_markdown(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out += &rest[..start];
        let Some(end) = rest[start..].find('>').map(|e| start + e) else { break };
        let tag = &rest[start + 1..end];
        if let Some(href) =
            tag.strip_prefix("a ").and_then(|a| a.split("href=\"").nth(1)).and_then(|h| h.split('"').next())
        {
            let close = rest[end..].find("</a>").map(|c| end + c);
            let text = close.map_or("", |c| &rest[end + 1..c]);
            out += &format!("[{}]({href})", text.trim());
            rest = close.map_or(&rest[end + 1..], |c| &rest[c + 4..]);
            continue;
        }
        if matches!(tag, "/p" | "br" | "br/" | "br /" | "/li" | "/div") {
            out.push('\n');
        }
        rest = &rest[end + 1..];
    }
    out += rest;
    let text = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_as_markdown() {
        let html =
            r#"<p>Source for the <a href="http://x.org/data?a_(b)">olivine data.</a></p><p>Second&nbsp;line</p>"#;
        assert_eq!(html_to_markdown(html), "Source for the [olivine data.](http://x.org/data?a_(b))\nSecond line");
        assert_eq!(html_to_markdown("plain"), "plain");
        assert_eq!(
            latex("The M<sup>3</sup> mapper, H<SUB>2</SUB>O and <i>in situ</i>"),
            "The M$^{3}$ mapper, H$_{2}$O and in situ"
        );
    }
}
