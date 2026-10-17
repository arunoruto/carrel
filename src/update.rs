//! Refreshing a paper's metadata, for instance once its arXiv preprint has been published.

use crate::bib::Entry;
use crate::{Library, citekey, fetch};
use anyhow::{Result, bail};

impl Library {
    /// Fetch a paper's metadata again and merge it into its .bib: fetched fields replace those with
    /// the same name when their text differs, everything else (keywords, your own fields) stays, and so
    /// does the key. `source` (a DOI, arXiv id or exact title) says which paper it is, for papers added
    /// without one. A preprint switches to its published version when Semantic Scholar knows one.
    /// Returns what changed.
    pub fn update(&self, id: &str, source: Option<&str>) -> Result<Vec<String>> {
        let mut p = self.get(id)?;
        let mut ids = match source.map(fetch::classify).transpose()? {
            Some(fetch::Source::Doi(doi)) => fetch::Ids { doi: Some(doi), arxiv: None },
            Some(fetch::Source::Arxiv(arxiv)) => fetch::Ids { doi: None, arxiv: Some(arxiv) },
            Some(fetch::Source::Title(title)) => fetch::by_title(&title)?,
            Some(fetch::Source::Pdf(_)) => bail!("to give a paper its PDF, use `carrel attach`"),
            None => fetch::Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() },
        };
        if ids.is_empty() {
            bail!("{id} has no DOI or arXiv id; say which paper it is: carrel update {id} <DOI, arXiv id or title>");
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

        let changes = merge(&mut p.bib, &fetch::metadata(&ids)?);
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
