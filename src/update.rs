//! Refreshing a paper's metadata, for instance once its arXiv preprint has been published.

use crate::bib::Entry;
use crate::{Library, books, citekey, fetch};
use anyhow::{Result, bail};
use std::fs;

impl Library {
    /// Fetch a paper's metadata again and merge it into its .bib: fetched fields replace those with
    /// the same name when their text differs, everything else (keywords, your own fields) stays, and so
    /// does the key. `source` (a DOI, arXiv id, ISBN or exact title) says which paper it is, for papers
    /// added without one. A preprint switches to its published version when Semantic Scholar knows one.
    /// Books only get what they lack (see [`Library::update_book`]). Returns what changed.
    pub fn update(&self, id: &str, source: Option<&str>) -> Result<Vec<String>> {
        let mut p = self.get(id)?;
        let mut ids = match source.map(fetch::classify).transpose()? {
            Some(fetch::Source::Doi(doi)) => fetch::Ids { doi: Some(doi), arxiv: None },
            Some(fetch::Source::Arxiv(arxiv)) => fetch::Ids { doi: None, arxiv: Some(arxiv) },
            Some(fetch::Source::Isbn(isbn)) => return self.update_book(id, Some(isbn)),
            Some(fetch::Source::Title(title)) => fetch::by_title(&title)?,
            Some(fetch::Source::Pdf(_)) => bail!("to give a paper its PDF, use `carrel attach`"),
            None => fetch::Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() },
        };
        if source.is_none() && p.bib.kind == "book" {
            return self.update_book(id, None);
        }
        if ids.is_empty() {
            bail!(
                "{id} has no DOI or arXiv id; say which paper it is: carrel update {id} <DOI, arXiv id, ISBN or title>"
            );
        }
        let mut notes = vec![];
        let preprint = ids.doi.as_ref().is_none_or(|doi| doi.to_lowercase().starts_with("10.48550/"));
        if preprint && let Some(arxiv) = &ids.arxiv {
            match fetch::published_doi(arxiv) {
                Ok(Some(doi)) => ids.doi = Some(doi),
                Ok(None) => {}
                Err(e) => notes.push(format!("couldn't look for a published version: {e:#}")),
            }
        }

        let fresh = fetch::metadata(&ids)?;
        if source.is_none() {
            check_doi(&p.bib, &fresh, id)?;
        }
        let changes = merge(&mut p.bib, &fresh);
        if changes.is_empty() {
            notes.push("already up to date".into());
            return Ok(notes);
        }
        p.save()?;
        let key = citekey(&self.key, &p.bib)?;
        if key != id {
            notes.push(format!(
                "the key template now gives {key}; `carrel rename {id}` switches (citations of {id} would break)"
            ));
        }
        Ok(changes.into_iter().chain(notes).collect())
    }
}

impl Library {
    /// A book gets only the fields it lacks, from its DOI if it has one, and from Open Library by its
    /// ISBN (given, in its .bib, on its copyright page, or found by title, author and year). Publishers'
    /// DOI records ("Wiley", a year early, `@misc`) and Open Library are less careful than a .bib.
    fn update_book(&self, id: &str, isbn: Option<String>) -> Result<Vec<String>> {
        let mut p = self.get(id)?;
        let mut changes = vec![];
        if let Some(doi) = p.bib.doi() {
            let fresh = fetch::metadata(&fetch::Ids { doi: Some(doi), arxiv: None })?;
            check_doi(&p.bib, &fresh, id)?;
            changes.extend(fill(&mut p.bib, &fresh));
        }
        let text = fs::read_to_string(p.dir.join("fulltext.md")).unwrap_or_default();
        let isbn = match isbn {
            Some(isbn) => Some(isbn),
            None => books::isbn_for(&p.bib, &text)?,
        };
        match isbn {
            Some(isbn) => {
                let mut book = books::by_isbn(&isbn)?.unwrap_or_else(|| Entry::new("book"));
                book.set("isbn", Some(&isbn));
                changes.extend(fill(&mut p.bib, &book));
            }
            None if p.bib.doi().is_none() => bail!(
                "no ISBN for {id}: none in its text, and Open Library has no edition of {} with one; \
                 `carrel update {id} <ISBN>` if you know it",
                p.bib.year().map_or("its year".into(), |y| y.to_string())
            ),
            None => {}
        }
        if changes.is_empty() {
            return Ok(vec!["already up to date".into()]);
        }
        p.save()?;
        Ok(changes)
    }
}

/// A DOI in a .bib can be another work's, from an import (a review of the book, say), and replacing the
/// entry with that work's metadata would lose it. A DOI given on the command line is believed instead.
fn check_doi(bib: &Entry, fresh: &Entry, id: &str) -> Result<()> {
    let theirs = fresh.title();
    if same_work(&bib.title(), &theirs) {
        return Ok(());
    }
    bail!(
        "its DOI {} is {theirs:?}, not this {}; fix the doi field (`carrel edit {id}`), or if the DOI is right, \
         `carrel update {id} <the DOI>`",
        fresh.doi().unwrap_or_default(),
        bib.kind
    )
}

