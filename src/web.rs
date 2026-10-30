//! `carrel web`: the library in your browser, for this machine only. It serves the web UI (built from
//! `web/` and embedded at compile time) and the JSON API under `/api` that the UI uses.
//!
//! Only loopback, and still guarded: any website you visit can make your browser send requests to
//! localhost. So requests must name localhost in `Host` (against DNS rebinding) and carry a cookie that
//! only the link `carrel web` prints sets (against other sites and other users on the machine).

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use carrel::{Config, Field, Library, Paper, consume};
use http_body_util::{BodyExt, Full, Limited, combinators::BoxBody};
use hyper::{Method, Request, Response, StatusCode, header};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/web.rs"));
}

type Body = BoxBody<Bytes, Infallible>;

/// Uploads (PDFs, supplementary files) up to this size.
const MAX_BODY: usize = 512 << 20;

/// The address `--listen` means, checked to be on this machine.
fn address(listen: Option<String>) -> Result<SocketAddr> {
    let listen = listen.unwrap_or_else(|| "127.0.0.1:7700".into());
    let addr: SocketAddr = listen.parse().with_context(|| format!("cannot listen on {listen:?}"))?;
    if !addr.ip().is_loopback() {
        bail!("carrel web is for this machine only; to share the library, see `carrel serve`");
    }
    Ok(addr)
}

fn link(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/?token={token}")
}

/// The link to open, with the token a running (or later started) `carrel web` on that address uses:
/// `carrel web --get-url`, for machines without a journal to find it in.
pub fn url(listen: Option<String>) -> Result<String> {
    let addr = address(listen)?;
    Ok(link(addr.port(), &token()?))
}

pub fn run(lib: Library, listen: Option<String>, open: bool) -> Result<()> {
    let addr = address(listen)?;
    let token = token()?;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("cannot listen on {addr}"))?;
        let url = link(listener.local_addr()?.port(), &token);
        println!("carrel web: {url}");
        if assets::FILES.is_empty() {
            println!("This carrel was built without the web UI; only the API under /api answers.");
        }
        if open {
            let _ = crate::open(Path::new(&url));
        }
        serve(lib, listener, token).await
    })
}

/// The secret in the link, kept across restarts so a logged-in browser stays logged in.
fn token() -> Result<String> {
    let path = carrel::state_dir().context("no folder to keep the web UI's token in")?.join("web-token");
    if let Ok(token) = fs::read_to_string(&path)
        && token.trim().len() >= 32
    {
        return Ok(token.trim().to_string());
    }
    let mut bytes = [0u8; 24];
    use std::io::Read;
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
    std::io::Write::write_all(&mut file, token.as_bytes())?;
    Ok(token)
}

pub async fn serve(lib: Library, listener: tokio::net::TcpListener, token: String) -> Result<()> {
    let port = listener.local_addr()?.port();
    let web = Arc::new(Web { lib, token, port });
    loop {
        let (stream, _) = listener.accept().await?;
        let web = web.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let web = web.clone();
                async move { Ok::<_, Infallible>(web.handle(request).await) }
            });
            let io = hyper_util::rt::TokioIo::new(stream);
            let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
        });
    }
}

struct Web {
    lib: Library,
    token: String,
    port: u16,
}

impl Web {
    fn cookie(&self) -> String {
        format!("carrel-{}", self.port)
    }

