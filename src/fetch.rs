//! Working out what a paper is: identifiers in text, metadata from doi.org, search on Crossref and Semantic Scholar.

use crate::bib::{self, Entry};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub enum Source {
    Pdf(PathBuf),
    Doi(String),
    Arxiv(String),
    Title(String),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Ids {
    pub doi: Option<String>,
    pub arxiv: Option<String>,
}

impl Ids {
    pub fn is_empty(&self) -> bool {
        self.doi.is_none() && self.arxiv.is_none()
    }

    /// What to hand `carrel add` for this paper.
    pub fn source(&self) -> String {
        self.doi.clone().or(self.arxiv.as_ref().map(|id| format!("arXiv:{id}"))).unwrap_or_default()
    }
}

/// A search result.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub title: String,
    /// Full names as the source writes them (`Carle M. Pieters`, `Pieters, C. M.`).
    pub authors: Vec<String>,
    pub year: Option<i64>,
    pub venue: String,
    pub ids: Ids,
    /// Its id, when it is already in the library.
    pub in_library: Option<String>,
}

impl fmt::Display for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let authors = match self.authors.as_slice() {
            [] => String::new(),
            [a] => bib::family(a).to_string(),
            [a, ..] => format!("{} et al.", bib::family(a)),
        };
        let year = self.year.map(|y| y.to_string()).unwrap_or_default();
        let details: Vec<&str> = [authors.as_str(), &year, &self.venue].into_iter().filter(|s| !s.is_empty()).collect();
        write!(f, "{} ({})  {}", self.title, details.join(", "), self.ids.source())
    }
}

/// What the user handed us: a file, an arXiv id/URL, a DOI/URL, or else a title.
pub fn classify(input: &str) -> Result<Source> {
    // Files dragged into a terminal arrive quoted or as file:// URLs.
    let s = input.trim().trim_matches(['\'', '"']);
    let s = s.strip_prefix("file://").unwrap_or(s);
    let path = match s.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(s),
    };
    if path.is_file() {
        return Ok(Source::Pdf(path));
    }
    let lower = s.to_lowercase();
    let after = ["arxiv.org/abs/", "arxiv.org/pdf/", "arxiv:"].iter().find_map(|p| lower.split_once(p));
    if let Some(id) = arxiv_id(after.map_or(lower.as_str(), |(_, rest)| rest)) {
        return Ok(Source::Arxiv(id));
    }
    if let Some(doi) = find_doi(s) {
        return Ok(Source::Doi(doi));
    }
    if s.is_empty() || s.starts_with(['/', '~', '.']) || lower.ends_with(".pdf") {
        bail!("no such file: {s}");
    }
    Ok(Source::Title(s.to_string()))
}

/// The archives of old-style arXiv ids (`hep-th/9711200`), used until 2007.
const OLD_ARCHIVES: [&str; 34] = [
    "acc-phys", "adap-org", "alg-geom", "ao-sci", "astro-ph", "atom-ph", "bayes-an", "chao-dyn", "chem-ph", "cmp-lg",
    "comp-gas", "cond-mat", "cs", "dg-ga", "funct-an", "gr-qc", "hep-ex", "hep-lat", "hep-ph", "hep-th", "math",
    "math-ph", "mtrl-th", "nlin", "nucl-ex", "nucl-th", "patt-sol", "physics", "plasm-ph", "q-alg", "q-bio",
    "quant-ph", "solv-int", "supr-con",
];

