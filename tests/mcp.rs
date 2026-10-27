//! The MCP server over stdio, the way an AI client talks to it. Offline.

mod common;

use base64::Engine;
use common::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    fn start(tmp: &Tmp) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_carrel"))
            .arg("mcp")
            .env("CARREL_HOME", tmp.path("lib"))
            .env("XDG_CACHE_HOME", tmp.path("cache"))
            .env("XDG_CONFIG_HOME", tmp.path("config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (stdin, stdout) = (child.stdin.take().unwrap(), BufReader::new(child.stdout.take().unwrap()));
        let mut client = Self { child, stdin, stdout, next: 0 };
        let info = json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}});
        let init = client.request("initialize", info);
        assert_eq!(init["result"]["serverInfo"]["name"], "carrel");
        assert!(init["result"]["instructions"].as_str().unwrap().contains("propose"));
        client.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string());
        client
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// The whole response message (with `result` or `error`).
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string());
        loop {
            let mut line = String::new();
            assert!(self.stdout.read_line(&mut line).unwrap() > 0, "the server hung up");
            let message: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
            if message["id"] == id {
                return message;
            }
        }
    }

    /// A tool's result: its first content item and whether it is an error.
    fn call(&mut self, tool: &str, args: Value) -> (Value, bool) {
        let response = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let result = &response["result"];
        (result["content"][0].clone(), result["isError"] == true)
    }

    /// A tool's text, which must not be an error.
    fn text(&mut self, tool: &str, args: Value) -> String {
        let (content, error) = self.call(tool, args.clone());
        assert!(!error, "{tool} {args} failed: {content}");
        content["text"].as_str().unwrap().to_string()
    }

    fn json(&mut self, tool: &str, args: Value) -> Value {
        serde_json::from_str(&self.text(tool, args)).unwrap()
    }

    fn error(&mut self, tool: &str, args: Value) -> String {
        let (content, error) = self.call(tool, args.clone());
        assert!(error, "{tool} {args} should have failed: {content}");
        content["text"].as_str().unwrap_or_default().to_string()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 1, 2, 3];

fn fixture() -> Tmp {
    let tmp = Tmp::new("mcp");
    tmp.write("lib/vaswani2017attention/vaswani2017attention.bib",
        "@misc{vaswani2017attention, title = {Attention Is All You Need}, author = {Vaswani, Ashish and Shazeer, Noam}, year = 2017, eprint = {1706.03762}}");
    tmp.write(
        "lib/vaswani2017attention/fulltext.md",
        format!("# Attention\n{}", "Self-attention über alles. ".repeat(1000)),
    );
    tmp.write("lib/vaswani2017attention/summary.md", "Transformers replace recurrence.\n");
    tmp.write("lib/vaswani2017attention/figures/page-0003-00.png", PNG);
    tmp.write("lib/vaswani2017attention/supplementary/appendix.md", "Appendix text.");
    tmp.write(
        "lib/ho2020denoising/ho2020denoising.bib",
        "@misc{ho2020denoising, title = {Denoising Diffusion Probabilistic Models}}",
    );
    tmp.write("lib/projects/thesis.txt", "vaswani2017attention\n");
    tmp
}

#[test]
fn tools() {
    let tmp = fixture();
    let mut ai = Client::start(&tmp);
    let tools = ai.request("tools/list", json!({}))["result"]["tools"].clone();
    let mut names: Vec<&str> = tools.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "authors",
            "bibtex",
            "doctor",
            "get_figure",
            "get_paper",
            "list_papers",
            "list_projects",
            "neighbors",
            "propose",
            "propose_people",
            "read_paper",
            "related",
            "search",
            "suggest",
            "tree"
        ]
    );
    for t in tools.as_array().unwrap() {
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 20) && t["inputSchema"]["type"] == "object", "{t}");
    }

    let hits = ai.json("search", json!({"query": "attention"}));
    assert_eq!(hits[0]["id"], "vaswani2017attention");
    assert!(hits[0]["match"].as_str().unwrap().contains('['), "a snippet marks the match");
    assert_eq!(ai.json("search", json!({"query": "project:thesis"})).as_array().unwrap().len(), 1);
    assert_eq!(ai.json("search", json!({"query": "\"(*", "limit": 1})).as_array().unwrap().len(), 0);
    assert_eq!(ai.json("list_papers", json!({})).as_array().unwrap().len(), 2);
    assert_eq!(ai.json("list_papers", json!({"project": "thesis"}))[0]["id"], "vaswani2017attention");
    ai.error("list_papers", json!({"project": "nope"}));
    assert_eq!(ai.json("list_projects", json!({})), json!([{"name": "thesis", "papers": ["vaswani2017attention"]}]));

    let paper = ai.json("get_paper", json!({"id": "vaswani2017attention"}));
    assert_eq!(paper["authors"], json!(["Vaswani, Ashish", "Shazeer, Noam"]));
    assert_eq!((paper["year"].clone(), paper["arxiv"].clone()), (json!(2017), json!("1706.03762")));
    assert_eq!(paper["figures"], json!(["page-0003-00.png"]));
    assert_eq!(paper["supplementary"], json!(["appendix.md"]));
    assert_eq!(paper["projects"], json!(["thesis"]));
    assert_eq!(paper["fulltext_chars"], json!(27012));
    assert!(paper["summary"].as_str().unwrap().contains("recurrence"));
    assert!(ai.error("get_paper", json!({"id": "../../etc"})).contains("invalid paper id"));
    assert!(ai.error("get_paper", json!({"id": "nope"})).contains("no paper"));

    tmp.write(
        "lib/vaswani2017attention/references.json",
        r#"[{"title": "Denoising diffusion probabilistic models", "source": "ads"}]"#,
    );
    assert_eq!(ai.json("neighbors", json!({"id": "ho2020denoising"}))["cited_by"], json!(["vaswani2017attention"]));
    let refs = ai.json("related", json!({"id": "vaswani2017attention"}));
    assert_eq!((refs[0]["source"].clone(), refs[0]["in_library"].clone()), (json!("ads"), json!("ho2020denoising")));
    assert_eq!(ai.json("suggest", json!({})), json!([]), "everything it cites is in the library");
    assert!(ai.error("related", json!({"id": "ho2020denoising"})).contains("no source has its references"));
    ai.text("propose", json!({"id": "ho2020denoising", "field": "references", "content": "[{\"title\": \"Gone\"}]"}));
    assert_eq!(ai.json("suggest", json!({})), json!([]), "a draft counts only once approved");
}

