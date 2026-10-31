//! The library's behavior at its edges, through its public API. Offline.

mod common;

use carrel::{Field, Library, chunk};
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
    // The old key stays as an alias: commands, and citations with biber, still find the paper.
    assert_eq!(lib.get("a").unwrap().id, "muller2021uber");
    assert_eq!(lib.get("muller2021uber").unwrap().bib.aliases(), ["a"]);

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
    assert_eq!(lib.projects(), ["thesis"], "no JabRef metadata, no projects from it");
}

#[test]
fn importing_from_jabref() {
    let tmp = Tmp::new("jabref-import");
    let lib = library(&tmp);
    tmp.write("refs/pdfs/a.pdf", FAKE_PDF);
    tmp.write("refs/pdfs/we:ird;name.pdf", FAKE_PDF);
    tmp.write("refs/pdfs/data.csv", "x\n");
    tmp.write("refs/Über café.pdf", FAKE_PDF); // next to the .bib, not in fileDirectory
    let elsewhere = tmp.write("elsewhere/b.pdf", FAKE_PDF);
    let bib = tmp.write(
        "refs/library.bib",
        format!(
            r"@article{{a, title = {{A}}, file = {{Paper:a.pdf:PDF;Appendix:we\:ird\;name.pdf:PDF;:data.csv:CSV;:missing.pdf:PDF}},
  groups = {{Chapter 1, Methods, old, a;b}}}}
@article{{b, title = {{B}}, file = {{:{}:PDF;:C:\papers\b.pdf:PDF}}, groups = {{Methods}}}}
@article{{c, title = {{C}}, file = {{:Über café.pdf:PDF}}, groups = {{}}}}
@article{{d, title = {{D}}, file = {{:C:\Users\me\d.pdf:PDF}}}}

@Comment{{jabref-meta: databaseType:biblatex;}}
@Comment{{jabref-meta: fileDirectory:pdfs;}}
@Comment{{jabref-meta: grouping:
0 AllEntriesGroup:;
1 StaticGroup:Chapter 1\;0\;1\;\;\;\;;
2 StaticGroup:Methods\;0\;1\;\;\;\;;
1 StaticGroup:Chapter 2\;0\;1\;\;\;\;;
2 StaticGroup:Methods\;0\;1\;\;\;\;;
2 StaticGroup:Methods, old\;0\;1\;\;\;\;;
2 StaticGroup:a\\\;b\;0\;1\;\;\;\;;
1 StaticGroup:Empty\;0\;1\;\;\;\;;
1 KeywordGroup:ml\;0\;keywords\;ml\;0\;0\;1\;\;\;\;;
1 ExplicitGroup:Old\;0\;c\;nokey\;;
}}
",
            elsewhere.display()
        ),
    );
    let (ids, messages) = lib.import_bib(&bib, false).unwrap();
    assert_eq!(ids, ["a", "b", "c", "d"]);
    for expected in [
        "a: skipped missing.pdf: no such file",
        r"b: skipped C:\papers\b.pdf: no such file",
        r"d: skipped C:\Users\me\d.pdf: no such file",
        "group \"Chapter 1\" is now project Chapter-1",
        "group \"Methods, old\" is now project Methods-old",
        "skipped 1 JabRef groups",
    ] {
        assert!(messages.iter().any(|m| m.contains(expected)), "{expected:?} not in {messages:?}");
    }
    let a = lib.get("a").unwrap();
    assert!(a.pdf().is_some(), "the first PDF");
    assert_eq!(a.supplementary(), ["data.csv", "we:ird;name-figures", "we:ird;name.md", "we:ird;name.pdf"]);
    assert!(a.bib.raw("file").is_none() && a.bib.raw("groups").is_none(), "carrel keeps these itself");
    assert!(lib.get("b").unwrap().pdf().is_some(), "an absolute path");
    assert!(lib.get("c").unwrap().pdf().is_some(), "unicode, next to the .bib");
    assert!(lib.get("d").unwrap().pdf().is_none());
    assert_eq!(lib.projects(), ["Chapter-1", "Chapter-2", "Empty", "Methods", "Methods-old", "Old", "ab"]);
    assert_eq!(lib.project("Methods").unwrap(), ["b"], "both Methods groups are one project");
    assert_eq!(lib.project("Methods-old").unwrap(), ["a"], "a name with a comma");
    assert_eq!(lib.project("ab").unwrap(), ["a"]);
    assert_eq!(lib.project("Chapter-1").unwrap(), ["a"]);
    assert_eq!(lib.project("Old").unwrap(), ["c"], "old JabRef files list the keys in the group");
    assert!(lib.project("Empty").unwrap().is_empty());
    assert_eq!(lib.projects_of("a"), ["Chapter-1", "Methods-old", "ab"]);

    // Importing again matches the papers and adds no duplicates to the projects.
    let (again, _) = lib.import_bib(&bib, false).unwrap();
    assert_eq!(again, ids);
    assert_eq!(lib.project("Methods").unwrap(), ["b"]);
}