/// The arXiv id at the start of `s` (lowercase), without its version: `2101.00001` or `hep-th/9711200`.
pub(crate) fn arxiv_id(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let digits = |from: usize| b[from.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let num = digits(5);
    if digits(0) == 4 && b.get(4) == Some(&b'.') && (4..=5).contains(&num) {
        return Some(s[..5 + num].to_string());
    }
    // Old style: archive, an optional subject class (math.AG), a slash, seven digits.
    let (archive, rest) = s.split_once('/')?;
    let archive = archive.split('.').next()?;
    let number = rest.get(..7).filter(|n| n.bytes().all(|c| c.is_ascii_digit()))?;
    let more = rest[7..].starts_with(|c: char| c.is_ascii_digit());
    (OLD_ARCHIVES.contains(&archive) && !more).then(|| format!("{archive}/{number}"))
}

/// The first DOI in `text`: bare, `doi:...` or a doi.org URL.
pub fn find_doi(text: &str) -> Option<String> {
    let text = text.replace("\\_", "_"); // Markdown escapes
    text.match_indices("10.").find_map(|(i, _)| {
        if text[..i].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let rest = &text[i..];
        let end = rest.find(|c: char| c.is_whitespace() || "\"<>{}|^`[]".contains(c)).unwrap_or(rest.len());
        let mut doi = rest[..end].trim_end_matches(['.', ',', ';', ':', '\'']);
        // A closing parenthesis is the DOI's own only if the DOI opened one: "(see 10.1/x)" vs "10.1/x(1)".
        while doi.ends_with(')') && doi.matches('(').count() < doi.matches(')').count() {
            doi = doi[..doi.len() - 1].trim_end_matches(['.', ',', ';', ':', '\'']);
        }
        let (prefix, suffix) = doi.split_once('/')?;
        let valid = prefix.len() >= 7 && prefix[3..].bytes().all(|c| c.is_ascii_digit() || c == b'.');
        (valid && !suffix.is_empty()).then(|| doi.to_string())
    })
}

/// Identifiers printed near the start of a paper. Later ones mostly belong to its references.
pub fn find_ids(text: &str) -> Ids {
    let head: String = text.chars().take(6000).collect();
    let lower = head.to_lowercase();
    let arxiv = lower.match_indices("arxiv").find_map(|(i, _)| {
        // "arXiv:2101.00001", "arxiv.org/abs/2101.00001", ...
        arxiv_id(lower[i + 5..].trim_start_matches([':', '.', '/', ' ', 'o', 'r', 'g', 'a', 'b', 's', 'p', 'd', 'f']))
    });
    Ids { doi: find_doi(&head), arxiv }
}

/// Titles match when their letters and digits do: case, punctuation, accents and ligatures don't count.
/// Strict on purpose, since a wrong match attaches the wrong metadata.
pub fn same_title(a: &str, b: &str) -> bool {
    let a = title_key(a);
    !a.is_empty() && a == title_key(b)
}

/// What identifies a paper: its DOI (lowercased), arXiv id and title key. Two papers sharing any key are the same.
pub fn paper_keys(ids: &Ids, title: &str) -> Vec<String> {
    let keys = [ids.doi.as_ref().map(|d| d.to_lowercase()), ids.arxiv.clone(), Some(title_key(title))];
    keys.into_iter().flatten().filter(|k| !k.is_empty()).collect()
}

/// A title's letters and digits, lowercased: equal for titles that [`same_title`] matches.
pub fn title_key(title: &str) -> String {
    deunicode::deunicode(&strip_tags(title))
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_lowercase()
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("carrel/", env!("CARGO_PKG_VERSION"), " (personal paper library)"))
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

pub fn download(url: &str, dest: &Path) -> Result<()> {
    let mut body = agent().get(url).call().with_context(|| format!("cannot download {url}"))?.into_body();
    std::io::copy(&mut body.as_reader(), &mut std::fs::File::create(dest)?)?;
    Ok(())
}

pub fn download_arxiv(id: &str, dest: &Path) -> Result<()> {
    download(&format!("https://arxiv.org/pdf/{id}"), dest)?;
    if !std::fs::read(dest)?.starts_with(b"%PDF") {
        bail!("arXiv did not return a PDF for {id}");
    }
    Ok(())
}

/// A BibTeX entry from doi.org, which covers Crossref, DataCite and others. arXiv papers use their DataCite DOI.
pub fn metadata(ids: &Ids) -> Result<Entry> {
    let doi = match (&ids.doi, &ids.arxiv) {
        (Some(doi), _) => doi.clone(),
        (None, Some(id)) => format!("10.48550/arXiv.{id}"),
        (None, None) => bail!("no DOI or arXiv id"),
    };
    let response = agent()
        .get(&format!("https://doi.org/{doi}"))
        .header("Accept", "application/vnd.citationstyles.csl+json")
        .call();
    let csl: Value = response.with_context(|| format!("cannot look up DOI {doi}"))?.into_body().read_json()?;
    let arxiv_doi = doi.to_lowercase().strip_prefix("10.48550/arxiv.").map(String::from);
    let arxiv = ids.arxiv.clone().or(arxiv_doi.clone());
    Ok(from_csl(&csl, &doi, arxiv.as_deref(), arxiv_doi.is_some()))
}

/// CSL-JSON (what doi.org returns) to BibTeX, in the field order people expect.
fn from_csl(v: &Value, doi: &str, arxiv: Option<&str>, preprint: bool) -> Entry {
    let text = |key: &str| -> Option<String> {
        let s = match &v[key] {
            Value::String(s) => strip_tags(s),
            Value::Number(n) => n.to_string(),
            Value::Array(a) => strip_tags(a.first()?.as_str()?),
            _ => return None,
        };
        Some(s).filter(|s| !s.is_empty())
    };
    let kind = match v["type"].as_str().unwrap_or("") {
        _ if preprint => "misc",
        "article-journal" | "journal-article" => "article",
        "paper-conference" | "proceedings-article" => "inproceedings",
        "chapter" | "book-chapter" => "incollection",
        "book" | "monograph" => "book",
        "thesis" | "dissertation" => "phdthesis",
        "report" => "techreport",
        _ => "misc",
    };
    let authors: Vec<String> = v["author"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let family = a["family"].as_str().map(|f| match a["given"].as_str() {
                Some(g) => format!("{f}, {g}"),
                None => f.to_string(),
            });
            family.or_else(|| a["literal"].as_str().map(String::from))
        })
        .collect();
    let year = &v["issued"]["date-parts"][0][0];
    let year = year.as_i64().or_else(|| year.as_str()?.parse().ok());

    // Crossref keeps subtitles apart.
    let title = match (text("title"), text("subtitle")) {
        (Some(title), Some(sub)) if !title.contains(&sub) => Some(format!("{title}: {sub}")),
        (title, _) => title,
    };
    let mut e = Entry::new(kind);
    e.set("title", title.as_deref());
    e.set("author", Some(&authors.join(" and ")));
    e.set("year", year.map(|y| y.to_string()).as_deref());
    match kind {
        "article" => e.set("journal", text("container-title").as_deref()),
        "inproceedings" | "incollection" => e.set("booktitle", text("container-title").as_deref()),
        _ => {}
    }
    e.set("volume", text("volume").as_deref());
    e.set("number", text("issue").as_deref());
    // APS and others number articles instead of giving pages.
    let pages = text("page").or_else(|| text("article-number"));
    e.set("pages", pages.map(|p| p.replace('-', "--")).as_deref());
    if !matches!(kind, "article" | "misc") {
        e.set("publisher", text("publisher").as_deref());
    }
    e.set("doi", Some(doi));
    if let Some(id) = arxiv {
        e.set("eprint", Some(id));
        e.set("archiveprefix", Some("arXiv"));
    }
    e.set("url", text("URL").filter(|u| !u.contains("doi.org")).as_deref());
    let abstract_text = text("abstract").map(|a| a.strip_prefix("Abstract ").map(String::from).unwrap_or(a));
    e.set("abstract", abstract_text.as_deref());
    e
}

