//! Who is who. Names are compared by family name and initials, and two compatible names (`Head, J.` and
//! `James W. Head`) count as one person only with evidence: a co-author in common, or the same first name.
//! Without it they stay apart, so two different `Zhang, Y.` don't merge. The author lists of references
//! count too, which gives far more co-authors than the library alone.
//!
//! People can also be confirmed, by a person or by an AI's approved draft: `people/<id>.json` lists the
//! spellings that belong to one person and to nobody else. Those decisions come before any evidence.

use crate::Library;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

/// The folder of confirmed people, in the library.
pub const DIR: &str = "people";

/// A confirmed person: every spelling listed belongs to them alone. Kept in `people/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confirmed {
    #[serde(skip)]
    pub id: String,
    pub name: String,
    pub spellings: Vec<String>,
}

/// A name split into what the comparison needs, ASCII-folded and lowercased.
#[derive(Debug, Clone)]
struct Name {
    family: String,
    /// Given-name initials in order: `James W.` → `jw`.
    initials: String,
    /// The first given name when written out (`james`), else empty.
    first: String,
    /// The confirmed person this name belongs to.
    person: Option<String>,
}

impl Name {
    fn parse(name: &str) -> Option<Name> {
        let folded = deunicode::deunicode(name).to_lowercase();
        let (family, given) = split(&folded);
        let family: String = family.chars().filter(char::is_ascii_alphanumeric).collect();
        if family.is_empty() || family == "others" {
            return None;
        }
        const SUFFIXES: [&str; 6] = ["jr", "sr", "ii", "iii", "iv", "et"];
        let words: Vec<String> = given
            .split(|c: char| c.is_whitespace() || c == '.' || c == '-')
            .map(|w| w.chars().filter(char::is_ascii_alphabetic).collect::<String>())
            .filter(|w| !w.is_empty() && !SUFFIXES.contains(&w.as_str()))
            .collect();
        let initials = words.iter().filter_map(|w| w.chars().next()).collect();
        let first = words.first().filter(|w| w.len() > 1).cloned().unwrap_or_default();
        Some(Name { family, initials, first, person: None })
    }

    /// Family name and first initial: names that may be one person share it.
    fn key(&self) -> String {
        format!("{} {}", self.family, self.initials.get(..1).unwrap_or(""))
    }

    /// Whether the two could be one person: one's initials start the other's, first names don't differ.
    fn compatible(&self, other: &Name) -> bool {
        let initials = !self.initials.is_empty()
            && !other.initials.is_empty()
            && (self.initials.starts_with(&other.initials) || other.initials.starts_with(&self.initials));
        let confirmed = match (&self.person, &other.person) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        confirmed && initials && (self.first.is_empty() || other.first.is_empty() || self.first == other.first)
    }
}

/// `Family, Given` or `Given Family` as (family, given).
fn split(name: &str) -> (&str, &str) {
    match name.split_once(',') {
        Some((family, given)) => (family.trim(), given.trim()),
        None => match name.trim().rsplit_once(' ') {
            Some((given, family)) => (family, given.trim()),
            None => (name.trim(), ""),
        },
    }
}

/// One person, as far as the library, its references and the confirmed people tell.
#[derive(Debug, Serialize)]
pub struct Person {
    /// The id of a confirmed person (`people/<id>.json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The fullest spelling, as `Family, Given`.
    pub name: String,
    /// Every spelling seen.
    pub variants: Vec<String>,
    /// Library papers by them.
    pub papers: Vec<String>,
    /// The people they wrote with most, with how many papers (library papers and references) they share.
    pub coauthors: Vec<(String, usize)>,
    /// Others whose names fit but who share no co-author or first name: perhaps the same person. Each with
    /// their library papers, or `(cited only)`.
    pub maybe: Vec<String>,
}

/// The people behind the library's papers and references.
pub struct Authors {
    people: Vec<Person>,
    /// For each library paper, its authors' people (indexes into `people`) with the name as that paper spells it.
    by_paper: HashMap<String, Vec<(usize, String)>>,
}

