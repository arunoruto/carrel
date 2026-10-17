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
    assert!(c.ok(&["show", "graphtheory"]).contains("graph-theory"));
    assert!(c.ok(&["search", "plain", "text"]).contains("graphtheory"));
    assert!(c.ok(&["bib"]).starts_with("@misc{graphtheory,"));
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
