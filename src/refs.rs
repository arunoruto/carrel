//! Where a paper's reference list comes from. Every source answers the same question through
//! [`ReferenceSource`], so they are interchangeable; [`sources`] lists the available ones, best first.

use crate::bib;
use crate::fetch::{self, Candidate, Ids};
use anyhow::{Context, Result, bail};
use serde_json::Value;

/// What a source gets to find a paper by.
pub struct Lookup<'a> {
    pub ids: &'a Ids,
    pub title: &'a str,
}

pub trait ReferenceSource {
    /// Its name, kept with every reference it finds so you know where each came from.
    fn name(&self) -> &'static str;
    /// The papers `paper` cites; empty when the source knows the paper but not its references.
    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>>;
}

/// The sources available here, best first: NASA ADS when there is a token, then OpenAlex and Semantic Scholar.
pub fn sources() -> Vec<Box<dyn ReferenceSource>> {
    let mut all: Vec<Box<dyn ReferenceSource>> = vec![];
    if let Some(token) = env(&["ADS_API_TOKEN", "ADS_DEV_KEY"]) {
        all.push(Box::new(Ads { token }));
    }
    all.push(Box::new(OpenAlex { api_key: env(&["OPENALEX_API_KEY"]) }));
    all.push(Box::new(SemanticScholar));
    all
}

fn env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok()).filter(|value| !value.trim().is_empty())
}

/// OpenAlex: free and broad. Without a key, everyone on an IP address shares a daily budget of about
/// 1000 requests; `OPENALEX_API_KEY` (free at openalex.org) gets its own.
pub struct OpenAlex {
    pub api_key: Option<String>,
}

impl OpenAlex {
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut request = fetch::agent().get(&format!("https://api.openalex.org/{path}"));
        for (key, value) in query {
            request = request.query(key, value);
        }
        if let Some(key) = &self.api_key {
            request = request.query("api_key", key);
        }
        match request.call() {
            Err(ureq::Error::StatusCode(404)) => bail!("doesn't know this paper"),
            Err(ureq::Error::StatusCode(429)) => {
                bail!("the free daily budget is used up; try tomorrow or set OPENALEX_API_KEY (free at openalex.org)")
            }
            response => Ok(response.context("OpenAlex")?.into_body().read_json()?),
        }
    }
}

impl ReferenceSource for OpenAlex {
    fn name(&self) -> &'static str {
        "openalex"
    }

    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>> {
        let doi = match (&paper.ids.doi, &paper.ids.arxiv) {
            (Some(doi), _) => doi.clone(),
            (None, Some(id)) => format!("10.48550/arXiv.{id}"),
            (None, None) => bail!("needs a DOI or arXiv id"),
        };
        let work = self.get(&format!("works/doi:{doi}"), &[("select", "referenced_works")])?;
        let works = work["referenced_works"].as_array().into_iter().flatten();
        let works: Vec<&str> = works.filter_map(|w| w.as_str()?.rsplit('/').next()).collect();
        let mut found = vec![];
        for chunk in works.chunks(50) {
            let filter = format!("openalex_id:{}", chunk.join("|"));
            let fields = "doi,title,publication_year,authorships,primary_location";
            let page = self.get("works", &[("filter", &filter), ("select", fields), ("per-page", "50")])?;
            found.extend(page["results"].as_array().into_iter().flatten().filter_map(openalex_work));
        }
        Ok(found)
    }
}

fn openalex_work(w: &Value) -> Option<Candidate> {
    let doi = w["doi"].as_str().map(|d| d.trim_start_matches("https://doi.org/").to_string());
    let arxiv = doi.as_deref().and_then(|d| d.to_lowercase().strip_prefix("10.48550/arxiv.").map(String::from));
    let names = w["authorships"].as_array().into_iter().flatten().filter_map(|a| a["author"]["display_name"].as_str());
    Some(Candidate {
        title: fetch::strip_tags(w["title"].as_str().filter(|t| !t.is_empty())?),
        authors: names.map(|n| bib::family(n).to_string()).collect(),
        year: w["publication_year"].as_i64(),
        venue: w["primary_location"]["source"]["display_name"].as_str().unwrap_or("").to_string(),
        ids: Ids { doi, arxiv },
        in_library: None,
    })
}

/// Semantic Scholar: good for computer science and preprints, but anonymous requests are often rate-limited.
pub struct SemanticScholar;

impl ReferenceSource for SemanticScholar {
    fn name(&self) -> &'static str {
        "semantic-scholar"
    }

    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>> {
        fetch::related(paper.ids, false)
    }
}

/// NASA ADS: the most complete reference lists in astronomy and planetary science, including
/// conference abstracts without a DOI. Needs a token (free at ui.adsabs.harvard.edu).
pub struct Ads {
    pub token: String,
}