impl Authors {
    /// `docs`: each paper's author list, with its library id (references have none).
    // ponytail: compares every pair of names with the same family name and first initial, fine for a
    // personal library; the first evidence decides, so `J.` joins whichever fitting person it meets first.
    pub fn new(docs: &[(Option<String>, Vec<String>)], confirmed: &[Confirmed]) -> Self {
        let owner: HashMap<&str, &str> =
            confirmed.iter().flat_map(|c| c.spellings.iter().map(|s| (s.trim(), c.id.as_str()))).collect();
        let mut names = vec![]; // (doc, raw name, parsed)
        for (doc, (_, authors)) in docs.iter().enumerate() {
            for raw in authors {
                if let Some(mut name) = Name::parse(raw) {
                    name.person = owner.get(raw.trim()).map(|id| id.to_string());
                    names.push((doc, raw.clone(), name));
                }
            }
        }
        let coauthors: Vec<Vec<String>> = docs
            .iter()
            .map(|(_, authors)| authors.iter().filter_map(|a| Name::parse(a)).map(|n| n.key()).collect())
            .collect();

        let mut parent: Vec<usize> = (0..names.len()).collect();
        // What each group of names knows about its person, so a merge can't join a James and a John
        // through a `J. W.` that fits both.
        let mut known: Vec<Name> = names.iter().map(|(_, _, name)| name.clone()).collect();
        // Confirmed spellings join their person whatever the evidence says.
        let mut first_of: HashMap<String, usize> = HashMap::new();
        for (i, (_, _, name)) in names.iter().enumerate() {
            if let Some(person) = &name.person {
                let first = *first_of.entry(person.clone()).or_insert(i);
                let (ra, rb) = (root(&mut parent, i), root(&mut parent, first));
                if ra != rb {
                    parent[ra] = rb;
                    if known[ra].initials.len() > known[rb].initials.len() {
                        known[rb].initials = known[ra].initials.clone();
                    }
                }
            }
        }
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (_, _, name)) in names.iter().enumerate() {
            by_key.entry(name.key()).or_default().push(i);
        }
        for group in by_key.values() {
            for (n, &a) in group.iter().enumerate() {
                for &b in &group[n + 1..] {
                    let ((doc_a, _, name_a), (doc_b, _, name_b)) = (&names[a], &names[b]);
                    let key = name_a.key();
                    let shared = || coauthors[*doc_a].iter().any(|c| *c != key && coauthors[*doc_b].contains(c));
                    let same_first = !name_a.first.is_empty() && name_a.first == name_b.first;
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                    if ra != rb
                        && name_a.compatible(name_b)
                        && known[ra].compatible(&known[rb])
                        && (same_first || shared())
                    {
                        parent[ra] = rb;
                        if known[ra].initials.len() > known[rb].initials.len() {
                            known[rb].initials = known[ra].initials.clone();
                        }
                        if known[rb].first.is_empty() {
                            known[rb].first = known[ra].first.clone();
                        }
                        if known[rb].person.is_none() {
                            known[rb].person = known[ra].person.clone();
                        }
                    }
                }
            }
        }

