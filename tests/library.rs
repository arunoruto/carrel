//! The library's behavior at its edges, through its public API. Offline.

mod common;

use carrel::{Field, chunk};
use common::*;
use std::fs;

#[test]
fn adding_pdfs() {
    let tmp = Tmp::new("add");
    let lib = library(&tmp);
    let pdf = tmp.write("in/graph-theory notes.pdf", FAKE_PDF);
    let (id, messages) = lib.add(&format!("'{}'", pdf.display())).unwrap(); // quoted, as terminals paste
    assert_eq!(id, "graphtheory", "no metadata: the key comes from the file name");
    assert!(messages.iter().any(|m| m.contains("no DOI or arXiv id")), "{messages:?}");
    let dir = lib.root.join(&id);
    for file in ["graphtheory.bib", "graphtheory.pdf", "fulltext.md", "figures/page-0001-00.png"] {
        assert!(dir.join(file).exists(), "{file} missing");
    }
    assert_eq!(lib.get(&id).unwrap().bib.title(), "graph-theory notes");

    let again = lib.add(&pdf.to_string_lossy()).unwrap_err().to_string();
    assert!(again.contains("already in the library as graphtheory"), "{again}");

    let text = tmp.write("in/notes.pdf", "just text");
    assert!(lib.add(&text.to_string_lossy()).unwrap_err().to_string().contains("is not a PDF"));
    assert!(lib.add(&tmp.path("in/missing.pdf").to_string_lossy()).is_err());

    // A failing extractor doesn't lose the paper; the doctor notices the missing text.
    let mut failing = library(&tmp);
    failing.extractor = "echo 'it broke' >&2; exit 3".into();
    let other = tmp.write("in/other.pdf", FAKE_PDF);
    let (id, messages) = failing.add(&other.to_string_lossy()).unwrap();
    assert!(messages.iter().any(|m| m.contains("it broke")), "{messages:?}");
    assert!(lib.doctor().iter().any(|i| i.check == "text" && i.id == id));

    assert_eq!(lib.papers().0.len(), 2);
    assert!(fs::read_dir(lib.root.join(".tmp")).unwrap().next().is_none(), "staging folders are cleaned up");
}

#[test]
fn citation_keys() {
    let tmp = Tmp::new("keys");
    let mut lib = library(&tmp);
    paper(&lib, "a", "title = {Über die Größe}, author = {Müller, Jörg}, year = 2021");
    paper(&lib, "b", "title = {深度学习研究}, author = {张, 伟}, year = 2020");
    paper(&lib, "c", "title = {On a Theory}, author = {van der Berg, Jan and O'Brien, Pat}, year = 2019");
    paper(&lib, "d", "title = {{}}");
    assert_eq!(lib.rename("a", None).unwrap(), "muller2021uber");
    assert_eq!(lib.rename("b", None).unwrap(), "zhang2020shen");
    assert_eq!(lib.rename("c", None).unwrap(), "vanderberg2019theory");
    assert_eq!(lib.rename("d", None).unwrap(), "paper", "nothing to build a key from");
    assert_eq!(lib.rename("muller2021uber", None).unwrap(), "muller2021uber", "already right: nothing happens");
    assert!(lib.get("muller2021uber").is_ok());

    for (template, key) in [
        ("{{ author }}:{{ year }}", "Muller:2021"),
        ("-{{ year }}", "2021"),
        ("{{ authors | join('-') | upper }}_{{ words[:2] | join | lower }}", "MULLER_ubergrosse"),
        ("projects", "projectsb"),
    ] {
        lib.key = template.into();
        let current = lib.papers().0.into_iter().find(|p| p.bib.title().starts_with("Über")).unwrap().id;
        assert_eq!(lib.rename(&current, None).unwrap(), key, "template {template}");
    }
    assert!(lib.rename("paper", Some("-flag")).is_err());
    assert!(lib.rename("paper", Some("../escape")).is_err());
    assert!(lib.rename("paper", Some("zhang2020shen")).is_err(), "taken");
    assert!(lib.rename("paper", Some("projects")).is_err(), "reserved for project files");
}