#[test]
fn jabref_round_trip() {
    let tmp = Tmp::new("jabref-round-trip");
    let lib = library(&tmp);
    paper(&lib, "smith:2020", "title = {With a colon}, journal = {J}, year = 2020");
    paper(&lib, "b", "title = {B}");
    paper(&lib, "c", "title = {C}");
    lib.attach("smith:2020", Some(&tmp.write("x.pdf", FAKE_PDF))).unwrap();
    let sources = [tmp.write("Anhang ü.pdf", FAKE_PDF), tmp.write("data.csv", "1,2\n")];
    lib.add_supplementary("smith:2020", &sources.map(|s| s.to_string_lossy().into_owned())).unwrap();
    lib.add_to_project("thesis", &["smith:2020".into(), "b".into()]).unwrap();
    lib.add_to_project("talk", &["smith:2020".into()]).unwrap();
    lib.add_to_project("empty", &[]).unwrap();

    let bib = lib.jabref_bib(&lib.papers().0);
    assert!(bib.contains(r"{:smith\:2020/smith\:2020.pdf:PDF;:smith\:2020/supplementary/Anhang ü.pdf:PDF;:smith\:2020/supplementary/data.csv:}"), "{bib}");
    assert!(bib.contains("groups  = {talk, thesis}"), "{bib}");
    assert!(bib.contains(&format!("@Comment{{jabref-meta: fileDirectory:{};}}", lib.root.display())));
    assert!(bib.contains("1 StaticGroup:thesis\\;0\\;1\\;\\;\\;\\;;\n"));
    assert!(!bib.contains("databaseType"), "the entries are BibTeX, not biblatex");
    assert!(!bib.contains("Anhang ü.md"), "carrel's extracted text isn't linked");
    assert_eq!(carrel::bib::Entry::parse_all(&bib).len(), 3, "the comments aren't entries");
    paper(&lib, "d", "title = {D}, date = {2020-01-01}");
    assert!(lib.jabref_bib(&[lib.get("d").unwrap()]).contains("databaseType:biblatex"));
    fs::remove_dir_all(lib.root.join("d")).unwrap();

    let file = tmp.write("export/carrel.bib", &bib);
    let mut other = Library::open(tmp.path("other")).unwrap();
    other.index = tmp.path("other.sqlite");
    other.extractor = lib.extractor.clone();
    let (ids, messages) = other.import_bib(&file, false).unwrap();
    assert_eq!(ids, ["b", "c", "smith:2020"], "{messages:?}");
    let p = other.get("smith:2020").unwrap();
    assert!(p.pdf().is_some());
    assert_eq!(p.supplementary(), lib.get("smith:2020").unwrap().supplementary());
    assert_eq!(p.bib, lib.get("smith:2020").unwrap().bib);
    assert_eq!(other.projects(), lib.projects());
    for name in lib.projects() {
        // JabRef's groups have no order: the members come in the file's order.
        let sorted = |lib: &Library| -> Vec<String> {
            let mut ids = lib.project(&name).unwrap();
            ids.sort();
            ids
        };
        assert_eq!(sorted(&other), sorted(&lib), "{name}");
    }
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

    // Citations of a key from before a rename count for the paper.
    let tex = tmp.write("doc/old.tex", "\\cite{a} \\cite{alpha}");
    let (added, missing, uncited) = lib.sync_project("old", &[tex]).unwrap();
    assert_eq!((added, missing, uncited), (vec!["alpha".to_string()], vec![], vec![]));
    lib.add_to_project("old", &["a".into()]).unwrap();
    assert_eq!(lib.project("old").unwrap(), ["alpha"], "listed by its current key");
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
    paper(&lib, "p", "title = {P}, keywords = {old, graphs}");
    assert!(lib.propose("p", Field::Summary, " \n\t ").is_err(), "empty");
    assert!(lib.propose("nope", Field::Summary, "x").is_err());
    lib.propose("p", Field::Keywords, "- Graphs\n* graphs; Spectra,, ").unwrap();
    assert_eq!(lib.get("p").unwrap().draft(Field::Keywords).unwrap(), "Graphs\nSpectra\n");
    lib.approve("p", Field::Keywords).unwrap();
    assert_eq!(lib.get("p").unwrap().bib.keywords(), ["old", "graphs", "Spectra"], "added; your tags stay, no doubles");
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

/// A tiny I, Librarian data folder: its database with only what the import reads, and one PDF.
#[test]
fn moving_from_ilibrarian() {
    let tmp = Tmp::new("ilibrarian");
    let lib = library(&tmp);
    tmp.write("ilib/pdfs/000/000/000000001.pdf", FAKE_PDF);
    fs::create_dir_all(tmp.path("ilib/database")).unwrap();
    let db = rusqlite::Connection::open(tmp.path("ilib/database/main.db")).unwrap();
    db.execute_batch(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT);
        CREATE TABLE items (id INTEGER PRIMARY KEY, title TEXT, primary_title_id INTEGER, secondary_title_id INTEGER,
            tertiary_title_id INTEGER, publication_date TEXT, volume TEXT, issue TEXT, pages TEXT, abstract TEXT,
            publisher TEXT, place_published TEXT, reference_type TEXT, bibtex_id TEXT, bibtex_type TEXT, urls TEXT);
        CREATE TABLE primary_titles (id INTEGER PRIMARY KEY, primary_title TEXT);
        CREATE TABLE secondary_titles (id INTEGER PRIMARY KEY, secondary_title TEXT);
        CREATE TABLE tertiary_titles (id INTEGER PRIMARY KEY, tertiary_title TEXT);
        CREATE TABLE authors (id INTEGER PRIMARY KEY, first_name TEXT, last_name TEXT);
        CREATE TABLE items_authors (item_id INTEGER, author_id INTEGER, position INTEGER);
        CREATE TABLE editors (id INTEGER PRIMARY KEY, first_name TEXT, last_name TEXT);
        CREATE TABLE items_editors (item_id INTEGER, editor_id INTEGER, position INTEGER);
        CREATE TABLE uids (id INTEGER PRIMARY KEY, uid_type TEXT, uid TEXT, item_id INTEGER);
        CREATE TABLE keywords (id INTEGER PRIMARY KEY, keyword TEXT);
        CREATE TABLE items_keywords (item_id INTEGER, keyword_id INTEGER);
        CREATE TABLE tags (id INTEGER PRIMARY KEY, tag TEXT);
        CREATE TABLE items_tags (item_id INTEGER, tag_id INTEGER);
        CREATE TABLE projects (id INTEGER PRIMARY KEY, user_id INTEGER, project TEXT);
        CREATE TABLE projects_items (project_id INTEGER, item_id INTEGER, added_time TEXT);
        CREATE TABLE item_notes (id INTEGER PRIMARY KEY, user_id INTEGER, item_id INTEGER, note TEXT);
        CREATE TABLE markers (id INTEGER PRIMARY KEY, user_id INTEGER, item_id INTEGER, page INTEGER,
            marker_position INTEGER, marker_top INTEGER, marker_color TEXT, marker_text TEXT);
        CREATE TABLE annotations (id INTEGER PRIMARY KEY, user_id INTEGER, item_id INTEGER, page INTEGER, annotation TEXT);

        INSERT INTO users VALUES (1, 'mar');
        INSERT INTO primary_titles VALUES (1, 'JQSRT');
        INSERT INTO secondary_titles VALUES (1, 'Journal of Quantitative Spectroscopy and Radiative Transfer');
        INSERT INTO items VALUES (1, 'CELES: CUDA-accelerated simulation of
            electromagnetic scattering', 1, 1, NULL, '2017-06-06', '199', NULL, '1-12', 'Uses 10<sup>5</sup> spheres',
            NULL, NULL, 'article', 'Egel-2017-ID1', 'article', 'https://doi.org/10.1016%2Fj.jqsrt.2017.05.010');
        INSERT INTO items VALUES (2, 'Celes: CUDA-accelerated simulation of electromagnetic scattering', NULL, NULL,
            NULL, '2017', NULL, NULL, NULL, NULL, NULL, NULL, 'article', 'Egel-2017-ID2', 'article', NULL);
        INSERT INTO items VALUES (3, 'Introduction to Mineralogy', NULL, NULL, NULL, '2000-01-01', NULL, NULL, NULL,
            NULL, 'Oxford University Press', 'New York', 'book', 'Nesse-2000-ID3', 'book', NULL);
        INSERT INTO authors VALUES (1, 'Amos', 'Egel'), (2, 'Uli', 'Lemmer'), (3, 'William D.', 'Nesse');
        INSERT INTO items_authors VALUES (1, 2, 2), (1, 1, 1), (2, 1, 1), (3, 3, 1);
        INSERT INTO uids VALUES (1, 'DOI', '10.1016/j.jqsrt.2017.05.010', 1), (2, 'ARXIV', '1706.02145v1', 1),
            (3, 'NASAADS', '2017JQSRT.199..103E', 1), (4, 'ISBN', '9780195346268', 3);
        INSERT INTO keywords VALUES (1, 'T-matrix');
        INSERT INTO items_keywords VALUES (1, 1);
        INSERT INTO tags VALUES (1, 'Scattering'), (2, 'CUDA'), (3, 'Mineral');
        INSERT INTO items_tags VALUES (1, 1), (1, 2), (3, 3);
        INSERT INTO projects VALUES (1, 1, '2025 Space Weathering'), (2, 1, 'Empty');
        INSERT INTO projects_items VALUES (1, 3, '2024'), (1, 2, '2025');
        INSERT INTO item_notes VALUES (1, 1, 3, '<p>Source for the <a href=\"http://x.org/a\">olivine data.</a></p>');
        INSERT INTO markers VALUES (1, 1, 1, 3, 43, 159, 'Y', 'Equations'), (2, 1, 1, 3, 44, 159, 'Y', 'form'),
            (3, 1, 1, 3, 52, 174, 'Y', 'a system.'), (4, 1, 1, 3, 53, 174, 'R', 'Another'), (5, 1, 1, 4, 1, 10, 'Y', 'Next page');
        INSERT INTO annotations VALUES (1, 1, 1, 3, 'H(x'')');",
    )
    .unwrap();

    let imported = lib.import_ilibrarian(&tmp.path("ilib")).unwrap();
    assert_eq!(imported.papers[&1], "egel2017celes");
    assert_eq!(imported.papers[&2], "egel2017celes", "the same paper twice in I, Librarian: one paper");
    assert!(
        imported.messages.iter().any(|m| m.contains("already in the library as egel2017celes")),
        "{:?}",
        imported.messages
    );
    assert!(imported.messages.iter().any(|m| m.contains("\"2025 Space Weathering\" is now 2025-Space-Weathering")));

    let p = lib.get("egel2017celes").unwrap();
    assert_eq!(p.bib.title(), "CELES: CUDA-accelerated simulation of electromagnetic scattering", "one line");
    assert_eq!(p.bib.authors(), ["Egel, Amos", "Lemmer, Uli"], "in their order");
    assert_eq!(p.bib.venue().as_deref(), Some("Journal of Quantitative Spectroscopy and Radiative Transfer"));
    assert_eq!((p.bib.year(), p.bib.arxiv().as_deref()), (Some(2017), Some("1706.02145")));
    assert_eq!(p.bib.keywords(), ["CUDA", "Scattering", "T-matrix"], "tags, then the publisher's keywords");
    assert_eq!(p.bib.get("ids").as_deref(), Some("Egel-2017-ID1"), "old citations keep working");
    assert_eq!(p.bib.get("pages").as_deref(), Some("1--12"));
    assert!(p.bib.raw("abstract").unwrap().contains("10$^{5}$"));
    assert!(
        p.bib.raw("adsurl").unwrap().contains("2017JQSRT.199..103E") && p.bib.get("url").is_none(),
        "a doi.org URL is not kept"
    );
    assert!(p.pdf().is_some());
    let notes = p.get(Field::Notes).unwrap();
    assert!(notes.contains("- p. 3: \u{201c}Equations form a system.\u{201d}"), "one passage over two lines: {notes}");
    assert!(
        notes.contains("- p. 3: \u{201c}Another\u{201d}") && notes.contains("- p. 4: \u{201c}Next page\u{201d}"),
        "{notes}"
    );
    assert!(notes.contains("- p. 3, margin note: H(x')") && !notes.contains("(mar)"), "{notes}");

    let book = lib.get("nesse2000introduction").unwrap();
    assert_eq!((book.bib.kind.as_str(), book.bib.get("isbn").as_deref()), ("book", Some("9780195346268")));
    assert_eq!(book.get(Field::Notes).unwrap(), "Source for the [olivine data.](http://x.org/a)\n");
    assert_eq!(lib.project("2025-Space-Weathering").unwrap(), ["nesse2000introduction", "egel2017celes"]);
    assert!(lib.projects().len() == 1, "empty projects are left out");

    let again = lib.import_ilibrarian(&tmp.path("ilib")).unwrap();
    assert_eq!(lib.papers().0.len(), 2, "importing again adds nothing");
    assert_eq!(again.papers.len(), 3);
}