        // Number the people, then gather their spellings, papers and co-authors.
        let mut person_of_root: HashMap<usize, usize> = HashMap::new();
        let person: Vec<usize> = (0..names.len())
            .map(|i| {
                let r = root(&mut parent, i);
                let next = person_of_root.len();
                *person_of_root.entry(r).or_insert(next)
            })
            .collect();
        let mut people: Vec<Person> = (0..person_of_root.len())
            .map(|_| Person {
                id: None,
                name: String::new(),
                variants: vec![],
                papers: vec![],
                coauthors: vec![],
                maybe: vec![],
            })
            .collect();
        let mut what: Vec<Option<Name>> = vec![None; people.len()];
        for (&r, &person) in &person_of_root {
            what[person] = Some(known[r].clone());
        }
        let what: Vec<Name> = what.into_iter().flatten().collect();
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (person, name) in what.iter().enumerate() {
            by_key.entry(name.key()).or_default().push(person);
        }
        let mut in_doc: Vec<Vec<usize>> = vec![vec![]; docs.len()];
        let mut by_paper: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (i, (doc, raw, _)) in names.iter().enumerate() {
            let p = &mut people[person[i]];
            if !p.variants.contains(raw) {
                p.variants.push(raw.clone());
            }
            in_doc[*doc].push(person[i]);
            if let Some(id) = &docs[*doc].0 {
                p.papers.push(id.clone());
                by_paper.entry(id.clone()).or_default().push((person[i], raw.clone()));
            }
        }
        let mut together: HashMap<(usize, usize), usize> = HashMap::new();
        for doc in &in_doc {
            for &a in doc {
                for &b in doc.iter().filter(|&&b| b != a) {
                    *together.entry((a, b)).or_default() += 1;
                }
            }
        }
        let confirmed_name: HashMap<&str, &str> = confirmed.iter().map(|c| (c.id.as_str(), c.name.as_str())).collect();
        let names: Vec<String> = (0..people.len())
            .map(|i| match what[i].person.as_deref().and_then(|id| confirmed_name.get(id)) {
                Some(name) => name.to_string(),
                None => fullest(&people[i].variants),
            })
            .collect();
        let mut papers: Vec<Vec<String>> = people.iter().map(|p| p.papers.clone()).collect();
        papers.iter_mut().for_each(|p| p.dedup());
        let described = |j: usize| match papers[j].as_slice() {
            [] => format!("{} (cited only)", names[j]),
            ids => format!("{} ({})", names[j], ids.join(", ")),
        };
        for (i, p) in people.iter_mut().enumerate() {
            p.id = what[i].person.clone();
            p.name = names[i].clone();
            p.papers.sort();
            p.papers.dedup();
            let mut with: Vec<(String, usize)> =
                together.iter().filter(|((a, _), _)| *a == i).map(|((_, b), n)| (names[*b].clone(), *n)).collect();
            with.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            with.truncate(8);
            p.coauthors = with;
            let fits = by_key[&what[i].key()].iter().filter(|&&j| j != i && what[i].compatible(&what[j]));
            p.maybe = fits.map(|&j| described(j)).collect();
            p.maybe.sort();
            p.maybe.dedup();
        }
        Authors { people, by_paper }
    }

    /// The people with papers in the library, those with the most papers first, optionally only those
    /// with a spelling containing `name`.
    pub fn people(&self, name: Option<&str>) -> Vec<&Person> {
        let wanted = name.map(|n| deunicode::deunicode(n).to_lowercase());
        let matches = |p: &&Person| {
            wanted
                .as_ref()
                .is_none_or(|w| p.variants.iter().any(|v| deunicode::deunicode(v).to_lowercase().contains(w)))
        };
        let mut found: Vec<&Person> = self.people.iter().filter(|p| !p.papers.is_empty()).filter(matches).collect();
        found.sort_by(|a, b| b.papers.len().cmp(&a.papers.len()).then(a.name.cmp(&b.name)));
        found
    }

    /// For a library paper, the other library papers sharing authors with it, with those authors as it spells them.
    pub fn shared(&self, id: &str) -> Vec<(String, Vec<String>)> {
        let mut shared: HashMap<String, Vec<String>> = HashMap::new();
        for (person, spelled) in self.by_paper.get(id).into_iter().flatten() {
            for other in self.people[*person].papers.iter().filter(|&other| other != id) {
                let names = shared.entry(other.clone()).or_default();
                if !names.contains(spelled) {
                    names.push(spelled.clone());
                }
            }
        }
        let mut shared: Vec<_> = shared.into_iter().collect();
        shared.sort();
        shared
    }
}

/// An AI's proposal about people, waiting in `people/drafts/` for a human.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    #[serde(skip)]
    pub id: String,
    /// One person (`true`) or different people (`false`).
    pub same: bool,
    pub spellings: Vec<String>,
    pub reason: String,
}

impl Library {
    /// The confirmed people, from `people/<id>.json`.
    pub fn people(&self) -> Result<Vec<Confirmed>> {
        let mut found = vec![];
        for path in json_files(&self.root.join(DIR)) {
            let mut c: Confirmed = serde_json::from_str(&fs::read_to_string(&path)?)
                .with_context(|| format!("{} is invalid", path.display()))?;
            c.id = stem(&path);
            found.push(c);
        }
        Ok(found)
    }

    /// Confirm that these spellings are one person. People already confirmed with any of them are joined.
    /// Returns the person's id.
    pub fn same_person(&self, spellings: &[String]) -> Result<String> {
        let spellings: Vec<String> = spellings.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if spellings.len() < 2 {
            bail!("name at least two spellings");
        }
        let people = self.people()?;
        let (joined, others): (Vec<Confirmed>, Vec<Confirmed>) =
            people.into_iter().partition(|c| c.spellings.iter().any(|s| spellings.contains(s)));
        let mut person = match joined.first() {
            Some(first) => first.clone(),
            None => {
                let name = fullest(&spellings);
                Confirmed { id: unique_slug(&name, &others), name, spellings: vec![] }
            }
        };
        for s in joined.iter().flat_map(|c| &c.spellings).chain(&spellings) {
            if !person.spellings.contains(s) {
                person.spellings.push(s.clone());
            }
        }
        for gone in joined.iter().skip(1) {
            fs::remove_file(self.root.join(DIR).join(format!("{}.json", gone.id)))?;
        }
        self.save_person(&person)?;
        Ok(person.id)
    }