/// Crossref titles and abstracts carry JATS/HTML tags, and so does extracted text (`Fe<sup>0</sup>`).
pub(crate) fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// HTML entities, which Crossref leaves in some titles (`&amp;`, `&lt;`, `&#8211;`).
fn decode_entities(text: &str) -> String {
    let (mut out, mut rest) = (String::new(), text);
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&end| end <= 10);
        let decoded = end.and_then(|end| match &rest[1..end] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with("#x") || n.starts_with("#X") => {
                u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32)
            }
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (decoded, end) {
            (Some(c), Some(end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// The ids of the paper with exactly this title if there is one, and the papers found on the way.
pub fn exact_title(title: &str) -> Result<(Option<Ids>, Vec<Candidate>)> {
    let found = search(title)?;
    if let Some(c) = found.iter().find(|c| same_title(&c.title, title)) {
        return Ok((Some(c.ids.clone()), found));
    }
    // Relevance search can rank the paper itself below others with similar titles; this can't.
    let fields = [("query", title), ("fields", "title,year,venue,authors,externalIds")];
    let matched =
        semantic_scholar("paper/search/match", &fields, 2).ok().and_then(|v| scholar_candidate(&v["data"][0]));
    Ok((matched.filter(|c| same_title(&c.title, title)).map(|c| c.ids), found))
}

/// The ids of the paper with exactly this title, or an error listing the closest matches.
pub fn by_title(title: &str) -> Result<Ids> {
    match exact_title(title)? {
        (Some(ids), _) => Ok(ids),
        (None, found) if found.is_empty() => bail!("found no paper titled {title:?}"),
        (None, found) => {
            let list: Vec<String> = found.iter().map(|c| format!("  {c}")).collect();
            bail!("no paper is titled exactly {title:?}; use one of these ids:\n{}", list.join("\n"))
        }
    }
}

/// Find papers by title or topic. Crossref knows everything with a DOI, Semantic Scholar also arXiv-only
/// papers. Exact title matches come first.
pub fn search(query: &str) -> Result<Vec<Candidate>> {
    let (crossref, scholar) = std::thread::scope(|s| {
        let crossref = s.spawn(|| crossref_search(query));
        // When Semantic Scholar is unavailable, arXiv's own search still finds arXiv papers.
        let scholar = semantic_scholar_search(query).or_else(|_| arxiv_search(query));
        (crossref.join().unwrap_or_else(|_| Err(anyhow::anyhow!("Crossref lookup crashed"))), scholar)
    });
    if let (Err(e), Err(_)) = (&crossref, &scholar) {
        bail!("search failed: {e:#}");
    }
    Ok(merge_results(query, scholar.unwrap_or_default(), crossref.unwrap_or_default()))
}

/// One list from two sources: each paper once (by DOI or title), exact title matches first.
fn merge_results(query: &str, first: Vec<Candidate>, second: Vec<Candidate>) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = vec![];
    for c in first.into_iter().chain(second) {
        let doi = |x: &Candidate| x.ids.doi.as_ref().map(|d| d.to_lowercase());
        let known = |f: &Candidate| doi(f).is_some() && doi(f) == doi(&c) || same_title(&f.title, &c.title);
        if !found.iter().any(known) {
            found.push(c);
        }
    }
    found.sort_by_key(|c| !same_title(&c.title, query)); // stable: otherwise the sources' order
    found
}

fn crossref_search(query: &str) -> Result<Vec<Candidate>> {
    let response = agent()
        .get("https://api.crossref.org/works")
        .query("query.bibliographic", query)
        .query("rows", "8")
        .query("select", "DOI,title,subtitle,author,issued,container-title")
        .call();
    let v: Value = response.context("Crossref")?.into_body().read_json()?;
    Ok(crossref_candidates(&v))
}

fn crossref_candidates(v: &Value) -> Vec<Candidate> {
    let items = v["message"]["items"].as_array().cloned().unwrap_or_default();
    let found = items.iter().map(|it| Candidate {
        title: match (strip_tags(it["title"][0].as_str().unwrap_or("")), it["subtitle"][0].as_str()) {
            (title, Some(sub)) if !sub.is_empty() && !title.contains(sub) => format!("{title}: {}", strip_tags(sub)),
            (title, _) => title,
        },
        authors: it["author"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                let family = a["family"].as_str()?;
                Some(a["given"].as_str().map_or(family.to_string(), |given| format!("{family}, {given}")))
            })
            .collect(),
        year: it["issued"]["date-parts"][0][0].as_i64(),
        venue: it["container-title"][0].as_str().unwrap_or("").to_string(),
        ids: Ids { doi: it["DOI"].as_str().map(String::from), arxiv: None },
        in_library: None,
    });
    // Supplementary files have DOIs too; they are titled like files.
    found.filter(|c| !c.title.is_empty() && !c.ids.is_empty() && !c.title.ends_with(".pdf")).collect()
}

fn semantic_scholar_search(query: &str) -> Result<Vec<Candidate>> {
    let fields = [("query", query), ("limit", "8"), ("fields", "title,year,venue,authors,externalIds")];
    let v = semantic_scholar("paper/search", &fields, 1)?; // someone is waiting: fall back soon
    Ok(v["data"].as_array().into_iter().flatten().filter_map(scholar_candidate).collect())
}

/// The DOI of an arXiv preprint's published version, when Semantic Scholar knows one.
pub fn published_doi(arxiv: &str) -> Result<Option<String>> {
    let published = |doi: Option<String>| doi.filter(|d| !d.to_lowercase().starts_with("10.48550/"));
    match semantic_scholar(&format!("paper/ARXIV:{arxiv}"), &[("fields", "externalIds")], 4) {
        Ok(v) => Ok(published(v["externalIds"]["DOI"].as_str().map(String::from))),
        // arXiv knows the journal DOI too, when the authors gave it.
        Err(scholar) => match arxiv_query(&[("id_list", arxiv)]) {
            Ok(found) => Ok(published(found.into_iter().next().and_then(|c| c.ids.doi))),
            Err(_) => Err(scholar),
        },
    }
}

/// The papers a paper cites, or with `cited_by` the papers citing it, from Semantic Scholar.
pub fn related(ids: &Ids, cited_by: bool) -> Result<Vec<Candidate>> {
    let paper = match (&ids.arxiv, &ids.doi) {
        (Some(id), _) => format!("ARXIV:{id}"),
        (None, Some(doi)) => format!("DOI:{doi}"),
        (None, None) => bail!("needs a DOI or arXiv id to look up its citations"),
    };
    let (endpoint, field) = if cited_by { ("citations", "citingPaper") } else { ("references", "citedPaper") };
    let fields = [("fields", "title,year,venue,authors,externalIds"), ("limit", "500")];
    let v = semantic_scholar(&format!("paper/{paper}/{endpoint}"), &fields, 4)?;
    Ok(v["data"].as_array().into_iter().flatten().filter_map(|d| scholar_paper(&d[field])).collect())
}

/// A Semantic Scholar Graph API call, waiting out its rate limit up to `retries` times (1, 2, 4, 8 s).
fn semantic_scholar(path: &str, query: &[(&str, &str)], retries: u32) -> Result<Value> {
    let mut wait = 1;
    loop {
        let mut request = agent().get(&format!("https://api.semanticscholar.org/graph/v1/{path}"));
        for (key, value) in query {
            request = request.query(key, value);
        }
        // Anonymous requests share one global quota; a free key gets its own.
        if let Some(key) = crate::token(&["S2_API_KEY"], |t| t.s2.as_ref())? {
            request = request.header("x-api-key", key);
        }
        match request.call() {
            Err(ureq::Error::StatusCode(429)) if wait < 1 << retries => {
                std::thread::sleep(Duration::from_secs(wait));
                wait *= 2;
            }
            Err(ureq::Error::StatusCode(429)) => {
                bail!("Semantic Scholar is rate-limiting anonymous requests; try later or add an S2 key")
            }
            Err(ureq::Error::StatusCode(404)) => bail!("Semantic Scholar doesn't know this paper"),
            response => return Ok(response.context("Semantic Scholar")?.into_body().read_json()?),
        }
    }
}

/// arXiv's search over titles.
fn arxiv_search(query: &str) -> Result<Vec<Candidate>> {
    let words: Vec<String> =
        query.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(|w| format!("ti:{w}")).collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    arxiv_query(&[("search_query", &words.join(" AND ")), ("max_results", "8")])
}

/// An arXiv API query; its answer is an Atom feed. arXiv asks for at most one request every 3 seconds.
fn arxiv_query(query: &[(&str, &str)]) -> Result<Vec<Candidate>> {
    let mut attempts = 0;
    let feed = loop {
        let mut request = agent().get("https://export.arxiv.org/api/query");
        for (key, value) in query {
            request = request.query(key, value);
        }
        match request.call() {
            Err(ureq::Error::StatusCode(429 | 503)) if attempts < 2 => {
                attempts += 1;
                std::thread::sleep(Duration::from_secs(3));
            }
            response => break response.context("arXiv")?.into_body().read_to_string()?,
        }
    };
    Ok(feed.split("<entry>").skip(1).filter_map(arxiv_entry).collect())
}

/// One `<entry>` of an arXiv Atom feed.
fn arxiv_entry(entry: &str) -> Option<Candidate> {
    let all = |tag: &str| -> Vec<String> {
        let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
        let mut found = vec![];
        let mut rest = entry;
        while let Some(start) = rest.find(&open) {
            let Some(body) = rest[start..].find('>').map(|i| start + i + 1) else { break };
            let Some(end) = rest[body..].find(&close).map(|i| body + i) else { break };
            found.push(decode_entities(&rest[body..end].split_whitespace().collect::<Vec<_>>().join(" ")));
            rest = &rest[end..];
        }
        found
    };
    let first = |tag: &str| all(tag).into_iter().next();
    let id = arxiv_id(first("id")?.rsplit_once("/abs/")?.1)?;
    Some(Candidate {
        title: first("title")?,
        authors: all("name"),
        year: first("published").and_then(|p| p.get(..4)?.parse().ok()),
        venue: "arXiv".into(),
        ids: Ids { doi: first("arxiv:doi"), arxiv: Some(id) },
        in_library: None,
    })
}

/// A search result needs an id to be added by.
fn scholar_candidate(p: &Value) -> Option<Candidate> {
    scholar_paper(p).filter(|c| !c.ids.is_empty())
}

/// A paper as Semantic Scholar describes it; references without ids still link by title.
fn scholar_paper(p: &Value) -> Option<Candidate> {
    let id = |key: &str| p["externalIds"][key].as_str().map(String::from);
    let names = p["authors"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str());
    Some(Candidate {
        title: p["title"].as_str()?.to_string(),
        authors: names.map(String::from).collect(),
        year: p["year"].as_i64(),
        venue: p["venue"].as_str().unwrap_or("").to_string(),
        ids: Ids { doi: id("DOI"), arxiv: id("ArXiv") },
        in_library: None,
    })
}

#[cfg(test)]
/// A one-request-per-reply HTTP server for tests: its address, and the requests it got (head and body).
pub(crate) fn fake_server(replies: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut requests = vec![];
        for (status, reply) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let (mut head, mut length) = (String::new(), 0);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            requests.push(head + &String::from_utf8(body).unwrap());
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (api, server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert_eq!(find_doi("see https://doi.org/10.1038/nature14539."), Some("10.1038/nature14539".into()));
        assert_eq!(find_doi("[x](https://doi.org/10.1145/3292500.3330701)"), Some("10.1145/3292500.3330701".into()));
        assert_eq!(find_doi("version 210.12/abc and 10.12/x"), None);
        assert_eq!(arxiv_id("1706.03762v7 [cs.CL]"), Some("1706.03762".into()));
        assert_eq!(arxiv_id("10.1038/nature"), None);
        let ids = find_ids("arXiv preprint arXiv:2006.11239v2 [cs.LG]\nDOI: 10.5555/12345");
        assert_eq!(ids, Ids { doi: Some("10.5555/12345".into()), arxiv: Some("2006.11239".into()) });
        assert!(
            matches!(classify("https://arxiv.org/abs/1706.03762v7").unwrap(), Source::Arxiv(id) if id == "1706.03762")
        );
        assert!(matches!(classify("1706.03762").unwrap(), Source::Arxiv(_)));
        assert!(matches!(classify("doi:10.1038/nature14539").unwrap(), Source::Doi(d) if d == "10.1038/nature14539"));
        assert!(matches!(classify("Attention is all you need").unwrap(), Source::Title(_)));
        assert!(classify("~/no-such-file.pdf").is_err());
        assert!(same_title("Attention Is All You Need", "Attention is All you Need."));
        assert!(same_title("Eﬃcient Pre-training", "Efficient Pretraining"));
        assert!(!same_title("Attention Is All You Need", "Is Attention All You Need?"));
        assert_eq!(strip_tags("<jats:p>Deep <i>learning</i></jats:p>"), "Deep learning");
    }

    #[test]
    fn more_identifiers() {
        // DOIs with their own parentheses, in parentheses, in URLs, with Markdown escapes.
        assert_eq!(find_doi("(see 10.1016/S0140-6736(20)30183-5)."), Some("10.1016/S0140-6736(20)30183-5".into()));
        assert_eq!(find_doi("(doi:10.1000/xyz)"), Some("10.1000/xyz".into()));
        assert_eq!(find_doi("https://dx.doi.org/10.1000/a\\_b"), Some("10.1000/a_b".into()));
        assert_eq!(find_doi("no doi, just 10.5 and 3.10/4"), None);
        // Old-style arXiv ids, with subject class, version and URL forms.
        assert!(matches!(classify("hep-th/9711200").unwrap(), Source::Arxiv(id) if id == "hep-th/9711200"));
        assert!(matches!(classify("arXiv:math.AG/0601001v2").unwrap(), Source::Arxiv(id) if id == "math/0601001"));
        assert!(
            matches!(classify("https://arxiv.org/abs/cond-mat/0303516").unwrap(), Source::Arxiv(id) if id == "cond-mat/0303516")
        );
        assert!(
            matches!(classify("https://arxiv.org/pdf/2101.00001v3.pdf").unwrap(), Source::Arxiv(id) if id == "2101.00001")
        );
        assert_eq!(arxiv_id("hep-th/97112001"), None, "eight digits");
        assert_eq!(arxiv_id("cooking/9711200"), None, "not an arXiv archive");
        assert_eq!(arxiv_id("2101.000011"), None);
        assert_eq!(find_ids("Preprint arXiv:hep-th/9711200v3 [hep-th]").arxiv.as_deref(), Some("hep-th/9711200"));
        // Things that aren't identifiers become title searches, mistyped paths don't.
        assert!(
            matches!(classify("  'Attention Is All You Need'  ").unwrap(), Source::Title(t) if t == "Attention Is All You Need")
        );
        assert!(
            classify("./missing.pdf").is_err() && classify("/no/such/paper.PDF").is_err() && classify("   ").is_err()
        );
        assert_eq!(decode_entities("Q&amp;A &lt;3 &#8211; &#x2014; &bogus; a&b"), "Q&A <3 – — &bogus; a&b");
    }

    #[test]
    fn metadata_from_csl() {
        let crossref = serde_json::json!({
            "type": "journal-article", "title": ["Gravitational waves &amp; <i>black holes</i>"],
            "subtitle": ["A first look"], "container-title": ["Physical Review Letters"],
            "author": [{"family": "Abbott", "given": "B. P."}, {"literal": "LIGO Scientific Collaboration"}, {"given": "Nobody"}],
            "issued": {"date-parts": [["2016", 2, 11]]}, "volume": 116, "issue": "6", "article-number": "061102",
            "URL": "http://dx.doi.org/10.1103/x", "abstract": "<jats:title>Abstract</jats:title><jats:p>We observe 100% of it.</jats:p>",
        });
        let e = from_csl(&crossref, "10.1103/x", None, false);
        assert_eq!(e.kind, "article");
        assert_eq!(e.title(), "Gravitational waves & black holes: A first look");
        assert_eq!(e.authors(), ["Abbott, B. P.", "LIGO Scientific Collaboration"]);
        assert_eq!(
            (e.year(), e.get("volume").as_deref(), e.get("pages").as_deref()),
            (Some(2016), Some("116"), Some("061102"))
        );
        assert_eq!(e.get("journal").as_deref(), Some("Physical Review Letters"));
        assert_eq!(e.get("url"), None, "doi.org links are redundant with the DOI");
        assert_eq!(e.get("abstract").as_deref(), Some("We observe 100% of it."));
        assert!(e.to_bib("k").contains(r"100\%"), "escaped for LaTeX");

        let datacite = serde_json::json!({"type": "article", "title": r"Canonical expansions of $\mathbb R_{\mathcal Q}$-germs",
            "author": [{"family": "Le Gal", "given": "Olivier"}], "issued": {"date-parts": [[2026]]}});
        let e = from_csl(&datacite, "10.48550/arXiv.2609.13189", Some("2609.13189"), true);
        assert_eq!(e.kind, "misc");
        assert!(
            e.to_bib("k").contains(r"{{Canonical expansions of $\mathbb R_{\mathcal Q}$-germs}}"),
            "math stays LaTeX"
        );
        assert_eq!((e.arxiv().as_deref(), e.venue().as_deref()), (Some("2609.13189"), Some("arXiv")));
        assert_eq!(from_csl(&serde_json::json!({}), "10.1/x", None, false).title(), "", "empty metadata doesn't panic");
    }

    #[test]
    fn search_results() {
        let crossref = serde_json::json!({"message": {"items": [
            {"DOI": "10.1/a", "title": ["Deep learning"], "subtitle": ["Methods"], "author": [{"family": "LeCun"}], "issued": {"date-parts": [[2015]]}},
            {"DOI": "10.1/b", "title": ["Figure 3.pdf"]},
            {"title": ["No DOI"]},
            {"DOI": "10.1/c"},
        ]}});
        let found = crossref_candidates(&crossref);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].title.as_str(), found[0].year), ("Deep learning: Methods", Some(2015)));
        assert_eq!(found[0].to_string(), "Deep learning: Methods (LeCun, 2015)  10.1/a");
        let candidate = |title: &str, doi: Option<&str>| Candidate {
            title: title.into(),
            authors: vec![],
            year: None,
            venue: String::new(),
            ids: Ids { doi: doi.map(String::from), arxiv: None },
            in_library: None,
        };
        let merged = merge_results(
            "Attention is all you need",
            vec![
                candidate("Attention is not all you need", Some("10.1/a")),
                candidate("Attention Is All You Need", None),
            ],
            vec![
                candidate("Something else", Some("10.1/A")),
                candidate("ATTENTION IS ALL YOU NEED.", Some("10.1/b")),
                candidate("New", Some("10.1/c")),
            ],
        );
        let titles: Vec<&str> = merged.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Attention Is All You Need", "Attention is not all you need", "New"],
            "deduplicated, exact first"
        );

        let scholar = serde_json::json!({"title": "X", "authors": [{"name": "Ashish Vaswani"}], "externalIds": {"ArXiv": "1706.03762"}});
        assert_eq!(scholar_candidate(&scholar).map(|c| c.ids.source()), Some("arXiv:1706.03762".into()));
        assert!(scholar_candidate(&serde_json::json!({"title": "Y", "externalIds": {"CorpusId": 1}})).is_none());
        assert!(scholar_candidate(&serde_json::json!({"externalIds": {"DOI": "10.1/z"}})).is_none(), "no title");

        let feed = r#"<feed><title>query results</title>
          <entry><id>http://arxiv.org/abs/2006.11239v2</id><published>2020-06-19T17:24:44Z</published>
            <title>Denoising Diffusion
              Probabilistic Models</title><author><name>Jonathan Ho</name></author><author><name>Pieter Abbeel</name></author>
            <arxiv:doi xmlns:arxiv="http://arxiv.org/schemas/atom">10.1/published</arxiv:doi></entry>
          <entry><id>http://arxiv.org/abs/hep-th/9711200v3</id><title>The Large N Limit &amp; more</title></entry>
          <entry><id>http://arxiv.org/api/errors#bad</id><title>Error</title></entry></feed>"#;
        let found: Vec<Candidate> = feed.split("<entry>").skip(1).filter_map(arxiv_entry).collect();
        assert_eq!(found.len(), 2, "the error entry has no arXiv id");
        assert_eq!(found[0].title, "Denoising Diffusion Probabilistic Models", "line breaks in titles");
        assert_eq!(
            (found[0].authors.clone(), found[0].year),
            (vec!["Jonathan Ho".to_string(), "Pieter Abbeel".to_string()], Some(2020))
        );
        assert_eq!(found[0].ids, Ids { doi: Some("10.1/published".into()), arxiv: Some("2006.11239".into()) });
        assert_eq!(
            (found[1].title.as_str(), found[1].ids.arxiv.as_deref()),
            ("The Large N Limit & more", Some("hep-th/9711200"))
        );
    }
}
