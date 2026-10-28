//! The `carrel` binary end to end, in a sandboxed environment. Offline.

mod common;

use common::*;
use std::process::{Command, Output, Stdio};

/// Runs carrel with its library, cache and config inside a temporary folder.
struct Carrel {
    tmp: Tmp,
}

impl Carrel {
    fn new(name: &str) -> Self {
        Self { tmp: Tmp::new(name) }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_carrel"));
        cmd.args(args)
            .env("CARREL_HOME", self.tmp.path("lib"))
            .env("XDG_CACHE_HOME", self.tmp.path("cache"))
            .env("XDG_CONFIG_HOME", self.tmp.path("config"))
            .env("CARREL_EXTRACTOR", fake_extractor("Plain text without a heading or identifier."))
            .env("VISUAL", "true")
            .env_remove("S2_API_KEY")
            .env_remove("ADS_API_TOKEN")
            .env_remove("ADS_DEV_KEY")
            .env_remove("OPENALEX_API_KEY")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Stdout of a command that must succeed.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "carrel {args:?} failed: {stderr}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// Stderr of a command that must fail.
    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "carrel {args:?} should have failed");
        String::from_utf8(out.stderr).unwrap()
    }

    fn script(&self, name: &str, body: &str) -> String {
        let path = self.tmp.write(name, format!("#!/bin/sh\n{body}\n"));
        Command::new("chmod").arg("+x").arg(&path).status().unwrap();
        path.display().to_string()
    }
}