    async fn handle(&self, request: Request<hyper::body::Incoming>) -> Response<Body> {
        let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default();
        let local = ["127.0.0.1", "localhost", "[::1]"].iter().any(|name| host == format!("{name}:{}", self.port));
        if !local {
            return text(StatusCode::FORBIDDEN, "carrel web only answers to localhost\n");
        }
        let path = request.uri().path().to_string();
        let query = query(request.uri().query().unwrap_or_default());
        // The link `carrel web` prints: trade the token for a cookie, and drop it from the address bar.
        if let Some(sent) = query.get("token") {
            if !same(sent, &self.token) {
                return text(StatusCode::FORBIDDEN, "that link is out of date; open the one `carrel web` printed\n");
            }
            let mut response = redirect(&path);
            let cookie =
                format!("{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000", self.cookie(), self.token);
            response.headers_mut().insert(header::SET_COOKIE, cookie.parse().expect("a valid header"));
            return response;
        }
        let Some(api) = path.strip_prefix("/api/") else { return asset(&path) };
        if !self.logged_in(&request) {
            return json_status(StatusCode::UNAUTHORIZED, json!({"error": "open the link `carrel web` printed"}));
        }
        // SameSite keeps the cookie off other sites' requests; Origin is the second lock on writes.
        let origin = request.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok());
        if request.method() != Method::GET && origin.is_some_and(|o| o != format!("http://{host}")) {
            return json_status(StatusCode::FORBIDDEN, json!({"error": "not from this page"}));
        }
        let method = request.method().clone();
        let segments: Vec<String> = api.split('/').filter(|s| !s.is_empty()).map(decode).collect();
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(e) => return json_status(StatusCode::PAYLOAD_TOO_LARGE, json!({"error": e.to_string()})),
        };
        let lib = self.lib.clone();
        // The library blocks (files, SQLite, lookups online), so off the async threads.
        let reply = tokio::task::spawn_blocking(move || {
            let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
            api_call(&lib, &method, &segments, &query, body)
        })
        .await;
        match reply {
            Ok(Ok(Reply::Json(value))) => json_status(StatusCode::OK, value),
            Ok(Ok(Reply::File(path))) => file(&path),
            Ok(Ok(Reply::Text(content))) => {
                let mut response = text(StatusCode::OK, content);
                response.headers_mut().insert(header::CONTENT_TYPE, "text/markdown; charset=utf-8".parse().unwrap());
                response
            }
            Ok(Ok(Reply::Download(name, content))) => {
                let mut response = text(StatusCode::OK, content);
                let disposition = format!("attachment; filename=\"{}\"", name.replace(['"', '\\'], "_"));
                response.headers_mut().insert(header::CONTENT_DISPOSITION, disposition.parse().unwrap());
                response
            }
            Ok(Err(e)) if e.is::<NotFound>() => json_status(StatusCode::NOT_FOUND, json!({"error": format!("{e:#}")})),
            Ok(Err(e)) => json_status(StatusCode::BAD_REQUEST, json!({"error": format!("{e:#}")})),
            Err(e) => json_status(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e.to_string()})),
        }
    }

    fn logged_in<B>(&self, request: &Request<B>) -> bool {
        let cookies = request.headers().get_all(header::COOKIE).iter().filter_map(|c| c.to_str().ok());
        let name = self.cookie();
        cookies
            .flat_map(|c| c.split(';'))
            .any(|c| c.trim().split_once('=').is_some_and(|(n, value)| n == name && same(value, &self.token)))
    }
}

/// Compares every byte, so the time taken doesn't tell how much of a guess was right.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[derive(Debug)]
struct NotFound;

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("no such API call")
    }
}

impl std::error::Error for NotFound {}

enum Reply {
    Json(Value),
    File(PathBuf),
    Text(String),
    /// A file to save, with its name.
    Download(String, String),
}

fn ok(value: Value) -> Result<Reply> {
    Ok(Reply::Json(value))
}

