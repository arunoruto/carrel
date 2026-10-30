//! `carrel doctor`: what's wrong or missing in the library, each with the command that fixes it.

use crate::{Library, fetch, graph};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize)]
pub struct Issue {
    /// "error": the paper can't be loaded; "warning": something needs fixing; "info": worth a look.
    pub level: &'static str,
    /// What was checked: broken, fields, markup, macro, duplicate, text, pdf, preprint, references, project, people.
    pub check: &'static str,
    pub id: String,
    pub message: String,
    /// A command that fixes it, when there is one.
    pub fix: Option<String>,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

impl Library {
    /// Check every paper and project. Errors come first, then warnings, then infos.
    pub fn doctor(&self) -> Vec<Issue> {
        let mut issues = vec![];
        let mut report = |level, check, id: &str, message: String, fix: Option<String>| {
            issues.push(Issue { level, check, id: id.to_string(), message, fix });
        };
        let mut papers = vec![];
        for id in self.folders() {
            match self.get(&id) {
                Ok(p) => papers.push(p),
                Err(e) => report("error", "broken", &id, format!("{e:#}"), Some(format!("carrel edit {id}"))),
            }
        }

        // Why `carrel extract` won't help, if it won't.
        let no_extractor = match self.extractor() {
            Ok(Some(_)) => String::new(),
            Ok(None) => format!("; {}", crate::NO_EXTRACTOR),
            Err(e) => format!("; {e:#}"),
        };
        let mut seen: HashMap<String, String> = HashMap::new();
        let mut doubled = HashSet::new();
        for p in &papers {
            let (b, id) = (&p.bib, p.id.as_str());
            let edit = Some(format!("carrel edit {id}"));
            let update = match b.doi().is_some() || b.arxiv().is_some() {
                true => format!("carrel update {id}"),
                false => format!("carrel update {id} <DOI, arXiv id or title>"),
            };

            // The fields every style needs (any of the alternatives), plus the venue the entry type needs.
            let venue: &[&str] = match b.kind.as_str() {
                "article" => &["journal", "journaltitle"],
                "inproceedings" | "incollection" => &["booktitle"],
                _ => &[],
            };
            let required: [&[&str]; 4] = [&["title"], &["author", "editor"], &["year", "date"], venue];
            let missing: Vec<&str> = required
                .iter()
                .filter(|names| !names.is_empty() && names.iter().all(|f| b.get(f).is_none()))
                .map(|names| names[0])
                .collect();
            if !missing.is_empty() {
                report("warning", "fields", id, format!("missing {}", missing.join(", ")), Some(update.clone()));
            }

            for (name, raw) in &b.fields {
                let tag = raw.find('<').is_some_and(|i| {
                    raw[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') && raw[i..].contains('>')
                });
                if tag {
                    report("warning", "markup", id, format!("{name} contains HTML/XML tags"), edit.clone());
                }
                let bare = !raw.starts_with(['{', '"']) && !raw.chars().all(|c| c.is_ascii_digit());
                if bare && !MONTHS.contains(&raw.to_lowercase().as_str()) {
                    let message = format!("{name} = {raw} is an @string macro, undefined outside its original .bib");
                    report("warning", "macro", id, message, edit.clone());
                }
            }

            let mut keys = fetch::paper_keys(&fetch::Ids { doi: b.doi(), arxiv: b.arxiv() }, &b.title());
            // The same pages of the same venue and year: the same paper, however its title is spelled.
            let pages = b.get("pages").filter(|p| p.contains(['-', '–']));
            if let (Some(year), Some(venue), Some(pages)) = (b.year(), b.venue(), pages) {
                keys.push(format!("{year} {} {}", fetch::title_key(&venue), fetch::title_key(&pages)));
            }
            if let Some(other) = keys.iter().find_map(|k| seen.get(k)) {
                let message = format!("the same paper as {other}; keep one (`carrel rm`) after moving over its notes");
                report("warning", "duplicate", id, message, None);
                doubled.extend([id.to_string(), other.clone()]);
            }
            for key in keys {
                seen.entry(key).or_insert_with(|| id.to_string());
            }

            match (p.pdf(), p.fulltext()) {
                (None, _) => {
                    let fix = match b.arxiv() {
                        Some(_) => format!("carrel attach {id}"),
                        None => format!("carrel attach {id} <file.pdf>"),
                    };
                    report("info", "pdf", id, "no PDF".into(), Some(fix));
                }
                (Some(_), None) => {
                    let message = format!("no extracted text, so AI assistants can't read it{no_extractor}");
                    report("warning", "text", id, message, Some(format!("carrel extract {id}")));
                }
                _ => {}
            }
            if b.arxiv().is_some() && b.doi().is_none_or(|d| d.to_lowercase().starts_with("10.48550/")) {
                let message = "an arXiv preprint; it may have been published since".into();
                report("info", "preprint", id, message, Some(format!("carrel update {id}")));
            }
            if let Err(e) = graph::read(&p.dir) {
                report("error", "references", id, format!("{e:#}"), Some(format!("carrel refs {id} --refresh")));
            }
        }

        for name in self.projects() {
            for id in self.project_papers(&name).map(|(_, missing)| missing).unwrap_or_default() {
                let message = format!("listed in project {name} but not in the library");
                report("warning", "project", &id, message, Some(format!("carrel project remove {name} {id}")));
            }
        }
        if let Err(e) = self.people() {
            report("error", "people", crate::authors::DIR, format!("{e:#}"), None);
        }
        // Nearly the same title, keywords and abstract: most likely one paper twice. 0.97 is above the most alike
        // two distinct papers we've seen (two papers by one group with one title, 0.95).
        let similar = self.similar(1).unwrap_or_default();
        let mut similar: Vec<_> =
            similar.into_iter().filter_map(|(id, near)| Some((id, near.into_iter().next()?))).collect();
        similar.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, (other, score)) in similar {
            if score >= 0.97 && id > other && !doubled.contains(&id) && !doubled.contains(&other) {
                let message = format!(
                    "probably the same paper as {other} ({score:.2} alike); keep one (`carrel rm`) after moving over its notes"
                );
                report("warning", "duplicate", &id, message, None);
            }
        }
        let rank = |level: &str| ["error", "warning", "info"].iter().position(|l| *l == level);
        issues.sort_by(|a, b| rank(a.level).cmp(&rank(b.level)).then(a.id.cmp(&b.id)));
        issues
    }
}

#[cfg(test)]
mod tests {
    use crate::Library;
    use std::fs;