#[test]
fn sharing_through_a_remote() {
    use carrel::remote::{Action, Folder, Keep};
    let tmp = Tmp::new("remote");
    let a = library(&tmp);
    let mut b = Library::open(tmp.path("libB")).unwrap();
    b.index = tmp.path("indexB.sqlite");
    let remote = Folder(tmp.path("remote"));
    let sync = |lib: &Library, keep| {
        let report = lib.sync(&remote, keep, false, |_, _| {}).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        report.done
    };
    assert!(a.sync(&remote, None, false, |_, _| {}).unwrap_err().to_string().contains("doesn't exist"));
    fs::create_dir(&remote.0).unwrap();

    let (id, _) = a.add(&tmp.write("in/graph-theory.pdf", FAKE_PDF).to_string_lossy()).unwrap();
    tmp.write("lib/.tmp/half-added/x.pdf", "hidden files stay here");
    let dry = a.sync(&remote, None, true, |_, _| {}).unwrap().done;
    assert!(dry.iter().all(|(_, action)| *action == Action::Upload) && !dry.is_empty());
    assert!(fs::read_dir(&remote.0).unwrap().next().is_none(), "a dry run changes nothing");
    let uploaded = sync(&a, None);
    assert_eq!(uploaded.len(), dry.len());
    assert!(!remote.0.join(".tmp").exists() && !remote.0.join(".carrel").exists());
    assert!(sync(&a, None).is_empty(), "nothing changed since");

    // The colleague gets the paper, and their notes come back.
    assert_eq!(sync(&b, None).len(), uploaded.len());
    assert_eq!(b.get(&id).unwrap().bib.title(), "graph-theory");
    fs::write(b.root.join(&id).join("notes.md"), "from b").unwrap();
    assert_eq!(sync(&b, None), [(format!("{id}/notes.md"), Action::Upload)]);
    assert_eq!(sync(&a, None), [(format!("{id}/notes.md"), Action::Download)]);
    assert_eq!(fs::read_to_string(a.root.join(&id).join("notes.md")).unwrap(), "from b");

    // Both change the notes: a conflict, until one side is kept; the loser goes to the trash.
    fs::write(a.root.join(&id).join("notes.md"), "a's").unwrap();
    fs::write(b.root.join(&id).join("notes.md"), "b's").unwrap();
    sync(&a, None);
    assert_eq!(sync(&b, None), [(format!("{id}/notes.md"), Action::Conflict)]);
    assert_eq!(fs::read_to_string(b.root.join(&id).join("notes.md")).unwrap(), "b's", "left alone");
    assert_eq!(sync(&b, Some(Keep::Remote)), [(format!("{id}/notes.md"), Action::Download)]);
    assert_eq!(fs::read_to_string(b.root.join(&id).join("notes.md")).unwrap(), "a's");
    assert_eq!(fs::read_to_string(b.root.join(".carrel/trash").join(&id).join("notes.md")).unwrap(), "b's");

    // An edit beats a deletion; a deletion of an unchanged file goes through, into the trash.
    fs::write(b.root.join(&id).join("notes.md"), "b edits").unwrap();
    fs::remove_file(a.root.join(&id).join("notes.md")).unwrap();
    sync(&a, None);
    assert_eq!(sync(&b, None), [(format!("{id}/notes.md"), Action::Upload)]);
    assert_eq!(sync(&a, None), [(format!("{id}/notes.md"), Action::Download)]);
    tmp.write("lib/reading-list.txt", "keeps the remote from emptying");
    sync(&a, None);
    sync(&b, None);
    a.remove(&id).unwrap();
    assert!(sync(&a, None).iter().all(|(_, action)| *action == Action::DeleteRemote));
    assert!(sync(&b, None).iter().all(|(_, action)| *action == Action::DeleteLocal));
    assert!(!b.root.join(&id).exists(), "no empty folder is left behind");
    assert!(b.papers().0.is_empty());
    assert!(b.root.join(".carrel/trash").join(&id).join(format!("{id}.pdf")).exists());

    // A remote that suddenly holds nothing (a wrong prefix, a wiped bucket) is refused, not mirrored.
    fs::remove_file(a.root.join("reading-list.txt")).unwrap();
    sync(&a, None);
    let refused = b.sync(&remote, None, false, |_, _| {}).unwrap_err().to_string();
    assert!(refused.contains("is empty, but 1 files here were synced with it"), "{refused}");
    assert!(b.root.join("reading-list.txt").exists());
}