/// The API. Paths are the segments after `/api/`.
fn api_call(
    lib: &Library,
    method: &Method,
    path: &[&str],
    query: &HashMap<String, String>,
    body: Bytes,
) -> Result<Reply> {
    let form = || -> Result<Value> { serde_json::from_slice(&body).context("expected JSON") };
    let field = |name: &str| -> Result<String> {
        let value = form()?;
        value[name].as_str().map(String::from).with_context(|| format!("expected {{\"{name}\": \"...\"}}"))
    };
    let (get, post) = (Method::GET, Method::POST);
    match (method, path) {
        (m, ["info"]) if *m == get => ok(info(lib)),
        (m, ["papers"]) if *m == get => {
            let papers = match query.get("project").filter(|p| !p.is_empty()) {
                Some(project) => lib.project_papers(project)?.0,
                None => lib.papers().0,
            };
            ok(Value::Array(papers.iter().map(summary).collect()))
        }
        (m, ["search"]) if *m == get => {
            let q = query.get("q").map(String::as_str).unwrap_or_default();
            let limit = query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50);
            let hits = match query.get("semantic").is_some_and(|s| s == "1" || s == "true") {
                true => {
                    let config = lib.embeddings.as_ref().context("semantic search needs [embeddings] in the config")?;
                    lib.semantic_search(&carrel::embed::Ollama::connect(config)?, q, limit)?
                }
                false => lib.search(q, limit)?,
            };
            let hits = hits.iter().map(|(p, snippet)| {
                let mut s = summary(p);
                s["snippet"] = json!(snippet);
                s
            });
            ok(Value::Array(hits.collect()))
        }
        (m, ["add"]) if *m == post => {
            let (id, messages) = lib.add(&field("source")?)?;
            ok(json!({"id": id, "messages": messages}))
        }
        // The PDF itself as the body, its file name in `?name=`.
        (m, ["upload"]) if *m == post => {
            let upload = Upload::new(query.get("name").map(String::as_str).unwrap_or("upload.pdf"), &body)?;
            let (id, messages) = lib.add(&upload.path.to_string_lossy())?;
            ok(json!({"id": id, "messages": messages}))
        }
        (m, ["drafts"]) if *m == get => {
            let drafts = lib.drafts().into_iter().map(|(p, f)| {
                json!({"id": p.id, "title": p.bib.title(), "field": f, "draft": p.draft(f), "current": p.get(f)})
            });
            ok(Value::Array(drafts.collect()))
        }
        (m, ["projects"]) if *m == get => {
            let projects = lib.projects().into_iter().map(|name| {
                let papers = lib.project(&name).unwrap_or_default();
                json!({"name": name, "papers": papers})
            });
            ok(Value::Array(projects.collect()))
        }
        (m, ["suggestions"]) if *m == get => {
            let within = query.get("project").map(|p| lib.project(p)).transpose()?;
            let mut suggestions = lib.graph()?.suggestions(within.as_deref());
            suggestions.truncate(query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50));
            ok(json!(suggestions))
        }
        (m, ["inbox"]) if *m == get => ok(inbox()?),
        (m, ["map"]) if *m == get => ok(map(lib, query.get("project").filter(|p| !p.is_empty()))?),
        (m, ["doctor"]) if *m == get => ok(json!(lib.doctor())),
        // A project's (or the library's) BibTeX, to save.
        (m, ["bib"]) if *m == get => {
            let project = query.get("project").filter(|p| !p.is_empty());
            let papers = match project {
                Some(name) => lib.project_papers(name)?.0,
                None => lib.papers().0,
            };
            let bib: String = papers.iter().map(|p| p.bibtex().trim_end().to_string() + "\n\n").collect();
            Ok(Reply::Download(format!("{}.bib", project.map_or("carrel", String::as_str)), bib))
        }
        (m, ["people"]) if *m == get => {
            let graph = lib.graph()?;
            let people = graph.authors.people(query.get("q").map(String::as_str).filter(|q| !q.is_empty()));
            ok(json!(people))
        }
        (m, ["people", "drafts"]) if *m == get => {
            let drafts = lib
                .people_drafts()
                .into_iter()
                .map(|d| json!({"id": d.id, "same": d.same, "spellings": d.spellings, "reason": d.reason}));
            ok(Value::Array(drafts.collect()))
        }
        (m, ["people", "drafts", draft, action @ ("approve" | "reject")]) if *m == post => {
            lib.decide_people(draft, *action == "approve")?;
            ok(json!({}))
        }
        // Confirm that spellings are one person, or different people.
        (m, ["people", how @ ("same" | "different")]) if *m == post => {
            let spellings: Vec<String> =
                serde_json::from_value(form()?["spellings"].clone()).context("expected {\"spellings\": [...]}")?;
            match *how {
                "same" => ok(json!({"id": lib.same_person(&spellings)?})),
                _ => ok(json!({"ids": lib.different_people(&spellings)?})),
            }
        }
        (m, ["inbox", action @ ("add" | "skip")]) if *m == post => {
            let path = PathBuf::from(field("path")?);
            // Only files the inbox lists: the API never reads or deletes any other path.
            let config = Config::load()?.consume.unwrap_or_default();
            let found = consume::find(&config.folders(), config.days());
            let f = found.iter().find(|f| f.path == path).context("that file isn't in the inbox anymore")?;
            if *action == "skip" {
                let mut seen = consume::Seen::load();
                seen.set(f, consume::Mark::Skipped);
                seen.save()?;
                return ok(json!({}));
            }
            let (id, messages) = lib.consume(f, false)?;
            ok(json!({"id": id, "messages": messages}))
        }
        (m, ["sync"]) if *m == post => {
            let remote = Config::load()?.remote.context("no remote in the config; see Sharing a library")?;
            let report = lib.sync(&*remote.open()?, None, false, |_, _| {})?;
            let failed: Vec<String> = report.failed.iter().map(|(path, e)| format!("{path}: {e}")).collect();
            ok(json!({"summary": report.summary(), "failed": failed}))
        }
        (_, ["papers", id, rest @ ..]) => paper_call(lib, method, id, rest, query, &body, form),
        _ => Err(NotFound.into()),
    }
}

