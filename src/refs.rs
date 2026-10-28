//! Where a paper's reference list comes from. Every source answers the same question through
//! [`ReferenceSource`], so they are interchangeable; [`sources`] lists the available ones, best first.

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
    /// Asked only when the other sources found nothing.
    fn fallback(&self) -> bool {
        false
    }
}

/// The sources available here, best first: NASA ADS when there is a token, OpenAlex and Crossref, and
/// Semantic Scholar as a fallback.
pub fn sources() -> Result<Vec<Box<dyn ReferenceSource>>> {
    let mut all: Vec<Box<dyn ReferenceSource>> = vec![];
    if let Some(ads) = ads()? {
        all.push(Box::new(ads));
    }
    all.push(Box::new(OpenAlex { api_key: crate::token(&["OPENALEX_API_KEY"], |t| t.openalex.as_ref())? }));
    all.push(Box::new(Crossref));
    all.push(Box::new(SemanticScholar));
    Ok(all)
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
                bail!("the free daily budget is used up; try tomorrow or set an OpenAlex key (free at openalex.org)")
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
        authors: names.map(String::from).collect(),
        year: w["publication_year"].as_i64(),
        venue: w["primary_location"]["source"]["display_name"].as_str().unwrap_or("").to_string(),
        ids: Ids { doi, arxiv },
        in_library: None,
    })
}

/// Crossref: the reference lists publishers deposit. OpenAlex is built from them but keeps only the references it
/// can match to a record, so Crossref adds the rest: conference abstracts, books, reports. Some are just the
/// citation as printed.
pub struct Crossref;

impl ReferenceSource for Crossref {
    fn name(&self) -> &'static str {
        "crossref"
    }

    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>> {
        let Some(doi) = &paper.ids.doi else { bail!("needs a DOI") };
        let work: Value = match fetch::agent().get(&format!("https://api.crossref.org/works/{doi}")).call() {
            Err(ureq::Error::StatusCode(404)) => bail!("doesn't know this paper"),
            response => response.context("Crossref")?.into_body().read_json()?,
        };
        Ok(work["message"]["reference"].as_array().into_iter().flatten().filter_map(crossref_reference).collect())
    }
}

fn crossref_reference(r: &Value) -> Option<Candidate> {
    let text = |key: &str| r[key].as_str().map(fetch::strip_tags).filter(|s| !s.is_empty());
    // Some deposited DOIs carry the printed citation's punctuation: `10.1126/science.1178658)`.
    let doi = text("DOI").and_then(|d| fetch::find_doi(&d));
    let title = ["article-title", "volume-title", "series-title", "unstructured"].into_iter().find_map(text);
    // Only the first author, often as `Clark R.`.
    let author = text("author").map(|a| match a.rsplit_once(' ') {
        Some((family, initials))
            if !a.contains(',') && initials.chars().all(|c| c.is_uppercase() || ".-".contains(c)) =>
        {
            format!("{family}, {initials}")
        }
        _ => a,
    });
    Some(Candidate {
        title: title.or_else(|| doi.clone())?,
        authors: author.into_iter().collect(),
        year: text("year").and_then(|y| y.get(..4)?.parse().ok()),
        venue: text("journal-title").unwrap_or_default(),
        ids: Ids { doi, arxiv: None },
        in_library: None,
    })
}

/// Semantic Scholar: good for computer science and preprints, but anonymous requests are often rate-limited and
/// some publishers (Wiley) hide their references from it.
pub struct SemanticScholar;

impl ReferenceSource for SemanticScholar {
    fn name(&self) -> &'static str {
        "semantic-scholar"
    }

    fn references(&self, paper: &Lookup) -> Result<Vec<Candidate>> {
        fetch::related(paper.ids, false)
    }

    fn fallback(&self) -> bool {
        true
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
            Err(ureq::Error::StatusCode(401)) => bail!("the token was refused; check your ADS token"),
            Err(ureq::Error::StatusCode(429)) => bail!("the daily request limit is used up; try tomorrow"),
            response => response.context("NASA ADS")?.into_body().read_json()?,
        };
        Ok(response["response"]["docs"].as_array().cloned().unwrap_or_default())
    }
}