#[test]
fn papers() {
    let c = Carrel::new("cli-papers");
    assert!(c.ok(&["config"]).contains(&c.tmp.path("lib").display().to_string()));
    assert_eq!(c.ok(&["list"]), "");
    let pdf = c.tmp.write("graph-theory.pdf", FAKE_PDF);
    let out = c.ok(&["add", pdf.to_str().unwrap()]);
    assert!(out.starts_with("added graphtheory\n"), "{out}");
    assert!(c.fails(&["add", pdf.to_str().unwrap()]).contains("already in the library"));
    assert!(c.fails(&["add", "./missing.pdf"]).contains("no such file"));
    assert!(c.ok(&["list"]).starts_with("graphtheory"));
    let papers: serde_json::Value = serde_json::from_str(&c.ok(&["list", "--json"])).unwrap();
    assert_eq!(papers[0]["id"], "graphtheory");
    let fulltext = std::fs::read_to_string(papers[0]["files"]["fulltext"].as_str().unwrap()).unwrap();
    assert!(fulltext.contains("Plain text"), "the path to the extracted text: {fulltext}");
    let shown: serde_json::Value = serde_json::from_str(&c.ok(&["show", "graphtheory", "--json"])).unwrap();
    assert_eq!(shown, papers[0]);
    assert!(c.ok(&["show", "graphtheory"]).contains("graph-theory"));
    assert!(c.ok(&["search", "plain", "text"]).contains("graphtheory"));
    assert!(c.ok(&["bib"]).starts_with("@misc{graphtheory,"));
    let pdf_path = c.tmp.path("lib/graphtheory/graphtheory.pdf").canonicalize().unwrap().display().to_string(); // /var is /private/var on macOS
    assert!(c.ok(&["bib", "--files"]).contains(&format!("{{:{pdf_path}:PDF}}")), "JabRef's file field");
    assert!(!c.ok(&["bib"]).contains("file ="), "only when asked");
    assert!(c.fails(&["show", "nope"]).contains("no paper"));
    assert!(c.fails(&["show", "../etc"]).contains("invalid paper id"));

    // Editing: a broken .bib is reported, and can still be opened to fix it.
    let breaker = c.script("break.sh", r#"echo '@misc{graphtheory, title = {never closed' > "$1""#);
    let out = c.command(&["edit", "graphtheory"]).env("VISUAL", &breaker).output().unwrap();
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("invalid"));
    assert!(c.ok(&["doctor"]).contains("error   graphtheory"));
    let fixer = c.script("fix.sh", r#"echo '@misc{graphtheory, title = {Fixed}}' > "$1""#);
    assert!(c.command(&["edit", "graphtheory"]).env("VISUAL", &fixer).status().unwrap().success());
    assert!(c.ok(&["show", "graphtheory"]).starts_with("Fixed"));

    let other = c.tmp.write("other.pdf", FAKE_PDF);
    assert!(c.fails(&["attach", "graphtheory", other.to_str().unwrap()]).contains("already has a PDF"));
    let csv = c.tmp.write("data.csv", "x\n");
    assert!(c.ok(&["addto", "graphtheory", csv.to_str().unwrap()]).contains("added data.csv"));
    assert!(c.ok(&["show", "graphtheory"]).contains("supplementary: data.csv"));

    assert!(c.fails(&["update", "graphtheory"]).contains("no DOI or arXiv id"), "fails before going online");
    assert_eq!(c.ok(&["update", "--all"]), "", "no paper has an id to update from");
    assert_eq!(c.ok(&["rename", "graphtheory"]), "fixed\n", "the key template, from the new title");
    c.ok(&["rm", "fixed"]);
    assert_eq!(c.ok(&["list"]), "");
    assert_eq!(c.ok(&["doctor"]), "all good\n");
}

#[test]
fn usage_errors() {
    let c = Carrel::new("cli-usage");
    for args in [
        &["add"][..],
        &["frobnicate"],
        &["approve", "x", "title"],
        &["update", "x", "--all"],
        &["project", "remove", "p"],
    ] {
        let out = c.run(args);
        assert_eq!(out.status.code(), Some(2), "carrel {args:?} is a usage error");
    }
}

#[test]
fn config_file() {
    let c = Carrel::new("cli-config");
    let pdf = c.tmp.write("graph-theory.pdf", FAKE_PDF);
    c.tmp.write(
        "config/carrel/config.toml",
        "key = \"paper-{{ word }}\"\nextractor = \"printf 'from the config' > fulltext.md\"\n",
    );
    let out = c.command(&["add", pdf.to_str().unwrap()]).env_remove("CARREL_EXTRACTOR").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("added paper-graphtheory"), "{out:?}");
    assert_eq!(read(c.tmp.path("lib/paper-graphtheory/fulltext.md")), "from the config", "the config's extractor ran");
    assert!(c.ok(&["config"]).contains("paper-{{ word }}"));

    c.tmp.write("config/carrel/config.toml", "default = \"list --json\"\n");
    assert!(c.ok(&[]).trim_start().starts_with('['), "carrel alone runs the default");
    c.tmp.write("config/carrel/config.toml", "default = \"frobnicate\"\n");
    assert!(c.fails(&[]).contains("the config's default \"frobnicate\""));
    c.tmp.write("config/carrel/config.toml", "theme = \"nope\"\n");
    assert!(c.fails(&["config"]).contains("no theme \"nope\""));
    let secret = c.tmp.write("ads-token", "  abc123\n");
    let tokens = format!("[tokens]\nads = {{ file = \"{}\" }}\ns2 = \"xyz\"\n", secret.display());
    c.tmp.write("config/carrel/config.toml", tokens);
    let out = c.ok(&["config"]);
    assert!(out.contains("tokens:  ads, s2\n") && !out.contains("abc123"), "{out}");
    c.tmp.write("config/carrel/config.toml", "[tokens]\nads = { file = \"/no/such/file\" }\n");
    assert!(c.fails(&["config"]).contains("cannot read /no/such/file"));
    c.tmp.write("config/carrel/config.toml", "keys = \"typo\"\n");
    assert!(c.fails(&["list"]).contains("config.toml is invalid"));
    c.tmp.write("config/carrel/config.toml", "key = \"{{ year \"\n");
    assert!(c.fails(&["list"]).contains("key template"));
    c.tmp.write("config/carrel/config.toml", format!("library = \"{}\"\n", c.tmp.path("elsewhere").display()));
    let out = c.command(&["config"]).env_remove("CARREL_HOME").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("elsewhere"), "the config's library");
}