fn paper_call(
    lib: &Library,
    method: &Method,
    id: &str,
    rest: &[&str],
    query: &HashMap<String, String>,
    body: &Bytes,
    form: impl Fn() -> Result<Value>,
) -> Result<Reply> {
    let p = lib.get(id)?;
    let (get, post, put, delete) = (Method::GET, Method::POST, Method::PUT, Method::DELETE);
    match (method, rest) {
        (m, []) if *m == get => ok(details(lib, &p)),
        (m, []) if *m == delete => {
            lib.remove(id)?;
            ok(json!({}))
        }
        (m, ["pdf"]) if *m == get => Ok(Reply::File(p.pdf().context(NotFound)?)),
        // The body is the PDF; empty, it is downloaded from arXiv.
        (m, ["pdf"]) if *m == post => {
            let upload = (!body.is_empty()).then(|| Upload::new("paper.pdf", body)).transpose()?;
            let messages = lib.attach(id, upload.as_ref().map(|u| u.path.as_path()))?;
            ok(json!({"messages": messages}))
        }
        (m, ["fulltext"]) if *m == get => Ok(Reply::Text(p.fulltext().context(NotFound)?)),
        (m, ["files", folder @ ("figures" | "supplementary"), name]) if *m == get => {
            if name.starts_with('.') || name.contains(['/', '\\']) {
                bail!("not a file name: {name:?}");
            }
            let path = p.dir.join(folder).join(name);
            Ok(Reply::File(Some(path).filter(|p| p.is_file()).context(NotFound)?))
        }
        // Supplementary material: the file as the body, its name in `?name=`.
        (m, ["supplementary"]) if *m == post => {
            let upload = Upload::new(query.get("name").map(String::as_str).unwrap_or("supplementary"), body)?;
            let messages = lib.add_supplementary(id, &[upload.path.to_string_lossy().into_owned()])?;
            ok(json!({"messages": messages}))
        }
        // Your own notes, written as they are: no draft, since you are the human.
        (m, ["notes"]) if *m == put => {
            let notes = String::from_utf8(body.to_vec()).context("notes must be UTF-8")?;
            let path = p.dir.join(Field::Notes.file());
            match notes.trim().is_empty() {
                true => drop(fs::remove_file(path)),
                false => fs::write(path, notes)?,
            }
            ok(details(lib, &lib.get(id)?))
        }
        (m, ["update"]) if *m == post => {
            let messages = lib.update(id, None)?;
            ok(json!({"messages": messages}))
        }
        (m, ["extract"]) if *m == post => {
            lib.extract(id)?;
            ok(json!({}))
        }
        (m, ["references"]) if *m == get => {
            let refs = lib.references(id, query.contains_key("refresh"))?.into_iter().map(|(r, found)| {
                let mut r = json!(r);
                r["in_library"] = json!(found);
                r
            });
            ok(Value::Array(refs.collect()))
        }
        (m, ["cited-by"]) if *m == get => {
            let found = lib.related(id, true)?.into_iter().map(|c| {
                let source = c.ids.source();
                json!({"title": c.title, "authors": c.authors, "year": c.year, "venue": c.venue,
                       "doi": c.ids.doi, "arxiv": c.ids.arxiv, "in_library": c.in_library, "add_with": source})
            });
            ok(Value::Array(found.collect()))
        }
        (m, ["neighbors"]) if *m == get => ok(json!(lib.graph()?.neighbors(id)?)),
        // The library papers it cites (or with `cited_by`, that cite it), and theirs in turn.
        (m, ["tree"]) if *m == get => {
            let depth = query.get("depth").and_then(|d| d.parse().ok()).unwrap_or(3);
            let tree = lib.graph()?.tree(id, depth, query.contains_key("cited_by"))?;
            let tree = tree.into_iter().map(|(level, id, again)| json!({"level": level, "id": id, "again": again}));
            ok(Value::Array(tree.collect()))
        }
        (m, ["similar"]) if *m == get => {
            let similar = lib.similar(12)?.remove(id).unwrap_or_default();
            ok(Value::Array(similar.into_iter().map(|(id, score)| json!({"id": id, "score": score})).collect()))
        }
        (m, ["drafts", field, action @ ("approve" | "reject")]) if *m == post => {
            let field: Field = serde_json::from_value(json!(field)).context("no such field")?;
            match *action {
                "approve" => lib.approve(id, field)?,
                _ => lib.reject(id, field)?,
            }
            ok(details(lib, &lib.get(id)?))
        }
        (m, ["projects", name]) if *m == put => {
            lib.add_to_project(name, &[id.to_string()])?;
            ok(json!({"projects": lib.projects_of(id)}))
        }
        (m, ["projects", name]) if *m == delete => {
            lib.remove_from_project(name, &[id.to_string()])?;
            ok(json!({"projects": lib.projects_of(id)}))
        }
        (m, ["keywords"]) if *m == put => {
            let keywords: Vec<String> =
                serde_json::from_value(form()?["keywords"].clone()).context("expected {\"keywords\": [...]}")?;
            let mut p = p;
            let keywords: Vec<&str> = keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()).collect();
            p.bib.set("keywords", Some(&keywords.join(", ")).filter(|k| !k.is_empty()).map(String::as_str));
            p.save()?;
            ok(details(lib, &p))
        }
        _ => Err(NotFound.into()),
    }
}

