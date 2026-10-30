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
            .env("XDG_STATE_HOME", self.tmp.path("state"))
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
    assert!(c.fails(&["sync"]).contains("no remote to sync with"));
    c.tmp.write("config/carrel/config.toml", "");
    assert!(c.ok(&["config"]).contains("embeddings: off; add [embeddings]"));
    assert!(c.fails(&["search", "-s", "moon"]).contains("needs [embeddings] in the config"));
    let (ollama, _) = fake_ollama(&[("bge-m3", true)]);
    c.tmp.write("config/carrel/config.toml", format!("[embeddings]\nollama = \"{ollama}\"\n"));
    assert!(c.ok(&["config"]).contains(&format!("embeddings: bge-m3 on {ollama}")));
    assert!(c.ok(&["embed", "--all"]).contains("embedding with bge-m3"));
    c.tmp.write("config/carrel/config.toml", "[embeddings]\nollama = \"http://127.0.0.1:9\"\n");
    assert!(c.ok(&["config"]).contains("embeddings: not working: cannot reach Ollama at http://127.0.0.1:9"));
    assert!(c.fails(&["embed", "--all"]).contains("cannot reach Ollama"));
    std::fs::create_dir_all(c.tmp.path("shared")).unwrap();
    c.tmp.write("lib/a2020/a2020.bib", "@misc{a2020, title = {A}}\n");
    c.tmp.write(
        "config/carrel/config.toml",
        format!("[remote]\ntype = \"folder\"\npath = \"{}\"\n", c.tmp.path("shared").display()),
    );
    let dry = c.ok(&["sync", "-n"]);
    assert!(dry.contains("a2020/a2020.bib") && dry.contains("would upload"), "{dry}");
    assert!(!c.tmp.path("shared/a2020").exists());
    assert!(c.ok(&["sync"]).contains("download 0, delete 0 here and 0 remotely"));
    assert_eq!(read(c.tmp.path("shared/a2020/a2020.bib")), "@misc{a2020, title = {A}}\n");
    c.tmp.write("config/carrel/config.toml", "[remote]\ntype = \"s3\"\nbucket = \"b\"\n");
    let out = c.command(&["sync"]).env_remove("AWS_ENDPOINT_URL").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs an endpoint"), "{out:?}");
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