/// Whether a DOI's work (titled `theirs`) can be the entry titled `mine`; true when there is nothing to
/// compare.
pub(crate) fn same_work(mine: &str, theirs: &str) -> bool {
    // Without a subtitle ("Numba" for "Numba: a LLVM-based Python JIT compiler") and a journal's section
    // number ("VIII. A dynamical theory of …").
    let main = |title: &str| {
        let title = title.split(':').next().unwrap_or_default().trim();
        let numbered =
            title.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| "IVXLC0123456789".contains(c)));
        numbered.map_or(title, |(_, rest)| rest).to_string()
    };
    mine.is_empty()
        || theirs.is_empty()
        || fetch::same_title(mine, theirs)
        || books::same_book(mine, theirs)
        || fetch::same_title(&main(mine), &main(theirs))
}

/// Only the fields `bib` lacks: Open Library's titles and names are less careful than yours.
fn fill(bib: &mut Entry, fresh: &Entry) -> Vec<String> {
    let mut changes = vec![];
    for (name, raw) in &fresh.fields {
        if bib.get(name).is_none() {
            changes.push(format!("{name}: {}", fresh.get(name).unwrap_or_default()));
            bib.set_raw(name, raw);
        }
    }
    changes
}

/// Fetched fields replace those whose text differs (so your formatting of an unchanged value
/// survives); fields only in `bib` stay. Returns what changed.
fn merge(bib: &mut Entry, fresh: &Entry) -> Vec<String> {
    let short = |text: Option<String>| {
        let text = text.unwrap_or_else(|| "(none)".into());
        match text.char_indices().nth(60) {
            Some((cut, _)) => format!("{}…", &text[..cut]),
            None => text,
        }
    };
    let mut changes = vec![];
    if bib.kind != fresh.kind {
        changes.push(format!("type: {} → {}", bib.kind, fresh.kind));
        bib.kind = fresh.kind.clone();
    }
    for (name, raw) in &fresh.fields {
        if bib.get(name) != fresh.get(name) {
            changes.push(format!("{name}: {} → {}", short(bib.get(name)), short(fresh.get(name))));
            bib.set_raw(name, raw);
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filling() {
        let (_, mut bib) = Entry::parse("@book{k, title = {{Lunar} Sourcebook}, year = 1991}").unwrap();
        let (_, fresh) = Entry::parse(
            "@book{x, title = {Lunar sourcebook}, year = {1991}, publisher = {CUP}, isbn = {9780521334440}}",
        )
        .unwrap();
        assert_eq!(fill(&mut bib, &fresh), ["publisher: CUP", "isbn: 9780521334440"]);
        assert_eq!(bib.raw("title"), Some("{{Lunar} Sourcebook}"), "yours stays");
        assert!(fill(&mut bib, &fresh).is_empty());
    }

    #[test]
    fn wrong_dois() {
        let entry = |s: &str| Entry::parse(s).unwrap().1;
        let book = entry("@book{k, title = {{Introduction to Mineralogy}}}");
        let review =
            entry("@article{x, title = {N ESSE , W. D. 2000. Introduction to Mineralogy. 442 pp.}, doi = {10.1/r}}");
        let e = check_doi(&book, &review, "k").unwrap_err().to_string();
        assert!(e.contains("10.1/r") && e.contains("not this book"), "{e}");
        assert!(check_doi(&book, &entry("@book{x, title = {Introduction to mineralogy}}"), "k").is_ok());
        let sub = entry("@book{x, title = {Introduction to Mineralogy: Second edition}}");
        assert!(check_doi(&book, &sub, "k").is_ok(), "a subtitle");
        assert!(check_doi(&entry("@misc{k, author = {A}}"), &review, "k").is_ok(), "no title to compare");
        assert!(same_work("Numba", "Numba: a LLVM-based Python JIT compiler"));
        assert!(same_work(
            "A Dynamical Theory of the Electromagnetic Field",
            "VIII. A dynamical theory of the electromagnetic field"
        ));
        assert!(same_work(
            "miepython: A Python library for Mie scattering",
            "miepython: Pure python calculation of Mie scattering"
        ));
        assert!(!same_work("Yet Another Scattering Framework", "AGBV/YASF: Initial Release"));
        assert!(!same_work(
            "Light Scattering by Nonspherical Particles: Theory",
            "Investigation of the single scattering approximation"
        ));
    }

    #[test]
    fn merging() {
        let (_, mut bib) =
            Entry::parse("@misc{k, title = {{RNA} world}, year = 2019, keywords = {mine}, eprint = {1}, note = {own}}")
                .unwrap();
        let (_, fresh) =
            Entry::parse("@article{x, title = {{{RNA} world}}, year = {2020}, journal = {Nature}, eprint = {1}}")
                .unwrap();
        let changes = merge(&mut bib, &fresh);
        assert_eq!(changes, ["type: misc → article", "year: 2019 → 2020", "journal: (none) → Nature"]);
        assert_eq!(bib.raw("title"), Some("{{RNA} world}"), "same text: your formatting stays");
        assert_eq!((bib.get("keywords").as_deref(), bib.get("note").as_deref()), (Some("mine"), Some("own")));
        assert!(merge(&mut bib, &fresh).is_empty(), "nothing left to change");
        let long = Entry::parse(&format!("@misc{{x, abstract = {{{}}}}}", "é".repeat(100))).unwrap().1;
        let changes = merge(&mut bib, &long);
        assert!(changes.iter().any(|c| c.starts_with("abstract") && c.ends_with('…')), "shortened on a char boundary");
    }
}