/// Hands out a made-up listing, or edits a file while it uploads to a folder.
struct Rigged<'a> {
    folder: &'a carrel::remote::Folder,
    listing: Vec<&'a str>,
    edit: Option<std::path::PathBuf>,
}

impl carrel::remote::Remote for Rigged<'_> {
    fn name(&self) -> String {
        self.folder.name()
    }
    fn list(&self) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
        let mut files = self.folder.list()?;
        files.extend(self.listing.iter().map(|key| (key.to_string(), "etag".to_string())));
        Ok(files)
    }
    fn download(&self, path: &str, to: &std::path::Path) -> anyhow::Result<()> {
        match self.listing.contains(&path) {
            true => Ok(fs::write(to, "from the remote")?),
            false => self.folder.download(path, to),
        }
    }
    fn upload(&self, path: &str, bytes: Vec<u8>) -> anyhow::Result<String> {
        if let Some(file) = &self.edit {
            fs::write(file, "edited during the upload").unwrap();
        }
        self.folder.upload(path, bytes)
    }
    fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.folder.delete(path)
    }
}

#[test]
fn syncing_safely() {
    use carrel::remote::{Action, Folder};
    let tmp = Tmp::new("sync-safely");
    let lib = library(&tmp);
    let folder = Folder(tmp.path("remote"));
    fs::create_dir(&folder.0).unwrap();
    let sync = |remote: &Rigged| lib.sync(remote, None, false, |_, _| {});

    // Keys are checked before they become paths: nothing lands outside the library or in `.carrel`.
    let bad = ["", "../escaped", "./a", "/abs", "a/../../escaped", "a//b", "a\\b", "folder/"]; // as listed: sorted
    let listing = [&bad[..], &[".carrel/sync.json", "a2020/notes.md"]].concat();
    let report = sync(&Rigged { folder: &folder, listing, edit: None }).unwrap();
    assert_eq!(report.done, [("a2020/notes.md".to_string(), Action::Download)]);
    assert_eq!(report.skipped, bad, "hidden keys are skipped quietly, like hidden files here");
    assert!(!tmp.path("escaped").exists() && !tmp.path("lib/folder").exists());
    assert!(!fs::read_to_string(lib.root.join(".carrel/sync.json")).unwrap().contains("escaped"));

    // A file edited while it uploads goes up again at the next sync.
    let notes = tmp.write("lib/b2021/notes.md", "first");
    let edit = Rigged { folder: &folder, listing: vec!["a2020/notes.md"], edit: Some(notes) };
    assert_eq!(sync(&edit).unwrap().done, [("b2021/notes.md".to_string(), Action::Upload)]);
    let plain = Rigged { folder: &folder, listing: vec!["a2020/notes.md"], edit: None };
    assert_eq!(sync(&plain).unwrap().done, [("b2021/notes.md".to_string(), Action::Upload)]);
    assert_eq!(fs::read_to_string(folder.0.join("b2021/notes.md")).unwrap(), "edited during the upload");

    // One sync at a time.
    let lock = fs::File::open(lib.root.join(".carrel/sync.lock")).unwrap();
    lock.lock().unwrap();
    assert!(sync(&plain).unwrap_err().to_string().contains("another sync is running"));
    drop(lock);
    assert!(sync(&plain).unwrap().done.is_empty());
}