/// What the sidebar needs: where the library is, and what works.
fn info(lib: &Library) -> Value {
    let config = Config::load().unwrap_or_default();
    let embeddings = lib.embeddings.as_ref().map(|c| match carrel::embed::Ollama::connect(c) {
        Ok(o) => json!({"model": o.model, "url": o.url}),
        Err(e) => json!({"error": format!("{e:#}")}),
    });
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "library": lib.root,
        "remote": config.remote.is_some(),
        "embeddings": embeddings,
        "drafts": lib.drafts().len() + lib.people_drafts().len(),
        "ui": !assets::FILES.is_empty(),
    })
}

/// A paper for lists.
fn summary(p: &Paper) -> Value {
    let b = &p.bib;
    let added = fs::metadata(&p.dir).and_then(|m| m.created().or_else(|_| m.modified())).ok();
    json!({
        "id": p.id,
        "title": b.title(),
        "authors": b.authors(),
        "year": b.year(),
        "venue": b.venue(),
        "keywords": b.keywords(),
        "pdf": p.pdf().is_some(),
        "text": p.dir.join("fulltext.md").exists(),
        "drafts": p.drafts(),
        "added": added.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()),
    })
}

/// A paper with everything the paper page shows.
fn details(lib: &Library, p: &Paper) -> Value {
    let mut d = crate::mcp::details(lib, p);
    let drafts: serde_json::Map<String, Value> =
        p.drafts().into_iter().map(|f| (f.name().to_string(), json!(p.draft(f)))).collect();
    d["drafts"] = Value::Object(drafts);
    d["bibtex"] = json!(p.bibtex());
    d["pdf"] = json!(p.pdf().is_some());
    d["text"] = json!(p.dir.join("fulltext.md").exists());
    d["url"] = json!(p.bib.get("url"));
    d.as_object_mut().map(|o| o.remove("files"));
    d
}

/// The library as a network: papers, the citations between them, the authors they share and, from the
/// embeddings, which are most alike.
fn map(lib: &Library, project: Option<&String>) -> Result<Value> {
    let papers = match project {
        Some(name) => lib.project_papers(name)?.0,
        None => lib.papers().0,
    };
    let shown: std::collections::HashSet<&str> = papers.iter().map(|p| p.id.as_str()).collect();
    let graph = lib.graph()?;
    let nodes = papers.iter().map(|p| {
        json!({
            "id": p.id,
            "title": p.bib.title(),
            "authors": p.short_authors(),
            "year": p.bib.year(),
            "venue": p.bib.venue(),
            "keywords": p.bib.keywords(),
            "projects": lib.projects_of(&p.id),
        })
    });
    let cites = graph.citations().into_iter().filter(|(a, b)| shown.contains(a) && shown.contains(b));
    let mut authors = vec![];
    for p in &papers {
        for (other, names) in graph.authors.shared(&p.id) {
            if p.id < other && shown.contains(other.as_str()) {
                authors.push(json!([p.id, other, names]));
            }
        }
    }
    // Each paper's three nearest, once per pair.
    let mut similar: Vec<(String, String, f32)> = vec![];
    for (id, near) in lib.similar(3)?.into_iter().filter(|(id, _)| shown.contains(id.as_str())) {
        for (other, score) in near.into_iter().filter(|(other, _)| shown.contains(other.as_str())) {
            let (a, b) = if id < other { (id.clone(), other) } else { (other, id.clone()) };
            if !similar.iter().any(|(x, y, _)| *x == a && *y == b) {
                similar.push((a, b, score));
            }
        }
    }
    Ok(json!({
        "nodes": nodes.collect::<Vec<_>>(),
        "cites": cites.map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        "authors": authors,
        "similar": similar.into_iter().map(|(a, b, s)| json!([a, b, s])).collect::<Vec<_>>(),
    }))
}