/// NASA ADS, when there is a token.
pub fn ads() -> Result<Option<Ads>> {
    Ok(crate::token(&["ADS_API_TOKEN", "ADS_DEV_KEY"], |t| t.ads.as_ref())?.map(|token| Ads { token }))
}

impl Ads {
    /// The DOI and arXiv id of the paper with this title, year (give or take one) and first author, if ADS
    /// knows them.
    /// Several papers share some titles, so without a year only a unique title counts.
    pub fn ids(&self, title: &str, year: Option<i32>, first_author: Option<&str>) -> Result<Ids> {
        let q = format!("title:\"{}\"", title.replace('"', ""));
        let docs = self.query(&q, "title,year,doi,identifier,first_author", "20")?;
        // Generic titles ("Classical Electrodynamics", 1998) need the author too.
        let family = |name: &str| fetch::title_key(crate::bib::family(name));
        let author_fits = |d: &Value| match (first_author, d["first_author"].as_str()) {
            (Some(a), Some(b)) => family(a) == family(b),
            _ => true,
        };
        let year_fits = |d: &Value| match (year, d["year"].as_str().and_then(|y| y.parse::<i32>().ok())) {
            (Some(a), Some(b)) => (a - b).abs() <= 1,
            _ => true,
        };
        let same: Vec<_> = docs
            .iter()
            .filter(|d| {
                d["title"][0].as_str().is_some_and(|t| fetch::same_title(t, title)) && year_fits(d) && author_fits(d)
            })
            .collect();
        // Without a year, only a title no other record shares.
        let [d, ..] = same[..] else { return Ok(Ids::default()) };
        if year.is_none() && same.len() > 1 {
            return Ok(Ids::default());
        }
        let identifiers = d["identifier"].as_array().into_iter().flatten().filter_map(Value::as_str);
        Ok(Ids {
            doi: d["doi"][0].as_str().map(String::from),
            arxiv: identifiers.filter_map(|i| i.strip_prefix("arXiv:")).find_map(fetch::arxiv_id),
        })
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
        authors: names.map(String::from).collect(),
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
            ("Attention Is All You Need", "Ashish Vaswani", Some(2017))
        );
        assert_eq!(c.ids, Ids { doi: Some("10.48550/arxiv.1706.03762".into()), arxiv: Some("1706.03762".into()) });
        assert_eq!(c.venue, "");
        assert!(openalex_work(&json!({"doi": "https://doi.org/10.1/x", "title": null})).is_none(), "no title");
    }

    #[test]
    fn crossref_references() {
        let printed = json!({"author": "Clark R.", "year": "2009a", "journal-title": "Lunar Planet. Sci.",
            "unstructured": "Clark R., Pieters C. M., Lunar Planet. Sci. XL, abstr. 2136 (2009)."});
        let c = crossref_reference(&printed).unwrap();
        assert_eq!((c.authors[0].as_str(), c.year, c.venue.as_str()), ("Clark, R.", Some(2009), "Lunar Planet. Sci."));
        assert!(
            c.title.starts_with("Clark R., Pieters") && c.ids.is_empty(),
            "the printed citation stands in for a title"
        );
        let only_doi = crossref_reference(&json!({"DOI": "10.1126/science.1178658", "key": "e_1"})).unwrap();
        assert_eq!(only_doi.title, "10.1126/science.1178658");
        let stray = crossref_reference(&json!({"DOI": "10.1126/science.1178105)"})).unwrap();
        assert_eq!(stray.ids.doi.as_deref(), Some("10.1126/science.1178105"), "punctuation from the printed citation");
        let book = crossref_reference(&json!({"volume-title": "Lunar Sourcebook", "author": "Heiken"})).unwrap();
        assert_eq!((book.title.as_str(), book.authors[0].as_str()), ("Lunar Sourcebook", "Heiken"));
        assert!(crossref_reference(&json!({"key": "nothing"})).is_none());
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
        assert_eq!(c.authors, ["Hiesinger, H.", "Head, J. W., III"]);
        assert_eq!(
            (c.year, c.ids.doi.as_deref(), c.ids.arxiv.as_deref()),
            (Some(2011), Some("10.1130/2011.2477(01)"), Some("1101.00001"))
        );
        let abstract_only = ads_paper(&json!({"title": ["An LPSC abstract"], "year": "2020"})).unwrap();
        assert!(abstract_only.ids.is_empty(), "kept without ids: it still links by title");
    }
}