#[test]
fn projects_and_imports() {
    let c = Carrel::new("cli-projects");
    let bib = c.tmp.write("refs.bib", "@article{a2020, title = {A}, author = {A, B}, year = 2020}\n@misc{b2021, title = {B}}\n@misc{c2022, title = {C}}\n");
    let out = c.ok(&["import", bib.to_str().unwrap(), "-p", "thesis"]);
    assert!(out.contains("3 entries"), "{out}");
    assert_eq!(c.ok(&["project"]), format!("{:<30} 3 papers\n", "thesis"));
    let thesis: serde_json::Value = serde_json::from_str(&c.ok(&["list", "-p", "thesis", "--json"])).unwrap();
    assert_eq!(thesis.as_array().unwrap().len(), 3);
    assert_eq!(
        (&thesis[0]["authors"], &thesis[0]["projects"]),
        (&serde_json::json!(["A, B"]), &serde_json::json!(["thesis"]))
    );
    assert!(c.fails(&["list", "-p", "nope"]).contains("no project"));
    c.tmp.write("doc/main.tex", "\\cite{a2020,c2022} % \\cite{b2021}\n\\cite{nowhere}");
    c.tmp.write("doc/talk.typ", "As shown by @a2020 and #cite(<b2021>).");
    let out = c.ok(&[
        "project",
        "sync",
        "talk",
        c.tmp.path("doc/main.tex").to_str().unwrap(),
        c.tmp.path("doc/talk.typ").to_str().unwrap(),
    ]);
    assert!(
        out.contains("added to talk: a2020, c2022, b2021")
            && out.contains("not in the library (or not citations): nowhere"),
        "{out}"
    );
    let entries = carrel::bib::Entry::parse_all(&c.ok(&["bib", "-p", "talk"]));
    assert_eq!(entries.len(), 3);
    assert_eq!(
        c.ok(&["search", "project:talk", "author:a"]).lines().next().unwrap().split_whitespace().next(),
        Some("a2020")
    );
    c.ok(&["project", "remove", "talk", "a2020"]);
    assert!(!c.ok(&["project", "show", "talk"]).contains("a2020"));
    c.ok(&["rename", "b2021", "bee"]);
    assert!(c.ok(&["project", "show", "thesis"]).contains("bee"));
    c.ok(&["project", "delete", "talk"]);
    assert!(c.fails(&["project", "show", "talk"]).contains("no project"));
}

#[test]
fn drafts_and_doctor() {
    let c = Carrel::new("cli-drafts");
    c.tmp.write("lib/p/p.bib", "@misc{p, title = {P}, eprint = {2101.00001}}");
    c.tmp.write("lib/p/drafts/summary.md", "An AI summary.\n");
    assert_eq!(c.ok(&["drafts"]), format!("{:<30} summary\n", "p"));
    assert!(c.fails(&["reject", "p", "keywords"]).contains("no keywords draft"));
    c.ok(&["approve", "p", "summary"]);
    assert_eq!(read(c.tmp.path("lib/p/summary.md")), "An AI summary.\n");
    assert_eq!(c.ok(&["drafts"]), "");

    let out = c.ok(&["doctor"]);
    assert!(out.contains("missing author, year") && out.contains("1 without a PDF, 1 arXiv preprints"), "{out}");
    assert!(c.ok(&["doctor", "--all"]).contains("carrel attach p"), "an arXiv id: attach can download it");
    let issues: serde_json::Value = serde_json::from_str(&c.ok(&["doctor", "--json"])).unwrap();
    assert_eq!(issues.as_array().unwrap().len(), 3);
}

