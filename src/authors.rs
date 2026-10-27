//! Who is who. Names are compared by family name and initials, and two compatible names (`Head, J.` and
//! `James W. Head`) count as one person only with evidence: a co-author in common, or the same first name.
//! Without it they stay apart, so two different `Zhang, Y.` don't merge. The author lists of references
//! count too, which gives far more co-authors than the library alone.

use serde::Serialize;
use std::collections::HashMap;

/// A name split into what the comparison needs, ASCII-folded and lowercased.
#[derive(Debug, Clone)]
struct Name {
    family: String,
    /// Given-name initials in order: `James W.` → `jw`.
    initials: String,
    /// The first given name when written out (`james`), else empty.
    first: String,
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
        Some(Name { family, initials, first })
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
        initials && (self.first.is_empty() || other.first.is_empty() || self.first == other.first)
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

/// One person, as far as the library and its references tell.
#[derive(Debug, Serialize)]
pub struct Person {
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
    pub fn new(docs: &[(Option<String>, Vec<String>)]) -> Self {
        let mut names = vec![]; // (doc, raw name, parsed)
        for (doc, (_, authors)) in docs.iter().enumerate() {
            for raw in authors {
                if let Some(name) = Name::parse(raw) {
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
            .map(|_| Person { name: String::new(), variants: vec![], papers: vec![], coauthors: vec![], maybe: vec![] })
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
        let fullest = |variants: &[String]| {
            let best = variants
                .iter()
                .max_by_key(|v| (split(v).1.chars().filter(char::is_ascii_alphabetic).count(), v.contains(',')));
            best.map(|v| match split(v) {
                (family, "") => family.to_string(),
                (family, given) => format!("{family}, {given}"),
            })
        };
        let names: Vec<String> = people.iter().map(|p| fullest(&p.variants).unwrap_or_default()).collect();
        let mut papers: Vec<Vec<String>> = people.iter().map(|p| p.papers.clone()).collect();
        papers.iter_mut().for_each(|p| p.dedup());
        let described = |j: usize| match papers[j].as_slice() {
            [] => format!("{} (cited only)", names[j]),
            ids => format!("{} ({})", names[j], ids.join(", ")),
        };
        for (i, p) in people.iter_mut().enumerate() {
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
        let authors = Authors::new(&[
            doc(Some("a"), &["Head, James W.", "Fassett, Caleb I."]),
            doc(Some("b"), &["J. W. Head", "Wilson, L."]),
            doc(None, &["Head, J. W.", "Wilson, Lionel", "Fassett, C."]), // a reference: it links a and b
            doc(Some("c"), &["Zhang, Y.", "Smith, A."]),
            doc(Some("d"), &["Zhang, Y.", "Jones, B."]),
            doc(Some("e"), &["Head, John", "Fassett, C. I."]),
            doc(Some("f"), &["Head, J. K.", "Wilson, L."]),
        ]);
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
}