#[test]
fn semantic_search() {
    use carrel::embed::{Config, Ollama};
    let tmp = Tmp::new("semantic");
    let lib = library(&tmp);
    let config = |url: &str, model: Option<&str>| Config { ollama: Some(url.into()), model: model.map(String::from) };
    let connect = |url: &str, model| Ollama::connect(&config(url, model)).map(|o| o.model);
    let error = |url: &str, model| format!("{:#}", Ollama::connect(&config(url, model)).err().unwrap());

    // Which model: the configured one, else bge-m3, else any that embeds.
    let (url, embedded) = fake_ollama(&[("llama3", false), ("nomic-embed-text", true), ("bge-m3", true)]);
    assert_eq!(connect(&url, None).unwrap(), "bge-m3");
    assert_eq!(connect(&url, Some("nomic-embed-text:latest")).unwrap(), "nomic-embed-text");
    assert!(error(&url, Some("llama3")).contains("llama3 on Ollama at http://127.0.0.1"), "not an embedding model");
    assert!(error(&url, Some("mxbai")).contains("doesn't have mxbai; `ollama pull mxbai` there"));
    let (other, _) = fake_ollama(&[("llama3", false), ("nomic-embed-text", true)]);
    assert_eq!(connect(other.trim_start_matches("http://"), None).unwrap(), "nomic-embed-text", "no scheme");
    let (none, _) = fake_ollama(&[("llama3", false)]);
    assert!(error(&none, None).contains("has no embedding model; `ollama pull bge-m3` there"));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    assert!(error(&format!("http://{closed}"), None).contains(&format!("cannot reach Ollama at http://{closed}")));

    paper(&lib, "moon", "title = {Lunar Surfaces}, keywords = {regolith}");
    fs::write(
        lib.root.join("moon/fulltext.md"),
        "The regolith of the lunar surface darkens through space weathering.\n",
    )
    .unwrap();
    paper(&lib, "graphs", "title = {Graph Theory}");
    fs::write(lib.root.join("graphs/fulltext.md"), "Every planar graph can be coloured with four colours.\n").unwrap();
    lib.add_to_project("thesis", &["graphs".into()]).unwrap();
    let ollama = Ollama::connect(&config(&url, None)).unwrap();
    for p in lib.papers().0 {
        assert!(!lib.embedded(&p, "bge-m3"));
        assert!(lib.embed(&ollama, &p).unwrap(), "computed");
        assert!(!lib.embed(&ollama, &p).unwrap() && lib.embedded(&p, "bge-m3"), "and then up to date");
    }
    assert_eq!(embedded.load(std::sync::atomic::Ordering::Relaxed), 4, "head and text of two papers, once");
    let stored: serde_json::Value = serde_json::from_str(&read(lib.root.join("moon/embeddings.json"))).unwrap();
    assert_eq!(stored["model"], "bge-m3", "the file says which model made it");

    let hits = lib.semantic_search(&ollama, "space weathering darkens", 10).unwrap();
    assert_eq!(hits[0].0.id, "moon");
    assert!(hits[0].1.contains("full text at 0: The regolith"), "{}", hits[0].1);
    assert_eq!(lib.semantic_search(&ollama, "Lunar regolith", 10).unwrap()[0].1.split(' ').nth(1), Some("title"));
    let within = lib.semantic_search(&ollama, "space weathering project:thesis", 10).unwrap();
    assert_eq!(within.iter().map(|(p, _)| p.id.as_str()).collect::<Vec<_>>(), ["graphs"]);

    // Alike papers come from the stored vectors alone: each paper's nearest, scored.
    let similar = lib.similar(5).unwrap();
    assert_eq!(similar["moon"].iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["graphs"]);
    assert!((-1.0..=1.0).contains(&similar["moon"][0].1));
    assert_eq!(similar["graphs"][0].0, "moon");
    assert!(lib.semantic_search(&ollama, "project:thesis", 10).is_err(), "nothing to search for");

    // A new text, or another model, makes them out of date.
    fs::write(lib.root.join("graphs/fulltext.md"), "Trees are graphs without cycles.\n").unwrap();
    let graphs = lib.get("graphs").unwrap();
    assert!(!lib.embedded(&graphs, "bge-m3") && lib.embed(&ollama, &graphs).unwrap());
    assert!(!lib.embedded(&graphs, "nomic-embed-text"));
    let nomic = Ollama::connect(&config(&url, Some("nomic-embed-text"))).unwrap();
    assert!(lib.semantic_search(&nomic, "space weathering", 10).unwrap().is_empty(), "no vectors by that model");
}