#[test]
fn quiet_on_a_closed_pipe() {
    // `carrel bib | head` must stop quietly, not panic, when head stops reading.
    let c = Carrel::new("cli-pipe");
    c.tmp.write("lib/p/p.bib", format!("@misc{{p, abstract = {{{}}}}}", "long ".repeat(400_000)));
    let mut child = c.command(&["bib"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    drop(child.stdout.take()); // nobody reads: the pipe fills and breaks
    let out = child.wait_with_output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"), "{}", String::from_utf8_lossy(&out.stderr));
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(out.status.signal(), Some(13), "ended by SIGPIPE");
}

#[test]
fn graph() {
    let c = Carrel::new("cli-graph");
    c.tmp.write("lib/a/a.bib", "@misc{a, title = {A}, author = {Smith, Jane and Doe, John}}"); // no ids: refs --all stays offline
    c.tmp.write("lib/b/b.bib", "@misc{b, title = {B}, author = {J. Smith and J. Doe}}");
    c.tmp.write(
        "lib/b/references.json",
        r#"[{"title": "A", "source": "openalex"}, {"title": "Outside", "year": 2000, "source": "openalex"}]"#,
    );
    let refs = c.ok(&["refs", "b"]);
    assert!(refs.contains("[openalex]  [in library: a]") && refs.contains("Outside (2000)"), "{refs}");
    let neighbors = c.ok(&["graph", "a"]);
    assert!(
        neighbors.contains("cited by:  b") && neighbors.contains("authors:   b (Smith, Jane; Doe, John)"),
        "{neighbors}"
    );
    assert!(c.ok(&["graph", "b"]).contains("1 of its references are not in the library"));
    let smith = c.ok(&["authors", "smith"]);
    assert!(
        smith.starts_with("Smith, Jane  (also J. Smith)\n  papers:     a, b\n  co-authors: Doe, John (2)"),
        "{smith}"
    );
    assert_eq!(c.ok(&["authors", "nobody"]), "");
    assert_eq!(c.ok(&["authors", "same", "Smith, Jane", "J. Smith"]), "smith-jane\n");
    assert!(c.ok(&["authors", "smith"]).contains("[confirmed: smith-jane]"));
    assert_eq!(c.run(&["authors", "same", "only one"]).status.code(), Some(2));
    assert_eq!(c.ok(&["tree", "b"]), "b  B\n  a  A\n");
    assert_eq!(c.ok(&["tree", "a", "--cited-by"]), "a  A\n  b  B\n");
    c.tmp.write(
        "lib/people/drafts/1.json",
        r#"{"same": false, "spellings": ["Doe, John", "J. Doe"], "reason": "two Does"}"#,
    );
    assert!(c.ok(&["drafts"]).contains("people 1"), "people drafts are listed too");
    c.ok(&["authors", "reject", "1"]);
    assert!(!c.ok(&["drafts"]).contains("people"));
    assert!(c.ok(&["graph"]).starts_with("  1× Outside (2000)"));
    let json: serde_json::Value = serde_json::from_str(&c.ok(&["graph", "--json"])).unwrap();
    assert_eq!(json[0]["cited_by"], serde_json::json!(["b"]));
    assert!(c.ok(&["graph", "--dot"]).contains("\"b\" -> \"a\";"));
    c.tmp.write("lib/c/c.bib", "@misc{c, title = {C}}");
    let err = c.fails(&["refs", "c"]);
    assert!(err.contains("openalex: needs a DOI") && err.contains("an AI can read them"), "offline, no ids: {err}");
    let all = c.ok(&["refs", "--all"]);
    assert!(all.contains("c: no source") && !all.contains("b:"), "b has its references already: {all}");
    assert_eq!(c.run(&["refs"]).status.code(), Some(2), "an id or --all");
    assert_eq!(c.run(&["graph", "a", "--dot"]).status.code(), Some(2));
}
