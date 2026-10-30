//! Where a paper's authors worked when they wrote it, from OpenAlex, kept in `<key>/affiliations.json`. The map
//! links papers through the institutions they share.

use crate::fetch::Ids;
use crate::refs::OpenAlex;
use crate::{Library, Paper};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const FILE: &str = "affiliations.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub institutions: Vec<Institution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Institution {
    pub name: String,
    /// Its ROR id (`https://ror.org/…`): one institution under several spellings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ror: Option<String>,
    /// ISO country code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl Institution {
    /// What tells two institutions apart: the ROR id, else the name.
    pub fn key(&self) -> String {
        self.ror.clone().unwrap_or_else(|| self.name.to_lowercase())
    }
}

/// A paper's affiliations; none when not looked up yet or the file is damaged.
pub fn read(dir: &Path) -> Option<Vec<Author>> {
    serde_json::from_str(&fs::read_to_string(dir.join(FILE)).ok()?).ok()
}

pub fn write(dir: &Path, authors: &[Author]) -> Result<()> {
    Ok(fs::write(dir.join(FILE), serde_json::to_string_pretty(authors)? + "\n")?)
}

/// The authors OpenAlex lists for a paper, each with their institutions.
pub fn look_up(ids: &Ids) -> Result<Vec<Author>> {
    let doi = match (&ids.doi, &ids.arxiv) {
        (Some(doi), _) => doi.clone(),
        (None, Some(id)) => format!("10.48550/arXiv.{id}"),
        (None, None) => bail!("needs a DOI or arXiv id"),
    };
    let openalex = OpenAlex { api_key: crate::token(&["OPENALEX_API_KEY"], |t| t.openalex.as_ref())? };
    Ok(parse(&openalex.get(&format!("works/doi:{doi}"), &[("select", "authorships")])?))
}

fn parse(work: &Value) -> Vec<Author> {
    let authorships = work["authorships"].as_array().into_iter().flatten();
    let authors = authorships.filter_map(|a| {
        let institutions = a["institutions"].as_array().into_iter().flatten().filter_map(|i| {
            Some(Institution {
                name: i["display_name"].as_str().filter(|n| !n.is_empty())?.to_string(),
                ror: i["ror"].as_str().map(String::from),
                country: i["country_code"].as_str().map(String::from),
            })
        });
        let name = a["author"]["display_name"].as_str().filter(|n| !n.is_empty())?.to_string();
        Some(Author { name, institutions: institutions.collect() })
    });
    authors.collect()
}

impl Library {
    /// Look up and save a paper's affiliations. Returns how many institutions it has.
    pub fn look_up_affiliations(&self, p: &Paper) -> Result<usize> {
        let authors = look_up(&Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() })?;
        write(&p.dir, &authors)?;
        let mut keys: Vec<String> = authors.iter().flat_map(|a| a.institutions.iter().map(Institution::key)).collect();
        keys.sort();
        keys.dedup();
        Ok(keys.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openalex_authorships() {
        let work = json!({"authorships": [
            {"author": {"display_name": "Ada Lovelace"}, "institutions": [
                {"display_name": "TU Dortmund University", "ror": "https://ror.org/01k97gp34", "country_code": "DE"},
                {"display_name": ""}
            ]},
            {"author": {"display_name": "Charles Babbage"}, "institutions": []},
            {"author": {"display_name": null}, "institutions": [{"display_name": "Nowhere"}]}
        ]});
        let authors = parse(&work);
        assert_eq!(authors.len(), 2, "an author without a name is left out");
        assert_eq!(authors[0].institutions.len(), 1, "an institution without a name is left out");
        assert_eq!(authors[0].institutions[0].key(), "https://ror.org/01k97gp34");
        assert!(authors[1].institutions.is_empty());

        let dir = std::env::temp_dir().join(format!("carrel-affiliations-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir).is_none());
        write(&dir, &authors).unwrap();
        assert_eq!(read(&dir).unwrap(), authors);
        let text = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains("\"institutions\": []"), "empty lists are left out of the file");
        fs::write(dir.join(FILE), "{").unwrap();
        assert!(read(&dir).is_none(), "a damaged file counts as not looked up");
        fs::remove_dir_all(&dir).unwrap();
    }
}
