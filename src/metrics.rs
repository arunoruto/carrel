//! How cited a paper is and how established its authors are, from OpenAlex, kept in `<key>/metrics.json`. The map
//! colours papers and authors by them. Counts grow, so a month on they are looked up again.

use crate::fetch::Ids;
use crate::refs::OpenAlex;
use crate::{Library, Paper};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FILE: &str = "metrics.json";
/// After this many seconds (30 days) the counts are looked up again.
pub const FRESH: u64 = 30 * 24 * 3600;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// When they were looked up, in seconds since 1970.
    pub at: u64,
    /// How many works cite the paper.
    pub cited_by: u64,
    pub authors: Vec<Author>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// As OpenAlex writes it.
    pub name: String,
    pub h_index: u32,
    pub works: u32,
    pub cited_by: u64,
}

impl Metrics {
    pub fn stale(&self) -> bool {
        now().saturating_sub(self.at) > FRESH
    }
}

/// A paper's metrics; none when not looked up yet or the file is damaged.
pub fn read(dir: &Path) -> Option<Metrics> {
    serde_json::from_str(&fs::read_to_string(dir.join(FILE)).ok()?).ok()
}

/// Two requests: the work, then all its authors at once.
pub fn look_up(ids: &Ids) -> Result<Metrics> {
    let doi = match (&ids.doi, &ids.arxiv) {
        (Some(doi), _) => doi.clone(),
        (None, Some(id)) => format!("10.48550/arXiv.{id}"),
        (None, None) => bail!("needs a DOI or arXiv id"),
    };
    let openalex = OpenAlex { api_key: crate::token(&["OPENALEX_API_KEY"], |t| t.openalex.as_ref())? };
    let work = openalex.get(&format!("works/doi:{doi}"), &[("select", "cited_by_count,authorships")])?;
    let ids: Vec<&str> = work["authorships"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["author"]["id"].as_str()?.rsplit('/').next())
        .take(50) // a page; collaborations of hundreds are counted by their first fifty
        .collect();
    let authors = match ids.is_empty() {
        true => Value::Null,
        false => openalex.get(
            "authors",
            &[
                ("filter", &format!("openalex_id:{}", ids.join("|"))),
                ("select", "display_name,summary_stats,works_count,cited_by_count"),
                ("per-page", "50"),
            ],
        )?,
    };
    Ok(parse(&work, &authors))
}

fn parse(work: &Value, authors: &Value) -> Metrics {
    let authors = authors["results"].as_array().into_iter().flatten().filter_map(|a| {
        Some(Author {
            name: a["display_name"].as_str().filter(|n| !n.is_empty())?.to_string(),
            h_index: a["summary_stats"]["h_index"].as_u64().unwrap_or(0) as u32,
            works: a["works_count"].as_u64().unwrap_or(0) as u32,
            cited_by: a["cited_by_count"].as_u64().unwrap_or(0),
        })
    });
    Metrics { at: now(), cited_by: work["cited_by_count"].as_u64().unwrap_or(0), authors: authors.collect() }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Library {
    /// Look up and save a paper's metrics.
    pub fn look_up_metrics(&self, p: &Paper) -> Result<Metrics> {
        let metrics = look_up(&Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() })?;
        fs::write(p.dir.join(FILE), serde_json::to_string_pretty(&metrics)? + "\n")?;
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openalex_counts() {
        let work = json!({"cited_by_count": 122093, "authorships": [{"author": {"id": "https://openalex.org/A1"}}]});
        let authors = json!({"results": [
            {"display_name": "Ashish Vaswani", "summary_stats": {"h_index": 30}, "works_count": 65, "cited_by_count": 135919},
            {"display_name": "", "summary_stats": {"h_index": 1}}
        ]});
        let m = parse(&work, &authors);
        assert_eq!(m.cited_by, 122093);
        assert_eq!(m.authors.len(), 1, "an author without a name is left out");
        assert_eq!((m.authors[0].h_index, m.authors[0].works), (30, 65));
        assert!(!m.stale() && Metrics { at: 0, ..m }.stale());
        // OpenAlex's names against the papers' spellings, for the authors' h-index on the map.
        use crate::authors::same_person;
        assert!(same_person("Vaswani, Ashish", "Ashish Vaswani") && same_person("Vaswani, A.", "Ashish Vaswani"));
        assert!(same_person("Kaiser, Lukasz", "Łukasz Kaiser"));
        assert!(!same_person("Vaswani, Ashish", "Noam Shazeer") && !same_person("Vaswani, B.", "Ashish Vaswani"));
    }
}