#[test]
fn serving_over_http() {
    use std::io::{BufRead, BufReader};
    let c = Carrel::new("cli-serve");
    let open = c.command(&["serve", "--listen", "0.0.0.0:0"]).env_remove("CARREL_SERVE_TOKEN").output().unwrap();
    assert!(String::from_utf8_lossy(&open.stderr).contains("open to anyone"), "no token, no network");

    let start = |listen: &str, token: Option<&str>| {
        let mut command = c.command(&["serve", "--listen", listen, "--every", "60"]);
        command.env_remove("CARREL_SERVE_TOKEN").stdout(Stdio::piped());
        if let Some(token) = token {
            command.env("CARREL_SERVE_TOKEN", token);
        }
        let mut server = command.spawn().unwrap();
        let mut lines = BufReader::new(server.stdout.take().unwrap()).lines();
        let serving = lines.find_map(|l| l.ok().filter(|l| l.contains("serving"))).expect("the server starts");
        let url = serving.split_whitespace().last().unwrap().trim_end_matches("/mcp").to_string();
        // The output stays open: a server whose log can't be written fails.
        (server, url.replace("0.0.0.0", "127.0.0.1"), lines)
    };
    let (mut server, url, _log) = start("127.0.0.1:0", Some("s3cret"));

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();
    let mut health = agent.get(&format!("{url}/health")).call().unwrap();
    assert_eq!((health.status().as_u16(), health.body_mut().read_to_string().unwrap()), (200, "ok\n".into()));
    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
    let post_to = |url: &str, token: &str| {
        agent
            .post(&format!("{url}/mcp"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .send(initialize)
            .unwrap()
    };
    let post = |token: &str| post_to(&url, token);
    assert_eq!(post("wrong").status().as_u16(), 401);
    assert_eq!(agent.get(&format!("{url}/mcp")).call().unwrap().status().as_u16(), 401, "no token");
    let mut ok = post("s3cret");
    assert_eq!(ok.status().as_u16(), 200);
    let body = ok.body_mut().read_to_string().unwrap();
    assert!(body.contains("\"name\":\"carrel\""), "{body}");
    // SIGTERM, as from `docker stop`, stops it at once.
    unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    let started = std::time::Instant::now();
    assert!(server.wait().unwrap().success());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));

    // Trusted networks need no token; everyone else does, and without one set nobody else gets in.
    c.tmp.write("config/carrel/config.toml", "[serve]\ntrusted = [\"127.0.0.1\"]\n");
    let (mut server, url, _log) = start("127.0.0.1:0", Some("s3cret"));
    assert_eq!(post_to(&url, "").status().as_u16(), 200, "trusted");
    server.kill().unwrap();
    // Behind a proxy, what it says in X-Forwarded-For.
    c.tmp.write("config/carrel/config.toml", "[serve]\ntrusted = [\"100.64.0.0/10\"]\nproxies = [\"127.0.0.1\"]\n");
    let (mut server, url, _log) = start("127.0.0.1:0", Some("s3cret"));
    let forwarded = |from: &str| {
        let request = agent.post(&format!("{url}/mcp")).header("X-Forwarded-For", from);
        let request = request.header("Accept", "application/json, text/event-stream");
        request.header("Content-Type", "application/json").send(initialize).unwrap().status().as_u16()
    };
    assert_eq!(forwarded("100.99.1.2"), 200);
    assert_eq!(forwarded("100.99.1.2, 8.8.8.8"), 401);
    assert_eq!(post_to(&url, "").status().as_u16(), 401, "the proxy itself isn't trusted");
    server.kill().unwrap();
    c.tmp.write("config/carrel/config.toml", "[serve]\ntrusted = [\"100.64.0.0/10\"]\n");
    let (mut server, url, _log) = start("0.0.0.0:0", None);
    assert_eq!(post_to(&url, "").status().as_u16(), 401, "not trusted, and no token to send");
    server.kill().unwrap();
    c.tmp.write("config/carrel/config.toml", "[serve]\ntrusted = [\"100.64.0.0/33\"]\n");
    assert!(c.fails(&["serve"]).contains("not a network"));
}

#[test]
fn consuming_downloads() {
    let c = Carrel::new("cli-consume");
    let (smtp, mails) = fake_smtp();
    let downloads = c.tmp.path("Downloads");
    c.tmp.write("Downloads/2301.01234v2.pdf", FAKE_PDF);
    c.tmp.write("Downloads/invoice.pdf", FAKE_PDF);
    c.tmp.write("Downloads/notes.txt", "not a pdf");
    // Folders on the command line beat the config's.
    c.tmp.write("Elsewhere/2301.01234v2.pdf", FAKE_PDF);
    c.tmp.write("config/carrel/config.toml", format!("[consume]\nfolders = [\"{}\"]\n", downloads.display()));
    let out = c.ok(&["consume", "--days", "1", &c.tmp.path("Elsewhere").to_string_lossy()]);
    assert!(out.contains("Elsewhere/2301.01234v2.pdf") && !out.contains("invoice"), "{out}");
    let config = format!(
        "[consume]\nfolders = [\"{}\"]\n\n[notify.email]\nserver = \"{smtp}\"\nfrom = \"carrel@x.org\"\nto = \"me@x.org\"\n",
        downloads.display()
    );
    c.tmp.write("config/carrel/config.toml", &config);

    // From a timer (no terminal): it tells, once.
    let out = c.ok(&["consume"]);
    assert!(
        out.contains("New, waiting for `carrel consume`")
            && out.contains("2301.01234v2.pdf  (arXiv id 2301.01234 in its name)"),
        "{out}"
    );
    assert!(!out.contains("notes.txt"));
    let mail = mails.lock().unwrap().pop().expect("a mail");
    assert!(mail.contains("Subject: carrel found 2 new PDFs\r\n") && mail.contains("To: me@x.org"), "{mail}");
    assert!(mail_body(&mail).contains("invoice.pdf"));
    assert!(c.ok(&["consume"]).contains("nothing new"));
    assert!(mails.lock().unwrap().is_empty(), "told once");

    // --yes adds what it can identify; offline, the fake text identifies nothing.
    let out = c.ok(&["consume", "--yes"]);
    assert!(out.contains("Not added") && out.contains("not recognised as a paper"), "{out}");
    assert!(downloads.join("invoice.pdf").exists(), "what isn't added stays");
    assert!(c.ok(&["list"]).is_empty());

    assert!(c.ok(&["notify", "hello"]).contains("email to me@x.org: sent"));
    assert_eq!(mail_body(&mails.lock().unwrap().pop().unwrap()), "hello\r\n");
    c.tmp.write("config/carrel/config.toml", config.replace(&smtp, "127.0.0.1:9"));
    assert!(c.fails(&["notify"]).contains("cannot reach 127.0.0.1:9"));
    c.tmp.write("config/carrel/config.toml", "");
    assert!(c.fails(&["notify"]).contains("no notifiers"));
}

