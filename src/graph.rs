//! The citation graph. Each paper keeps its reference list in `<key>/references.json`, looked up from the
//! [`crate::refs`] sources or read from the full text by an AI (as a draft a human approves). Every
//! reference says where it came from. The links between papers are worked out from those files.

use crate::authors::{Authors, Confirmed};
use crate::fetch::{self, Candidate, Ids};
use crate::refs::{Lookup, ReferenceSource};
use crate::{Library, Paper};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;

pub const FILE: &str = "references.json";
/// The source of references an AI read from the full text and a human approved.
pub const AI: &str = "ai";

/// A paper another paper cites. It need not be in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub title: String,
    /// Full names as the source writes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub venue: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arxiv: Option<String>,
    /// Where it came from: a source's name (`openalex`, `semantic-scholar`, `ads`) or `ai`.
    #[serde(default)]
    pub source: String,
}

impl Reference {
    fn new(c: Candidate, source: &str) -> Self {
        let Candidate { title, authors, year, venue, ids, .. } = c;
        Reference { title, authors, year, venue, doi: ids.doi, arxiv: ids.arxiv, source: source.into() }
    }

    fn ids(&self) -> Ids {
        Ids { doi: self.doi.clone(), arxiv: self.arxiv.clone() }
    }

    fn keys(&self) -> Vec<String> {
        fetch::paper_keys(&self.ids(), &self.title)
    }

    /// As a search result, to show it or add it to the library.
    pub fn candidate(&self, in_library: Option<String>) -> Candidate {
        let Reference { title, authors, year, venue, .. } = self.clone();
        Candidate { title, authors, year, venue, ids: self.ids(), in_library }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}  [{}]", self.candidate(None), self.source)
    }
}

/// A paper's references from every source that has them. Each adds what the earlier ones lack; where two
/// list the same paper, the earlier entry stays. Fallback sources are asked only when the rest found nothing.
pub fn look_up(sources: &[Box<dyn ReferenceSource>], ids: &Ids, title: &str) -> Result<Vec<Reference>> {
    let (mut found, mut failed) = (vec![], vec![]);
    for source in sources.iter().filter(|s| !s.fallback()).chain(sources.iter().filter(|s| s.fallback())) {
        if source.fallback() && !found.is_empty() {
            break;
        }
        match source.references(&Lookup { ids, title }) {
            Ok(refs) if !refs.is_empty() => {
                found = merge(found, refs.into_iter().map(|c| Reference::new(c, source.name())));
            }
            Ok(_) => failed.push(format!("{} has none", source.name())),
            Err(e) => failed.push(format!("{}: {e:#}", source.name())),
        }
    }
    if found.is_empty() {
        bail!("no source has its references ({}); an AI can read them from the full text", failed.join("; "));
    }
    Ok(found)
}

/// `first`, then those of `more` that are not in it already.
pub fn merge(mut first: Vec<Reference>, more: impl IntoIterator<Item = Reference>) -> Vec<Reference> {
    let mut seen: Vec<String> = first.iter().flat_map(Reference::keys).collect();
    for r in more {
        let keys = r.keys();
        if !keys.iter().any(|k| seen.contains(k)) {
            seen.extend(keys);
            first.push(r);
        }
    }
    first
}