    #[test]
    fn checks() {
        let tmp = std::env::temp_dir().join(format!("carrel-doctor-{}", std::process::id()));
        let lib = Library::open(&tmp).unwrap();
        let write = |id: &str, bib: &str| {
            fs::create_dir_all(lib.root.join(id)).unwrap();
            fs::write(lib.root.join(id).join(format!("{id}.bib")), bib).unwrap();
        };
        write(
            "a",
            "@article{a, title = {Graphs now}, author = {Doe, J.}, year = 2020, journal = nat, abstract = {<p>Hi</p>}}",
        );
        write("b", "@misc{b, title = {Graphs now}, author = {Roe, R.}, date = {2021}, eprint = {2101.00001}}");
        write("c", "@inproceedings{c, title = {Other}}");
        write("d", "@misc{wrong, title = {D}}");
        write(
            "e",
            "@article{e, title = {Steps. I. Glass}, author = {Jaeger, C.}, year = 1994, journal = {A&A}, pages = {641--655}}",
        );
        write(
            "f",
            "@article{f, title = {Steps. 1: Glass}, author = {Jäger, C.}, year = 1994, journal = {A&A}, pages = {641--655}}",
        );
        fs::create_dir_all(lib.root.join("projects")).unwrap();
        fs::write(lib.root.join("projects/x.txt"), "a\ngone\n").unwrap();

        let found: Vec<(&str, &str)> = lib.doctor().leak().iter().map(|i| (i.check, i.id.as_str())).collect();
        for expected in [
            ("broken", "d"),
            ("markup", "a"),
            ("macro", "a"),
            ("duplicate", "b"),
            ("duplicate", "f"),
            ("fields", "c"),
            ("project", "gone"),
            ("pdf", "a"),
            ("preprint", "b"),
        ] {
            assert!(found.contains(&expected), "{expected:?} not in {found:?}");
        }
        assert!(!found.contains(&("fields", "a")) && !found.contains(&("fields", "b")), "{found:?}");
        assert_eq!(found[0], ("broken", "d"), "errors first");
        fs::remove_dir_all(tmp).unwrap();
    }
}