impl Ads {
    fn query(&self, q: &str, fields: &str, rows: &str) -> Result<Vec<Value>> {
        let request = fetch::agent()
            .get("https://api.adsabs.harvard.edu/v1/search/query")
            .header("Authorization", &format!("Bearer {}", self.token))
            .query("q", q)
            .query("fl", fields)
            .query("rows", rows);
        let response: Value = match request.call() {
            Err(ureq::Error::StatusCode(401)) => bail!("the token was refused; check ADS_API_TOKEN"),
            Err(ureq::Error::StatusCode(429)) => bail!("the daily request limit is used up; try tomorrow"),
            response => response.context("NASA ADS")?.into_body().read_json()?,
        };
        Ok(response["response"]["docs"].as_array().cloned().unwrap_or_default())
    }
}

impl ReferenceSource for Ads {
    fn name(&self) -> &'static str {
        "ads"
    }

    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>> {
        let target = match (&paper.ids.doi, &paper.ids.arxiv) {
            (Some(doi), _) => format!("doi:\"{doi}\""),
            (None, Some(id)) => format!("identifier:\"arXiv:{id}\""),
            // Only by title: find its record first, so another paper's references never slip in.
            (None, None) if !paper.title.trim().is_empty() => {
                let docs = self.query(&format!("title:\"{}\"", paper.title.replace('"', "")), "bibcode,title", "10")?;
                let same =
                    docs.iter().find(|d| d["title"][0].as_str().is_some_and(|t| fetch::same_title(t, paper.title)));
                match same.and_then(|d| d["bibcode"].as_str()) {
                    Some(bibcode) => format!("bibcode:\"{bibcode}\""),
                    None => bail!("doesn't know this paper"),
                }
            }
            (None, None) => bail!("needs a DOI, arXiv id or title"),
        };
        let docs = self.query(&format!("references({target})"), "title,author,year,doi,pub,identifier", "2000")?;
        Ok(docs.iter().filter_map(ads_paper).collect())
    }
}

fn ads_paper(d: &Value) -> Option<Candidate> {
    let first = |key: &str| d[key][0].as_str().map(String::from);
    let identifiers = d["identifier"].as_array().into_iter().flatten().filter_map(Value::as_str);
    let arxiv = identifiers.filter_map(|i| i.strip_prefix("arXiv:")).find_map(fetch::arxiv_id);
    let names = d["author"].as_array().into_iter().flatten().filter_map(Value::as_str);
    Some(Candidate {
        title: fetch::strip_tags(&first("title")?),
        authors: names.map(|n| bib::family(n).to_string()).collect(),
        year: d["year"].as_str().and_then(|y| y.parse().ok()),
        venue: d["pub"].as_str().unwrap_or("").to_string(),
        ids: Ids { doi: first("doi"), arxiv },
        in_library: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openalex_works() {
        let w = json!({
            "doi": "https://doi.org/10.48550/arxiv.1706.03762",
            "title": "Attention Is <i>All</i> You Need",
            "publication_year": 2017,
            "authorships": [{"author": {"display_name": "Ashish Vaswani"}}],
            "primary_location": {"source": null},
        });
        let c = openalex_work(&w).unwrap();
        assert_eq!(
            (c.title.as_str(), c.authors[0].as_str(), c.year),
            ("Attention Is All You Need", "Vaswani", Some(2017))
        );
        assert_eq!(c.ids, Ids { doi: Some("10.48550/arxiv.1706.03762".into()), arxiv: Some("1706.03762".into()) });
        assert_eq!(c.venue, "");
        assert!(openalex_work(&json!({"doi": "https://doi.org/10.1/x", "title": null})).is_none(), "no title");
    }

    #[test]
    fn ads_papers() {
        let d = json!({
            "title": ["Ages and stratigraphy of lunar mare basalts"],
            "author": ["Hiesinger, H.", "Head, J. W., III"],
            "year": "2011",
            "doi": ["10.1130/2011.2477(01)"],
            "pub": "Recent Advances and Current Research Issues in Lunar Stratigraphy",
            "identifier": ["2011GSASP.477....1H", "arXiv:1101.00001v2"],
        });
        let c = ads_paper(&d).unwrap();
        assert_eq!(c.authors, ["Hiesinger", "Head"]);
        assert_eq!(
            (c.year, c.ids.doi.as_deref(), c.ids.arxiv.as_deref()),
            (Some(2011), Some("10.1130/2011.2477(01)"), Some("1101.00001"))
        );
        let abstract_only = ads_paper(&json!({"title": ["An LPSC abstract"], "year": "2020"})).unwrap();
        assert!(abstract_only.ids.is_empty(), "kept without ids: it still links by title");
    }
}