    /// Confirm that these spellings are different people: each ends up with a different confirmed person.
    pub fn different_people(&self, spellings: &[String]) -> Result<Vec<String>> {
        let spellings: Vec<String> = spellings.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if spellings.len() < 2 {
            bail!("name at least two spellings");
        }
        let mut ids = vec![];
        for (n, spelling) in spellings.iter().enumerate() {
            let people = self.people()?;
            let owner = people.iter().find(|c| c.spellings.contains(spelling));
            let shared = owner.is_some_and(|c| spellings.iter().take(n).any(|earlier| c.spellings.contains(earlier)));
            match owner {
                Some(c) if !shared => ids.push(c.id.clone()),
                _ => {
                    if let Some(c) = owner {
                        let mut rest = c.clone();
                        rest.spellings.retain(|s| s != spelling);
                        self.save_person(&rest)?;
                    }
                    let person = Confirmed {
                        id: unique_slug(spelling, &people),
                        name: spelling.clone(),
                        spellings: vec![spelling.clone()],
                    };
                    self.save_person(&person)?;
                    ids.push(person.id);
                }
            }
        }
        Ok(ids)
    }

    fn save_person(&self, person: &Confirmed) -> Result<()> {
        let dir = self.root.join(DIR);
        fs::create_dir_all(&dir)?;
        Ok(fs::write(dir.join(format!("{}.json", person.id)), serde_json::to_string_pretty(person)? + "\n")?)
    }

    /// Store an AI's proposal about people as a draft.
    pub fn propose_people(&self, same: bool, spellings: &[String], reason: &str) -> Result<String> {
        if spellings.len() < 2 {
            bail!("name at least two spellings");
        }
        let dir = self.root.join(DIR).join("drafts");
        fs::create_dir_all(&dir)?;
        let taken: Vec<String> = json_files(&dir).iter().map(|p| stem(p)).collect();
        let id = (1..).map(|n| n.to_string()).find(|n| !taken.contains(n)).unwrap_or_default();
        let proposal =
            Proposal { id: id.clone(), same, spellings: spellings.to_vec(), reason: reason.trim().to_string() };
        fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(&proposal)? + "\n")?;
        Ok(id)
    }

    /// The proposals about people waiting for review.
    pub fn people_drafts(&self) -> Vec<Proposal> {
        let files = json_files(&self.root.join(DIR).join("drafts"));
        let read = |path: &std::path::PathBuf| -> Option<Proposal> {
            let mut p: Proposal = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
            p.id = stem(path);
            Some(p)
        };
        files.iter().filter_map(read).collect()
    }

    /// Accept a proposal about people, or with `accept` false discard it.
    pub fn decide_people(&self, draft: &str, accept: bool) -> Result<()> {
        let proposal = self
            .people_drafts()
            .into_iter()
            .find(|p| p.id == draft)
            .with_context(|| format!("no people draft {draft}"))?;
        if accept {
            match proposal.same {
                true => _ = self.same_person(&proposal.spellings)?,
                false => _ = self.different_people(&proposal.spellings)?,
            }
        }
        let dir = self.root.join(DIR).join("drafts");
        fs::remove_file(dir.join(format!("{draft}.json")))?;
        let _ = fs::remove_dir(dir);
        Ok(())
    }
}

fn json_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.retain(|p| p.extension().is_some_and(|e| e == "json"));
    files.sort();
    files
}