#[test]
fn importing_bib_files() {
    let tmp = Tmp::new("import");
    let lib = library(&tmp);
    paper(&lib, "known", "title = {Known Paper}, doi = {10.1000/KNOWN}");
    let bib = tmp.write(
        "refs.bib",
        r#"Exported by someone@example.org
@string{nat = "Nature"}
@comment{a comment with {braces}}
@article{smith:2020, title = {First}, author = {Smith, A.}, journal = nat, year = 2020}
@Article(paren2021, Title = "Second, with a comma", Year = 2021)
@misc{DBLP:journals/corr/abs-1, title = {Third}, author = {Doe, J.}, year = 2019}
@misc{projects, title = {Fourth}, author = {Roe, R.}, year = 2018}
@misc{smith:2020, title = {A different paper with a taken key}, author = {Poe, E.}, year = 2017}
@misc{again, title = {Known Paper}, doi = {10.1000/known}}
@misc{knowntwice, title = {First}}
@misc{broken, title = {never closed
"#,
    );
    let (ids, messages) = lib.import_bib(&bib, false).unwrap();
    assert_eq!(
        ids,
        ["smith:2020", "paren2021", "doe2019third", "roe2018fourth", "poe2017different", "known", "smith:2020"]
    );
    for expected in [
        "imported as doe2019third",
        "imported as roe2018fourth",
        "imported as poe2017different",
        "already in the library as known",
        "skipped an entry",
    ] {
        assert!(messages.iter().any(|m| m.contains(expected)), "{expected:?} not in {messages:?}");
    }
    let smith = lib.get("smith:2020").unwrap();
    assert_eq!(smith.bib.get("journal").as_deref(), Some("Nature"), "@string macros are expanded");
    assert!(smith.bibtex().contains("journal = \"Nature\""), "{}", smith.bibtex());

    lib.add_to_project("thesis", &ids).unwrap();
    assert_eq!(lib.project("thesis").unwrap().len(), 6, "listed once each");
    let joined: String = lib.project_papers("thesis").unwrap().0.iter().map(|p| p.bibtex()).collect();
    let parsed = carrel::bib::Entry::parse_all(&joined);
    assert_eq!(parsed.len(), 6);
    assert!(parsed.iter().all(|e| e.is_ok()), "the joined bibliography parses");
    assert!(lib.import_bib(&tmp.path("nope.bib"), false).is_err());
}

#[test]
fn projects() {
    let tmp = Tmp::new("projects");
    let lib = library(&tmp);
    paper(&lib, "a", "title = {A}");
    paper(&lib, "b", "title = {B}");
    tmp.write("lib/projects/thesis.txt", "# my thesis\r\na  # first chapter\r\n\r\nmissing\r\n");
    assert_eq!(lib.project("thesis").unwrap(), ["a", "missing"], "CRLF, comments, blank lines");
    assert_eq!(lib.project_papers("thesis").unwrap().1, ["missing"]);

    let before = read(tmp.path("lib/projects/thesis.txt"));
    assert!(lib.add_to_project("thesis", &["b".into(), "nope".into()]).is_err());
    assert_eq!(read(tmp.path("lib/projects/thesis.txt")), before, "a failed add changes nothing");

    lib.add_to_project("thesis", &["b".into(), "b".into(), "a".into()]).unwrap();
    assert_eq!(lib.project("thesis").unwrap(), ["a", "missing", "b"]);
    lib.rename("a", Some("alpha")).unwrap();
    assert!(
        read(tmp.path("lib/projects/thesis.txt")).contains("alpha  # first chapter"),
        "the comment survives a rename"
    );
    lib.remove("b").unwrap();
    assert_eq!(lib.project("thesis").unwrap(), ["alpha", "missing"]);
    lib.remove_from_project("thesis", &["missing".into(), "not-listed".into()]).unwrap();
    assert_eq!(lib.project("thesis").unwrap(), ["alpha"]);
    assert_eq!(lib.projects_of("alpha"), ["thesis"]);

    lib.add_to_project("empty", &[]).unwrap();
    assert_eq!(lib.projects(), ["empty", "thesis"]);
    assert!(lib.search("project:empty", 10).unwrap().is_empty());
    assert!(lib.search("project:nope", 10).is_err());
    assert!(lib.add_to_project("../escape", &[]).is_err());
    assert!(lib.delete_project("nope").is_err());
    lib.delete_project("empty").unwrap();
    assert_eq!(lib.papers().0.len(), 1, "the projects folder is not a paper");

    let tex = tmp.write("doc/main.tex", "\\cite{alpha} % \\cite{commented}\n\\citep[p.~1]{missing2}");
    let (added, missing, uncited) = lib.sync_project("doc", &[tex]).unwrap();
    assert_eq!((added, missing, uncited), (vec!["alpha".to_string()], vec!["missing2".to_string()], vec![]));
}

#[test]
fn attaching_and_supplementary_files() {
    let tmp = Tmp::new("files");
    let lib = library(&tmp);
    paper(&lib, "p", "title = {P}");
    assert!(lib.attach("p", None).unwrap_err().to_string().contains("no arXiv id"));
    assert!(lib.attach("p", Some(&tmp.write("x.txt", "text"))).is_err());
    fs::create_dir_all(lib.root.join("p/figures/old")).unwrap(); // left over from an earlier extraction
    lib.attach("p", Some(&tmp.write("x.pdf", FAKE_PDF))).unwrap();
    assert!(lib.root.join("p/p.pdf").exists() && lib.root.join("p/figures/page-0001-00.png").exists());
    assert!(!lib.root.join("p/figures/old").exists(), "old figures are replaced");
    assert!(lib.attach("p", Some(&tmp.path("x.pdf"))).unwrap_err().to_string().contains("already has a PDF"));

    let mut lib = lib;
    lib.extractor = fake_extractor("supplementaryword appears only here");
    let data = tmp.write("data.csv", "a,b\n1,2\n");
    let appendix = tmp.write("appendix.pdf", FAKE_PDF);
    let dot = tmp.write(".hidden", "x");
    let sources = [data.to_string_lossy().into(), appendix.to_string_lossy().into(), dot.to_string_lossy().into()];
    lib.add_supplementary("p", &sources).unwrap();
    let p = lib.get("p").unwrap();
    assert_eq!(p.supplementary(), ["appendix-figures", "appendix.md", "appendix.pdf", "data.csv", "download"]);
    assert_eq!(lib.search("supplementaryword", 10).unwrap().len(), 1, "supplementary text is searchable");

    assert!(
        lib.add_supplementary("p", &[data.to_string_lossy().into()])
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert!(lib.add_supplementary("p", &[tmp.path("gone.csv").to_string_lossy().into()]).is_err());
    assert!(!lib.root.join("p/supplementary/gone.csv").exists(), "no half-copied leftovers");

    fs::write(lib.root.join("p/supplementary/mine.md"), "my own notes").unwrap();
    let mine = tmp.write("mine.pdf", FAKE_PDF);
    let messages = lib.add_supplementary("p", &[mine.to_string_lossy().into()]).unwrap();
    assert!(messages.iter().any(|m| m.contains("not extracting")), "{messages:?}");
    assert_eq!(read(lib.root.join("p/supplementary/mine.md")), "my own notes", "never overwritten");
}

#[test]
fn drafts() {
    let tmp = Tmp::new("drafts");
    let lib = library(&tmp);
    paper(&lib, "p", "title = {P}, keywords = {old}");
    assert!(lib.propose("p", Field::Summary, " \n\t ").is_err(), "empty");
    assert!(lib.propose("nope", Field::Summary, "x").is_err());
    lib.propose("p", Field::Keywords, "- Graphs\n* graphs; Spectra,, ").unwrap();
    assert_eq!(lib.get("p").unwrap().draft(Field::Keywords).unwrap(), "Graphs\nSpectra\n");
    lib.approve("p", Field::Keywords).unwrap();
    assert_eq!(lib.get("p").unwrap().bib.keywords(), ["Graphs", "Spectra"], "replaces the old keywords");
    assert!(lib.approve("p", Field::Keywords).is_err(), "nothing left to approve");
    assert!(lib.reject("p", Field::Summary).is_err());

    fs::write(lib.root.join("p/notes.md"), "  \n").unwrap();
    lib.propose("p", Field::Notes, "from the AI").unwrap();
    fs::write(lib.root.join("p/drafts/stray.txt"), "not a draft").unwrap();
    lib.approve("p", Field::Notes).unwrap();
    assert_eq!(read(lib.root.join("p/notes.md")), "from the AI\n", "blank notes are replaced, not appended to");
    assert!(lib.root.join("p/drafts/stray.txt").exists(), "other files in drafts/ are left alone");
}

#[test]
fn searching() {
    let tmp = Tmp::new("search");
    let lib = library(&tmp);
    paper(&lib, "muller2021", "title = {Spectral graphs}, author = {Müller, Jörg}, year = 2021, keywords = {graphs}");
    paper(&lib, "doe2020", "title = {Neural nets}, author = {Doe, Jane}, year = 2020");
    fs::write(lib.root.join("doe2020/fulltext.md"), "We train networks on graphs.").unwrap();

    let ids = |q: &str| lib.search(q, 10).unwrap().into_iter().map(|(p, _)| p.id).collect::<Vec<_>>();
    assert_eq!(ids("muller"), ["muller2021"], "without the umlaut");
    assert_eq!(ids("Müller"), ["muller2021"]);
    assert_eq!(ids("graphs"), ["muller2021", "doe2020"], "a keyword beats a word in the full text");
    assert_eq!(ids("author:doe"), ["doe2020"]);
    assert_eq!(ids("year:2021"), ["muller2021"]);
    assert_eq!(ids("spectr"), ["muller2021"], "prefixes match while typing");
    assert_eq!(ids("network"), ["doe2020"], "stemmed");
    assert_eq!(ids("muller2021"), ["muller2021"], "by id");
    assert_eq!(ids("graphs zebra"), ["muller2021", "doe2020"], "no paper has every word: any word counts");
    assert_eq!(ids("").len(), 2);
    for odd in [
        "\"unclosed",
        "a AND",
        "NOT",
        "OR OR",
        "(",
        "*",
        "^x",
        "NEAR(a b)",
        "title:",
        "author:\"x\"",
        "-x",
        "foo:bar",
        "🙂",
        "c++",
        "a'b",
        "{}[]",
        &"long ".repeat(500),
    ] {
        assert!(lib.search(odd, 10).is_ok(), "query {odd:?} failed");
    }

    // The index follows the files.
    fs::write(lib.root.join("doe2020/fulltext.md"), "Now about zebras.").unwrap();
    assert_eq!(ids("zebras"), ["doe2020"]);
    fs::remove_dir_all(lib.root.join("doe2020")).unwrap();
    assert!(ids("zebras").is_empty());

    // It's a cache: a damaged one is rebuilt.
    fs::write(&lib.index, "this is not a database").unwrap();
    assert_eq!(ids("spectral"), ["muller2021"]);

    // Searches from several processes or threads at once.
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let lib = lib.clone();
            std::thread::spawn(move || lib.search("graphs", 10).map(|hits| hits.len()))
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap().unwrap(), 1);
    }
}

#[test]
fn chunks() {
    let text = "äöü".repeat(10); // 30 characters, 60 bytes
    assert!(chunk(&text, 0, 4).starts_with("[characters 0-4 of 30; continue with offset 4]\n\näöüä"));
    assert!(chunk(&text, 28, 100).starts_with("[characters 28-30 of 30; the end]"));
    assert!(chunk(&text, 999, 10).starts_with("[characters 30-30 of 30; the end]"), "past the end");
    assert!(chunk(&text, 0, 0).contains("of 30; the end"), "0 means the default length");
    assert!(chunk("", 0, 10).starts_with("[characters 0-0 of 0; the end]"));
}

/// Three lunar papers: references from a source and from an approved AI draft link them.
#[test]
fn citation_graph() {
    let tmp = Tmp::new("graph");
    let lib = library(&tmp);
    paper(
        &lib,
        "pieters2009",
        "title = {Character and Spatial Distribution of OH/H2O on the Surface of the Moon}, author = {Pieters, Carle M. and Goswami, J. N.}, year = 2009, doi = {10.1126/science.1178658}",
    );
    paper(
        &lib,
        "hiesinger2011",
        "title = {Ages and stratigraphy of lunar mare basalts}, author = {Hiesinger, Harald and Head, James W.}, year = 2011",
    );
    paper(
        &lib,
        "head2010",
        "title = {Global Distribution of Large Lunar Craters}, author = {James W. Head and Fassett, C. I.}, year = 2010, doi = {10.1126/science.1195050}",
    );
    fs::write(
        lib.root.join("head2010/references.json"),
        r#"[{"title": "Character and spatial distribution of OH/H2O", "doi": "10.1126/SCIENCE.1178658", "source": "openalex"},
            {"title": "Lunar Sourcebook", "year": 1991, "source": "openalex"}]"#,
    )
    .unwrap();

    // No source knows hiesinger2011's references: an AI reads them, the user approves.
    let drafted = r#"[{"title": "Global distribution of large lunar craters", "authors": ["Head"], "source": "openalex"},
                      {"title": "Lunar sourcebook!"}]"#;
    lib.propose("hiesinger2011", Field::References, drafted).unwrap();
    assert!(!lib.graph().unwrap().neighbors("hiesinger2011").unwrap().references_known, "a draft doesn't count yet");
    assert!(lib.propose("hiesinger2011", Field::References, "Head et al. 2010").is_err(), "not JSON");
    lib.approve("hiesinger2011", Field::References).unwrap();

    let g = lib.graph().unwrap();
    let n = g.neighbors("head2010").unwrap();
    assert_eq!(
        (n.cites.as_slice(), n.cited_by.as_slice()),
        (&["pieters2009".to_string()][..], &["hiesinger2011".to_string()][..])
    );
    assert_eq!(n.shared_authors["hiesinger2011"], ["James W. Head"], "Head, James W. is the same person");
    assert!(!n.shared_authors.contains_key("pieters2009"));
    assert_eq!(n.not_in_library.len(), 1);
    assert!(!g.neighbors("pieters2009").unwrap().references_known);
    assert!(g.neighbors("nope").is_err());

    let s = g.suggestions(None);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        (s[0].reference.title.as_str(), s[0].cited_by.as_slice()),
        ("Lunar Sourcebook", &["head2010".to_string(), "hiesinger2011".into()][..])
    );
    assert_eq!(g.suggestions(Some(&["head2010".into()]))[0].cited_by, ["head2010"]);
    assert!(g.dot(None).contains("\"head2010\" -> \"pieters2009\";"));

    let refs = lib.references("hiesinger2011", false).unwrap(); // already known: no lookup
    assert_eq!(
        (refs[0].0.source.as_str(), refs[0].1.as_deref()),
        ("ai", Some("head2010")),
        "an AI can't claim a source"
    );

    // Renaming keeps the links: references point at DOIs and titles, not keys.
    lib.rename("head2010", Some("head2010craters")).unwrap();
    assert_eq!(lib.graph().unwrap().neighbors("pieters2009").unwrap().cited_by, ["head2010craters"]);
}