#[test]
fn reading() {
    let tmp = fixture();
    let mut ai = Client::start(&tmp);
    let first = ai.text("read_paper", json!({"id": "vaswani2017attention"}));
    assert!(first.starts_with("[characters 0-20000 of 27012; continue with offset 20000]"), "{}", &first[..80]);
    let rest = ai.text("read_paper", json!({"id": "vaswani2017attention", "offset": 20000}));
    assert!(rest.starts_with("[characters 20000-27012 of 27012; the end]"));
    let piece = ai.text("read_paper", json!({"id": "vaswani2017attention", "offset": 12, "length": 4}));
    assert!(piece.ends_with("\n\nSelf"), "{piece}");
    assert!(ai.text("read_paper", json!({"id": "vaswani2017attention", "offset": 99999})).contains("27012-27012"));
    assert!(
        ai.text("read_paper", json!({"id": "vaswani2017attention", "file": "appendix.md"})).ends_with("Appendix text.")
    );
    assert!(
        ai.error("read_paper", json!({"id": "vaswani2017attention", "file": "../vaswani2017attention.bib"}))
            .contains("no supplementary file")
    );
    assert!(ai.error("read_paper", json!({"id": "ho2020denoising"})).contains("no extracted text"));
    ai.error("read_paper", json!({"id": "vaswani2017attention", "offset": -1}));

    let (image, error) = ai.call("get_figure", json!({"id": "vaswani2017attention", "name": "page-0003-00.png"}));
    assert!(!error && image["type"] == "image" && image["mimeType"] == "image/png", "{image}");
    let bytes = base64::engine::general_purpose::STANDARD.decode(image["data"].as_str().unwrap()).unwrap();
    assert_eq!(bytes, PNG);
    for name in ["../vaswani2017attention.bib", "/etc/passwd", "", "page-0003-00.png/.."] {
        ai.error("get_figure", json!({"id": "vaswani2017attention", "name": name}));
    }
}

#[test]
fn writing_and_more() {
    let tmp = fixture();
    let mut ai = Client::start(&tmp);
    assert!(
        ai.text("propose", json!({"id": "ho2020denoising", "field": "summary", "content": "A diffusion model."}))
            .contains("draft")
    );
    assert_eq!(read(tmp.path("lib/ho2020denoising/drafts/summary.md")), "A diffusion model.\n");
    ai.text(
        "propose",
        json!({"id": "ho2020denoising", "field": "keywords", "content": "diffusion, Diffusion\n- denoising"}),
    );
    assert_eq!(read(tmp.path("lib/ho2020denoising/drafts/keywords.txt")), "diffusion\ndenoising\n");
    assert!(!tmp.path("lib/ho2020denoising/summary.md").exists(), "nothing is approved behind the user's back");
    assert!(
        ai.error("propose", json!({"id": "ho2020denoising", "field": "title", "content": "x"}))
            .contains("unknown variant")
    );
    ai.error("propose", json!({"id": "ho2020denoising", "field": "notes", "content": "   "}));
    ai.error("propose", json!({"id": "../x", "field": "notes", "content": "x"}));
    ai.error("propose", json!({"id": "ho2020denoising", "field": "notes"}));

    let bib = ai.text("bibtex", json!({"project": "thesis"}));
    assert!(bib.starts_with("@misc{vaswani2017attention,") && !bib.contains("ho2020"));
    assert_eq!(carrel::bib::Entry::parse_all(&ai.text("bibtex", json!({}))).len(), 2);
    ai.error("bibtex", json!({"ids": ["nope"]}));
    let issues = ai.json("doctor", json!({}));
    assert!(issues.as_array().unwrap().iter().any(|i| i["check"] == "pdf" && i["id"] == "vaswani2017attention"));

    // Mistakes by the client don't take the server down.
    assert!(!ai.request("tools/call", json!({"name": "no_such_tool", "arguments": {}}))["error"].is_null());
    ai.send("this is not JSON");
    assert_eq!(ai.json("list_projects", json!({}))[0]["name"], "thesis", "still answering");
}