/// The PDFs `carrel consume` would offer, with what became of each so far.
fn inbox() -> Result<Value> {
    let config = Config::load()?.consume.unwrap_or_default();
    let seen = consume::Seen::load();
    let found = consume::find(&config.folders(), config.days()).into_iter().rev().map(|f| {
        json!({
            "path": f.path,
            "name": f.path.file_name().map(|n| n.to_string_lossy()),
            "size": f.size,
            "modified": f.modified,
            "clue": f.clue,
            "skipped": seen.get(&f) == Some(consume::Mark::Skipped),
        })
    });
    let folders: Vec<PathBuf> = config.folders();
    Ok(json!({"folders": folders, "days": config.days(), "files": found.collect::<Vec<_>>()}))
}

/// A file sent by the browser, in a folder of its own that goes away with it.
struct Upload {
    dir: PathBuf,
    path: PathBuf,
}

impl Upload {
    fn new(name: &str, bytes: &[u8]) -> Result<Self> {
        let name = Path::new(name).file_name().context("no file name")?.to_string_lossy().into_owned();
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("carrel-upload-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        fs::write(&path, bytes)?;
        Ok(Self { dir, path })
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

/// Percent-decoding, with `+` as a space as forms send it.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn text(status: StatusCode, body: impl Into<Bytes>) -> Response<Body> {
    let mut response = Response::new(Full::new(body.into()).boxed());
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, "text/plain; charset=utf-8".parse().unwrap());
    response
}

fn json_status(status: StatusCode, value: Value) -> Response<Body> {
    let mut response = text(status, serde_json::to_vec(&value).unwrap_or_default());
    response.headers_mut().insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

fn redirect(to: &str) -> Response<Body> {
    let mut response = text(StatusCode::SEE_OTHER, "");
    response.headers_mut().insert(header::LOCATION, to.parse().unwrap_or_else(|_| "/".parse().unwrap()));
    response
}

fn file(path: &Path) -> Response<Body> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut response = text(StatusCode::OK, bytes);
            response.headers_mut().insert(header::CONTENT_TYPE, mime(path).parse().unwrap());
            response.headers_mut().insert(header::CONTENT_DISPOSITION, "inline".parse().unwrap());
            response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
            // Supplementary files come from anywhere: an SVG or HTML one must not run scripts as this page.
            // PDFs are spared, as Chrome won't show them sandboxed.
            if mime(path) != "application/pdf" {
                response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, "sandbox".parse().unwrap());
            }
            response
        }
        Err(e) => json_status(StatusCode::NOT_FOUND, json!({"error": e.to_string()})),
    }
}

/// A built-in file of the UI; anything else gets the app, which routes in the browser.
fn asset(path: &str) -> Response<Body> {
    let name = path.trim_start_matches('/');
    let found = assets::FILES.iter().find(|(n, _)| *n == name);
    let Some((name, bytes)) = found.or_else(|| assets::FILES.iter().find(|(n, _)| *n == "index.html")) else {
        return text(
            StatusCode::NOT_FOUND,
            "This carrel was built without the web UI. Build it with `npm ci && npm run build` in web/, then rebuild carrel.\n",
        );
    };
    let mut response = text(StatusCode::OK, Bytes::from_static(bytes));
    response.headers_mut().insert(header::CONTENT_TYPE, mime(Path::new(name)).parse().unwrap());
    // Vite names built files by their content, so they never change; index.html must be fresh.
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    response.headers_mut().insert(header::CACHE_CONTROL, cache.parse().unwrap());
    response
}

fn mime(path: &Path) -> &'static str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        "md" => "text/markdown; charset=utf-8",
        "txt" | "csv" | "tsv" | "dat" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(decode("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(
            query("q=space+weathering&semantic=1&x"),
            HashMap::from([
                ("q".into(), "space weathering".into()),
                ("semantic".into(), "1".into()),
                ("x".into(), String::new()),
            ])
        );
    }
}