/// The references in a paper's folder; `None` before they were ever looked up.
pub fn read(dir: &Path) -> Result<Option<Vec<Reference>>> {
    match fs::read_to_string(dir.join(FILE)) {
        Ok(text) => Ok(Some(serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{FILE} is invalid: {e}"))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn write(dir: &Path, refs: &[Reference]) -> Result<()> {
    Ok(fs::write(dir.join(FILE), serde_json::to_string_pretty(refs)? + "\n")?)
}

/// An AI's reference list (a JSON array) as a draft: checked, and marked as coming from an AI.
pub fn ai_draft(content: &str) -> Result<String> {
    let mut refs: Vec<Reference> = serde_json::from_str(content).map_err(|e| {
        anyhow::anyhow!("references must be a JSON array of {{title, authors, year, venue, doi, arxiv}}: {e}")
    })?;
    refs.retain(|r| !r.title.trim().is_empty());
    if refs.is_empty() {
        bail!("no reference with a title");
    }
    refs.iter_mut().for_each(|r| r.source = AI.into());
    Ok(serde_json::to_string_pretty(&refs)?)
}

/// How one paper connects to the rest of the library.
#[derive(Debug, Serialize)]
pub struct Neighbors {
    /// Library papers it cites.
    pub cites: Vec<String>,
    /// Library papers citing it.
    pub cited_by: Vec<String>,
    /// Library papers sharing authors with it, with those authors.
    pub shared_authors: BTreeMap<String, Vec<String>>,
    /// Whether its references were looked up; without them, `cites` and `not_in_library` are empty.
    pub references_known: bool,
    /// What it cites that is not in the library.
    pub not_in_library: Vec<Reference>,
}

/// A paper missing from the library, with the library papers citing it.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    #[serde(flatten)]
    pub reference: Reference,
    pub cited_by: Vec<String>,
}

// ponytail: rebuilt from the files on every call, fine for thousands of papers; move into the index if it gets slow.
pub struct Graph {
    papers: Vec<Paper>,
    /// Who is who, from the library's papers and their references.
    pub authors: Authors,
    refs: BTreeMap<String, Vec<Reference>>,
    /// Paper keys (see [`fetch::paper_keys`]) to library ids.
    known: HashMap<String, String>,
    /// Library papers each paper cites, and the reverse.
    cites: BTreeMap<String, Vec<String>>,
    cited_by: BTreeMap<String, Vec<String>>,
}

impl Graph {
    fn new(papers: Vec<Paper>, refs: BTreeMap<String, Vec<Reference>>, people: &[Confirmed]) -> Self {
        let mut known = HashMap::new();
        for p in &papers {
            for key in fetch::paper_keys(&Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() }, &p.bib.title()) {
                known.entry(key).or_insert_with(|| p.id.clone());
            }
        }
        let find = |r: &Reference| r.keys().iter().find_map(|k| known.get(k)).cloned();
        let (mut cites, mut cited_by) = (BTreeMap::new(), BTreeMap::<String, Vec<String>>::new());
        for (id, list) in &refs {
            let mut found: Vec<String> = list.iter().filter_map(find).filter(|other| other != id).collect();
            found.sort();
            found.dedup();
            for other in &found {
                cited_by.entry(other.clone()).or_default().push(id.clone());
            }
            cites.insert(id.clone(), found);
        }
        // Each paper's author list once: the library's, then the cited papers not in it.
        let mut docs: Vec<(Option<String>, Vec<String>)> =
            papers.iter().map(|p| (Some(p.id.clone()), p.bib.authors())).collect();
        let mut seen: HashSet<String> = HashSet::new();
        for r in refs.values().flatten() {
            let keys = r.keys();
            let new = !keys.iter().any(|k| known.contains_key(k) || seen.contains(k));
            seen.extend(keys);
            if new && r.authors.len() > 1 {
                docs.push((None, r.authors.clone()));
            }
        }
        Graph { authors: Authors::new(&docs, people), papers, refs, known, cites, cited_by }
    }

    /// The library id of a referenced paper.
    pub fn find(&self, r: &Reference) -> Option<&str> {
        r.keys().iter().find_map(|k| self.known.get(k)).map(String::as_str)
    }

    /// Library papers that `id` cites, or with `cited_by` that cite it.
    fn linked(&self, id: &str, cited_by: bool) -> &[String] {
        let edges = if cited_by { &self.cited_by } else { &self.cites };
        edges.get(id).map_or(&[], Vec::as_slice)
    }

    /// The library papers `id` cites (or that cite it), and theirs in turn, `depth` levels deep: each as
    /// (level, id, already listed). A paper met again is listed but not followed, so cycles end.
    pub fn tree(&self, id: &str, depth: usize, cited_by: bool) -> Result<Vec<(usize, String, bool)>> {
        if !self.papers.iter().any(|p| p.id == id) {
            bail!("no paper {id}");
        }
        fn walk(
            g: &Graph,
            id: &str,
            level: usize,
            depth: usize,
            cited_by: bool,
            seen: &mut HashSet<String>,
            out: &mut Vec<(usize, String, bool)>,
        ) {
            for next in g.linked(id, cited_by) {
                let again = !seen.insert(next.clone());
                out.push((level, next.clone(), again));
                if !again && level < depth {
                    walk(g, next, level + 1, depth, cited_by, seen, out);
                }
            }
        }
        let mut out = vec![];
        walk(self, id, 1, depth, cited_by, &mut HashSet::from([id.to_string()]), &mut out);
        Ok(out)
    }

    /// Every citation between library papers, as (citing, cited).
    pub fn citations(&self) -> Vec<(&str, &str)> {
        self.cites.iter().flat_map(|(a, cited)| cited.iter().map(move |b| (a.as_str(), b.as_str()))).collect()
    }

    /// A library paper's title.
    pub fn title(&self, id: &str) -> String {
        self.papers.iter().find(|p| p.id == id).map(|p| p.bib.title()).unwrap_or_default()
    }

    pub fn neighbors(&self, id: &str) -> Result<Neighbors> {
        if !self.papers.iter().any(|p| p.id == id) {
            bail!("no paper {id}");
        }
        let shared_authors = self.authors.shared(id).into_iter().collect();
        let refs = self.refs.get(id);
        Ok(Neighbors {
            cites: self.linked(id, false).to_vec(),
            cited_by: self.linked(id, true).to_vec(),
            shared_authors,
            references_known: refs.is_some(),
            not_in_library: refs.into_iter().flatten().filter(|r| self.find(r).is_none()).cloned().collect(),
        })
    }

    /// Papers missing from the library, most cited by the library papers in `within` (all if `None`) first.
    pub fn suggestions(&self, within: Option<&[String]>) -> Vec<Suggestion> {
        let mut found: Vec<Suggestion> = vec![];
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let citing = self.refs.iter().filter(|(id, _)| within.is_none_or(|ids| ids.contains(id)));
        for (id, refs) in citing {
            for r in refs.iter().filter(|r| self.find(r).is_none()) {
                let keys = r.keys();
                match keys.iter().find_map(|k| by_key.get(k)).copied() {
                    Some(i) if !found[i].cited_by.contains(id) => found[i].cited_by.push(id.clone()),
                    Some(_) => {}
                    None => {
                        keys.into_iter().for_each(|k| _ = by_key.insert(k, found.len()));
                        found.push(Suggestion { reference: r.clone(), cited_by: vec![id.clone()] });
                    }
                }
            }
        }
        found.sort_by(|a, b| b.cited_by.len().cmp(&a.cited_by.len()).then(b.reference.year.cmp(&a.reference.year)));
        found
    }

    /// The citations between the papers in `within` (all if `None`) for Graphviz.
    pub fn dot(&self, within: Option<&[String]>) -> String {
        let shown = |id: &String| within.is_none_or(|ids| ids.contains(id));
        let mut out = String::from("digraph carrel {\n  rankdir=LR;\n  node [shape=box];\n");
        for p in self.papers.iter().filter(|p| shown(&p.id)) {
            out += &format!("  \"{}\";\n", p.id);
            for cited in self.linked(&p.id, false).iter().filter(|&c| shown(c)) {
                out += &format!("  \"{}\" -> \"{cited}\";\n", p.id);
            }
        }
        out + "}\n"
    }
}

impl Library {
    /// The citation graph and who is who, from the papers, their references (cached in the index) and the
    /// decisions about people.
    pub fn graph(&self) -> Result<Graph> {
        let papers = self.papers().0;
        let refs = crate::index::references(&self.index, &papers)?;
        Ok(Graph::new(papers, refs, &self.people()?))
    }

    /// A paper's references, looked up the first time (or again with `refresh`), each with its library id.
    pub fn references(&self, id: &str, refresh: bool) -> Result<Vec<(Reference, Option<String>)>> {
        let p = self.get(id)?;
        if refresh || read(&p.dir)?.is_none() {
            self.look_up_references(&p)?;
        }
        let graph = self.graph()?;
        let refs = read(&p.dir)?.unwrap_or_default();
        let with_ids = refs.into_iter().map(|r| {
            let found = graph.find(&r).filter(|&found| found != id).map(String::from);
            (r, found)
        });
        Ok(with_ids.collect())
    }

    /// Look up a paper's references and save them. References an AI read stay unless a source has them too.
    /// A paper without a DOI or arXiv id gets them from ADS first, when it knows the title; the note says so.
    pub fn look_up_references(&self, p: &Paper) -> Result<(usize, Option<String>)> {
        let note = self.fill_ids(p).unwrap_or_else(|e| Some(format!("ADS: {e:#}")));
        let p = &self.get(&p.id)?;
        let found = look_up(&crate::refs::sources()?, &Ids { doi: p.bib.doi(), arxiv: p.bib.arxiv() }, &p.bib.title());
        let found = found.map_err(|e| match &note {
            Some(note) => e.context(note.clone()),
            None => e,
        })?;
        let n = found.len();
        let from_ai = read(&p.dir)?.unwrap_or_default().into_iter().filter(|r| r.source == AI);
        write(&p.dir, &merge(found, from_ai))?;
        if crate::affiliations::read(&p.dir).is_none() {
            // Best effort: the map's institution links, not worth failing the references for.
            let _ = self.look_up_affiliations(p);
        }
        Ok((n, note))
    }

    /// Adds the DOI and arXiv id ADS has for a paper lacking both.
    fn fill_ids(&self, p: &Paper) -> Result<Option<String>> {
        let (None, None, Some(ads)) = (p.bib.doi(), p.bib.arxiv(), crate::refs::ads()?) else { return Ok(None) };
        let ids = ads.ids(&p.bib.title(), p.bib.year(), p.bib.authors().first().map(String::as_str))?;
        if ids.is_empty() {
            return Ok(None);
        }
        let mut p = p.clone();
        p.bib.set("doi", ids.doi.as_deref());
        if ids.arxiv.is_some() {
            p.bib.set("eprint", ids.arxiv.as_deref());
            p.bib.set("archiveprefix", Some("arXiv"));
        }
        p.save()?;
        let found = [ids.doi.map(|d| format!("DOI {d}")), ids.arxiv.map(|a| format!("arXiv {a}"))];
        Ok(Some(format!("found {} via ADS", found.into_iter().flatten().collect::<Vec<_>>().join(" and "))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(&'static str, Result<Vec<&'static str>, &'static str>, bool);

    impl ReferenceSource for Fake {
        fn name(&self) -> &'static str {
            self.0
        }

        fn references(&self, _: &Lookup) -> Result<Vec<Candidate>> {
            let titles = self.1.clone().map_err(|e| anyhow::anyhow!(e))?;
            Ok(titles.into_iter().map(|t| Reference { title: t.into(), ..reference("") }.candidate(None)).collect())
        }

        fn fallback(&self) -> bool {
            self.2
        }
    }

    fn reference(title: &str) -> Reference {
        Reference {
            title: title.into(),
            authors: vec![],
            year: None,
            venue: String::new(),
            doi: None,
            arxiv: None,
            source: String::new(),
        }
    }

    #[test]
    fn sources_add_up_and_fallbacks_wait() {
        let ids = Ids::default();
        let sources: Vec<Box<dyn ReferenceSource>> = vec![
            Box::new(Fake("spare", Ok(vec!["Z"]), true)),
            Box::new(Fake("down", Err("offline"), false)),
            Box::new(Fake("first", Ok(vec!["A", "B"]), false)),
            Box::new(Fake("second", Ok(vec!["b", "C"]), false)),
        ];
        let found = look_up(&sources, &ids, "").unwrap();
        let got: Vec<(&str, &str)> = found.iter().map(|r| (r.title.as_str(), r.source.as_str())).collect();
        assert_eq!(
            got,
            [("A", "first"), ("B", "first"), ("C", "second")],
            "B once, from the earlier source; no fallback"
        );
        let only_fallback = look_up(&sources[..2], &ids, "").unwrap();
        assert_eq!(only_fallback[0].source, "spare", "asked when the others found nothing");
        let err = look_up(&sources[1..2], &ids, "").unwrap_err().to_string();
        assert!(err.contains("down: offline") && err.contains("an AI can read"), "{err}");
    }

    #[test]
    fn merging_keeps_one_of_each() {
        let doi = |title: &str, doi: &str| Reference { doi: Some(doi.into()), ..reference(title) };
        let merged = merge(vec![doi("A", "10.1/A")], [doi("A again", "10.1/a"), reference("B"), reference("b!")]);
        let titles: Vec<&str> = merged.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"], "same DOI in another case, same title in other punctuation");
    }

    #[test]
    fn ai_drafts() {
        let draft = ai_draft(r#"[{"title": "A", "source": "openalex"}, {"title": " "}]"#).unwrap();
        let refs: Vec<Reference> = serde_json::from_str(&draft).unwrap();
        assert_eq!((refs.len(), refs[0].source.as_str()), (1, AI), "an AI cannot claim to be a source");
        assert!(ai_draft("Smith 2020").unwrap_err().to_string().contains("JSON array"));
        assert!(ai_draft("[]").is_err());
    }
}