/// One HTTP/1.1 request on a fresh connection, written out by hand so the Host header can lie.
fn http(addr: &str, method: &str, path: &str, headers: &[&str], body: &[u8]) -> (u16, String, Vec<u8>) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !headers.iter().any(|h| h.to_lowercase().starts_with("host:")) {
        head.push_str(&format!("Host: {addr}\r\n"));
    }
    for h in headers {
        head.push_str(&format!("{h}\r\n"));
    }
    stream.write_all(format!("{head}\r\n").as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut response = vec![];
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut body = response[split + 4..].to_vec();
    // hyper sends bodies of unknown length chunked; the API's are all known, but be sure.
    if head.to_lowercase().contains("transfer-encoding: chunked") {
        let text = String::from_utf8_lossy(&body).into_owned();
        body = text.split("\r\n").skip(1).step_by(2).collect::<String>().into_bytes();
    }
    (status, head, body)
}

#[test]
fn web_ui() {
    use std::io::{BufRead, BufReader};
    let c = Carrel::new("cli-web");
    assert!(c.fails(&["web", "--no-open", "--listen", "0.0.0.0:0"]).contains("this machine only"));

    let mut command = c.command(&["web", "--no-open", "--listen", "127.0.0.1:0"]);
    let mut server = command.stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(server.stdout.take().unwrap()).lines();
    let link = lines.next().unwrap().unwrap();
    let link = link.strip_prefix("carrel web: ").expect("the link first").to_string();
    let addr = link.trim_start_matches("http://").split('/').next().unwrap().to_string();
    let token = link.split("token=").nth(1).unwrap().to_string();
    let token_file = c.tmp.path("state/carrel/web-token");
    assert_eq!(std::fs::read_to_string(&token_file).unwrap(), token, "kept for the next start");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777, 0o600);

    let get = |path: &str, headers: &[&str]| http(&addr, "GET", path, headers, b"");
    // Strangers: no cookie, another site's name (DNS rebinding), a wrong link.
    assert_eq!(get("/api/info", &[]).0, 401);
    assert_eq!(get("/", &["Host: evil.example:80"]).0, 403);
    let port = addr.rsplit(':').next().unwrap();
    assert_eq!(get("/api/info", &[&format!("Host: attacker.example:{port}")]).0, 403);
    assert_eq!(get("/?token=guess", &[]).0, 403);

    // The link trades the token for a cookie and leaves the address bar clean.
    let (status, head, _) = get(&format!("/?token={token}"), &[]);
    assert_eq!(status, 303, "{head}");
    assert!(
        head.contains("location: /\r") || head.contains("location: /\n") || head.ends_with("location: /"),
        "{head}"
    );
    let cookie = head.lines().find_map(|l| l.strip_prefix("set-cookie: ")).expect("a cookie").to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"), "{cookie}");
    let cookie = format!("Cookie: {}", cookie.split(';').next().unwrap());
    let api = |method: &str, path: &str, body: &[u8]| {
        let (status, _, body) = http(&addr, method, path, &[&cookie], body);
        (status, String::from_utf8_lossy(&body).into_owned())
    };
    let json = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();

    let (status, info) = api("GET", "/api/info", b"");
    assert_eq!(status, 200, "{info}");
    assert_eq!(json(&info)["remote"], false);
    assert_eq!(api("GET", "/api/papers", b""), (200, "[]".into()));

    // A PDF from the browser becomes a paper.
    let (status, added) = api("POST", "/api/upload?name=My%20Paper.pdf", FAKE_PDF);
    assert_eq!(status, 200, "{added}");
    let id = json(&added)["id"].as_str().unwrap().to_string();
    let papers = json(&api("GET", "/api/papers", b"").1);
    assert_eq!(papers[0]["id"], id.as_str());
    assert_eq!(papers[0]["pdf"], true);
    let (status, pdf) = api("GET", &format!("/api/papers/{id}/pdf"), b"");
    assert_eq!((status, pdf.as_bytes()), (200, FAKE_PDF));
    assert_eq!(api("GET", &format!("/api/papers/{id}/fulltext"), b"").0, 200);

    // Notes are yours: written as sent, and gone when emptied.
    let (status, paper) = api("PUT", &format!("/api/papers/{id}/notes"), "Read §3 again.".as_bytes());
    assert_eq!(status, 200, "{paper}");
    assert_eq!(json(&paper)["notes"], "Read §3 again.");
    assert_eq!(json(&api("PUT", &format!("/api/papers/{id}/notes"), b"  ").1)["notes"], serde_json::Value::Null);
    let (_, paper) = api("PUT", &format!("/api/papers/{id}/keywords"), br#"{"keywords": ["regolith", " ", "optics"]}"#);
    assert_eq!(json(&paper)["keywords"], serde_json::json!(["regolith", "optics"]));
    assert!(c.ok(&["show", &id]).contains("regolith"));
    let (_, projects) = api("PUT", &format!("/api/papers/{id}/projects/thesis"), b"");
    assert_eq!(json(&projects)["projects"], serde_json::json!(["thesis"]));
    assert_eq!(json(&api("GET", "/api/papers?project=thesis", b"").1).as_array().unwrap().len(), 1);

    // Drafts from an AI wait for the human here too.
    c.tmp.write(&format!("lib/{id}/drafts/summary.md"), "An AI's summary.");
    let drafts = json(&api("GET", "/api/drafts", b"").1);
    assert_eq!(drafts[0]["field"], "summary");
    let (status, paper) = api("POST", &format!("/api/papers/{id}/drafts/summary/approve"), b"");
    assert_eq!(status, 200, "{paper}");
    assert_eq!(json(&paper)["summary"], "An AI's summary.");

    // Files only from the paper's own folders.
    c.tmp.write(&format!("lib/{id}/figures/page-0001-01.png"), "png");
    let (status, head, _) =
        http(&addr, "GET", &format!("/api/papers/{id}/files/figures/page-0001-01.png"), &[&cookie], b"");
    assert_eq!(status, 200);
    assert!(head.contains("content-security-policy: sandbox"), "no scripts from files: {head}");
    assert_eq!(api("GET", &format!("/api/papers/{id}/files/figures/..%2F{id}.bib"), b"").0, 400);
    assert_eq!(api("GET", &format!("/api/papers/{id}/files/drafts/summary.md"), b"").0, 404);
    assert_eq!(api("GET", "/api/papers/..%2F..%2Fetc/pdf", b"").0, 400);

    // Writes from another site's page are refused even with the cookie.
    let (status, _, _) =
        http(&addr, "DELETE", &format!("/api/papers/{id}"), &[&cookie, "Origin: http://evil.example"], b"");
    assert_eq!(status, 403);
    assert_eq!(api("GET", "/api/nothing", b"").0, 404);
    assert_eq!(api("DELETE", &format!("/api/papers/{id}"), b"").0, 200);
    assert_eq!(api("GET", "/api/papers", b"").1, "[]");
    server.kill().unwrap();
    server.wait().unwrap();
}