fn stem(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The fullest of some spellings, as `Family, Given`.
fn fullest(spellings: &[String]) -> String {
    let best = spellings
        .iter()
        .max_by_key(|v| (split(v).1.chars().filter(char::is_ascii_alphabetic).count(), v.contains(',')));
    best.map(|v| match split(v) {
        (family, "") => family.to_string(),
        (family, given) => format!("{family}, {given}"),
    })
    .unwrap_or_default()
}

/// A file name for a person: `clark-roger-n`, made unique among `taken`.
fn unique_slug(name: &str, taken: &[Confirmed]) -> String {
    let words = deunicode::deunicode(name).to_lowercase();
    let base: Vec<&str> = words.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let base = base.join("-");
    let base = if base.is_empty() { "person".to_string() } else { base };
    std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|id| !taken.iter().any(|c| &c.id == id))
        .unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: Option<&str>, names: &[&str]) -> (Option<String>, Vec<String>) {
        (id.map(String::from), names.iter().map(|n| n.to_string()).collect())
    }

    #[test]
    fn names() {
        let n = Name::parse("Head, J. W., III").unwrap();
        assert_eq!((n.family.as_str(), n.initials.as_str(), n.first.as_str()), ("head", "jw", ""));
        let n = Name::parse("Jean-Philippe Combe").unwrap();
        assert_eq!((n.family.as_str(), n.initials.as_str(), n.first.as_str()), ("combe", "jp", "jean"));
        assert_eq!(Name::parse("Müller, Jörg").unwrap().key(), "muller j");
        assert!(Name::parse("others").is_none());
        let [a, b, c, d] = ["Head, J.", "James W. Head", "Head, J. K.", "Head, John"].map(|n| Name::parse(n).unwrap());
        assert!(a.compatible(&b) && a.compatible(&c) && !b.compatible(&c), "J. fits both, J. W. and J. K. don't");
        assert!(!b.compatible(&d), "James is not John");
    }

    #[test]
    fn co_authors_decide() {
        let authors = Authors::new(
            &[
                doc(Some("a"), &["Head, James W.", "Fassett, Caleb I."]),
                doc(Some("b"), &["J. W. Head", "Wilson, L."]),
                doc(None, &["Head, J. W.", "Wilson, Lionel", "Fassett, C."]), // a reference: it links a and b
                doc(Some("c"), &["Zhang, Y.", "Smith, A."]),
                doc(Some("d"), &["Zhang, Y.", "Jones, B."]),
                doc(Some("e"), &["Head, John", "Fassett, C. I."]),
                doc(Some("f"), &["Head, J. K.", "Wilson, L."]),
            ],
            &[],
        );
        let heads = authors.people(Some("head"));
        let papers: Vec<&[String]> = heads.iter().map(|p| p.papers.as_slice()).collect();
        assert_eq!(papers.len(), 3, "{heads:#?}");
        assert!(papers.contains(&&["a".to_string(), "b".into()][..]), "through their co-authors: {papers:?}");
        assert_eq!(heads[0].name, "Head, James W.", "the fullest spelling");
        assert!(heads[0].coauthors.iter().any(|(name, n)| name == "Wilson, Lionel" && *n == 2));
        let zhangs = authors.people(Some("zhang"));
        assert_eq!(zhangs.len(), 2, "no co-author in common: two people");
        assert_eq!(zhangs[0].maybe, ["Zhang, Y. (d)"], "but perhaps one");
        assert!(heads[0].maybe.iter().all(|m| !m.starts_with("Head, John")), "James is not John: {:?}", heads[0].maybe);
        let shared =
            [("b".to_string(), vec!["Head, James W.".to_string()]), ("e".into(), vec!["Fassett, Caleb I.".into()])];
        assert_eq!(authors.shared("a"), shared, "e's Fassett, C. I. also wrote with a Head");
        assert!(authors.shared("c").is_empty());
    }

    #[test]
    fn confirmed_people_come_first() {
        let docs = [
            doc(Some("x"), &["Clark, Roger N."]),
            doc(Some("y"), &["Clark, R. N.", "Pieters, C. M."]),
            doc(Some("u"), &["Head, J.", "Wilson, L."]),
            doc(Some("v"), &["Head, James W.", "Wilson, L."]),
        ];
        let confirmed = |id: &str, spellings: &[&str]| Confirmed {
            id: id.into(),
            name: spellings[0].into(),
            spellings: spellings.iter().map(|s| s.to_string()).collect(),
        };
        let guessed = Authors::new(&docs, &[]);
        assert_eq!(guessed.people(Some("clark")).len(), 2, "no evidence: apart");
        assert_eq!(guessed.people(Some("head")).len(), 1, "a co-author in common: together");

        let decided = Authors::new(
            &docs,
            &[
                confirmed("clark", &["Clark, Roger N.", "Clark, R. N."]),
                confirmed("head-j", &["Head, J."]),
                confirmed("head-james", &["Head, James W."]),
            ],
        );
        let clark = decided.people(Some("clark"));
        assert_eq!((clark.len(), clark[0].id.as_deref()), (1, Some("clark")), "confirmed: together");
        assert_eq!(
            (clark[0].name.as_str(), clark[0].papers.as_slice()),
            ("Clark, Roger N.", &["x".to_string(), "y".into()][..])
        );
        let heads = decided.people(Some("head"));
        assert_eq!(heads.len(), 2, "confirmed different, whatever the co-authors say");
        assert!(heads.iter().all(|h| h.maybe.is_empty()), "and no longer a maybe: {heads:?}");
    }
}