/// A citation tree through cycles, and decisions about people that outlast the evidence.
#[test]
fn trees_and_people() {
    let tmp = Tmp::new("people");
    let lib = library(&tmp);
    paper(&lib, "a", "title = {A}, author = {Clark, Roger N.}");
    paper(&lib, "b", "title = {B}, author = {Clark, R. N. and Pieters, C. M.}");
    paper(&lib, "c", "title = {C}, author = {Pieters, Carle M.}");
    let cites = |id: &str, titles: &[&str]| {
        let refs: Vec<String> = titles.iter().map(|t| format!(r#"{{"title": "{t}", "source": "openalex"}}"#)).collect();
        fs::write(lib.root.join(id).join("references.json"), format!("[{}]", refs.join(","))).unwrap();
    };
    cites("a", &["B", "Elsewhere"]);
    cites("b", &["C", "A"]);
    let g = lib.graph().unwrap();
    let tree = g.tree("a", 3, false).unwrap();
    assert_eq!(tree, [(1, "b".to_string(), false), (2, "a".into(), true), (2, "c".into(), false)], "a cycle ends");
    assert_eq!(g.tree("c", 1, true).unwrap(), [(1, "b".to_string(), false)]);
    assert!(g.tree("nope", 1, false).is_err());

    assert_eq!(g.authors.people(Some("clark")).len(), 2, "single-author paper: no evidence");
    assert_eq!(lib.same_person(&["Clark, Roger N.".into(), "Clark, R. N.".into()]).unwrap(), "clark-roger-n");
    assert!(lib.get("people").is_err() && lib.papers().0.len() == 3, "people/ is not a paper");
    let clark = lib
        .graph()
        .unwrap()
        .authors
        .people(Some("clark"))
        .into_iter()
        .map(|p| (p.id.clone(), p.papers.clone()))
        .collect::<Vec<_>>();
    assert_eq!(clark, [(Some("clark-roger-n".to_string()), vec!["a".to_string(), "b".into()])]);

    // An AI says the two Pieters are different people; the user agrees.
    let draft = lib
        .propose_people(false, &["Pieters, C. M.".into(), "Pieters, Carle M.".into()], "different institutes")
        .unwrap();
    assert_eq!(lib.people_drafts()[0].reason, "different institutes");
    lib.decide_people(&draft, true).unwrap();
    assert!(lib.people_drafts().is_empty());
    assert_eq!(lib.graph().unwrap().authors.people(Some("pieters")).len(), 2);
    // Later the user changes their mind: joining them removes the separate files.
    lib.same_person(&["Pieters, C. M.".into(), "Pieters, Carle M.".into()]).unwrap();
    assert_eq!(lib.people().unwrap().len(), 2, "clark and one pieters");
    assert!(lib.decide_people("nope", true).is_err());

    fs::write(lib.root.join("people/broken.json"), "{").unwrap();
    assert!(lib.doctor().iter().any(|i| i.check == "people"));
    assert!(lib.graph().is_err(), "a broken decision is not silently ignored");
}