#[test]
fn consuming() {
    use carrel::consume::find;
    let tmp = Tmp::new("consume");
    let lib = library(&tmp);
    let dirs = [tmp.path("Downloads")];
    tmp.write("Downloads/2301.01234v2.pdf", FAKE_PDF);
    tmp.write("Downloads/plain.PDF", FAKE_PDF);
    tmp.write("Downloads/with-doi.pdf", b"%PDF-1.4\n<prism:doi>10.1029/2000JE001402</prism:doi>\n%%EOF");
    tmp.write("Downloads/sub/deeper.pdf", FAKE_PDF);
    tmp.write("Downloads/empty.pdf", "");
    tmp.write("Downloads/loading.pdf", [FAKE_PDF, &[b'x'; 2000]].concat()); // still being written
    tmp.write("Downloads/firefox.pdf", ""); // next to the firefox.pdf.part it writes
    let old = tmp.write("Downloads/old.pdf", FAKE_PDF);
    let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 86400);
    fs::File::options().write(true).open(&old).unwrap().set_modified(month_ago).unwrap();

    let found = find(&dirs, 30);
    let names: Vec<(String, Option<String>)> =
        found.iter().map(|f| (f.path.file_name().unwrap().to_string_lossy().into_owned(), f.clue.clone())).collect();
    let mut sorted = names.clone();
    sorted.sort();
    let clue = |c: &str| Some(c.to_string());
    assert_eq!(
        sorted,
        [
            ("2301.01234v2.pdf".into(), clue("arXiv id 2301.01234 in its name")),
            ("plain.PDF".into(), None),
            ("with-doi.pdf".into(), clue("DOI 10.1029/2000JE001402 in the file")),
        ]
    );
    assert_eq!(find(&dirs, 40).len(), 4, "older ones with more days");

    let plain = found.iter().find(|f| f.path.ends_with("plain.PDF")).unwrap();
    assert!(lib.consume(plain, true).unwrap_err().to_string().contains("not recognised as a paper"));
    assert!(plain.path.exists() && lib.papers().0.is_empty(), "refused: left where it was");
    let (id, _) = lib.consume(plain, false).unwrap();
    assert!(!plain.path.exists(), "moved");
    assert!(lib.get(&id).unwrap().pdf().is_some());
}
