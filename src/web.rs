//! `carrel web`: the library in your browser, for this machine only. It serves the web UI (built from
//! `web/` and embedded at compile time) and the JSON API under `/api` that the UI uses.
//!
//! Only loopback, and still guarded: any website you visit can make your browser send requests to
//! localhost. So requests must name localhost in `Host` (against DNS rebinding) and carry a cookie that
//! only the link `carrel web` prints sets (against other sites and other users on the machine).
//!
//! `carrel serve` serves the same UI to a group ([`Web::server`]). There the cookie alone keeps strangers
//! out, from trusted networks too: browsers send it only to the server's own name, so a rebound name
//! doesn't get it either.

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use carrel::{Config, Field, Library, Paper, consume};
use http_body_util::{BodyExt, Full, Limited, combinators::BoxBody};
use hyper::{Method, Request, Response, StatusCode, header};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    let path = carrel::state_dir().context("no folder to keep the web UI's token in")?.join("web-token");
    Ok(link(addr.port(), &token(&path)?))
}

pub fn run(lib: Library, listen: Option<String>, open: bool) -> Result<()> {
    let addr = address(listen)?;
    let token = token(&carrel::state_dir().context("no folder to keep the web UI's token in")?.join("web-token"))?;
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

/// The secret in the link, kept in `path` across restarts so a logged-in browser stays logged in.
pub fn token(path: &Path) -> Result<String> {
    if let Ok(token) = fs::read_to_string(path)
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
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    std::io::Write::write_all(&mut file, token.as_bytes())?;
    Ok(token)
}

pub async fn serve(lib: Library, listener: tokio::net::TcpListener, token: String) -> Result<()> {
    let port = listener.local_addr()?.port();
    let web = Arc::new(Web {
        lib,
        token,
        port,
        local: true,
        auth: None,
        site: None,
        hashes: Default::default(),
        history: None,
        writes: Default::default(),
    });
    loop {
        let (stream, _) = listener.accept().await?;
        let web = web.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let web = web.clone();
                async move { Ok::<_, Infallible>(web.handle(request, None, false).await) }
            });
            let io = hyper_util::rt::TokioIo::new(stream);
            let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
        });
    }
}

pub struct Web {
    lib: Library,
    token: String,
    port: u16,
    /// `carrel web`, answering only to localhost; not `carrel serve`'s.
    local: bool,
    /// `carrel serve`'s logins, tokens and roles.
    auth: Option<Arc<crate::auth::Auth>>,
    /// The server's name as browsers reach it, the only one it answers to when known.
    site: Option<String>,
    /// MD5s of the library's files for `/sync`.
    hashes: Hashes,
    /// A server's record of changes and deletions; changes go one at a time, so each is recorded alone.
    history: Option<Arc<crate::history::History>>,
    writes: Arc<std::sync::Mutex<()>>,
}

/// MD5s by path, with the size and modification time they were taken at.
type Hashes = Arc<std::sync::Mutex<HashMap<String, (u64, u128, String)>>>;

impl Web {
    /// For `carrel serve`, which hands it every request but MCP's. `url` is where browsers reach it.
    pub fn server(
        lib: Library,
        token: String,
        port: u16,
        auth: Arc<crate::auth::Auth>,
        url: Option<&str>,
        history: Arc<crate::history::History>,
    ) -> Self {
        let site = url.map(|u| u.split_once("://").map_or(u, |(_, rest)| rest).trim_end_matches('/').to_string());
        let (hashes, writes) = (Default::default(), Default::default());
        Self { lib, token, port, local: false, auth: Some(auth), site, hashes, history: Some(history), writes }
    }

    /// Whether a bearer token is a member's, for MCP.
    pub fn member<B>(&self, request: &Request<B>, from: Option<std::net::IpAddr>) -> bool {
        use crate::auth::Role;
        self.auth.as_ref().is_some_and(|a| a.who(request.headers(), false, from).role >= Role::Member)
    }

    /// With logins, the link's token is off: people log in instead.
    fn link_works(&self) -> bool {
        self.local || !self.auth.as_ref().is_some_and(|a| a.login())
    }

    /// Who sent a request, from a trusted network or not, with the name its client claims.
    fn who<B>(&self, request: &Request<B>, trusted: bool, from: Option<std::net::IpAddr>) -> crate::auth::Who {
        let client = request.headers().get("x-carrel-client").and_then(|v| v.to_str().ok());
        let client = client.map(|c| c.chars().filter(|c| !c.is_control()).take(64).collect::<String>());
        let mut who = self.who_alone(request, trusted, from);
        who.client = client.filter(|c| !c.is_empty());
        who
    }

    fn who_alone<B>(&self, request: &Request<B>, trusted: bool, from: Option<std::net::IpAddr>) -> crate::auth::Who {
        use crate::auth::{Role, Who};
        if self.link_works() && self.logged_in(request) {
            // Whoever opened the link; on a server, one more admin.
            return if self.local { Who::role(Role::Admin) } else { Who::named("link", Role::Admin) };
        }
        match &self.auth {
            Some(auth) => auth.who(request.headers(), trusted, from),
            None => Who::role(Role::None),
        }
    }

    /// The `/auth/` pages of a server with logins.
    async fn auth_page(
        &self,
        request: &Request<hyper::body::Incoming>,
        query: &HashMap<String, String>,
    ) -> Response<Body> {
        let auth = self.auth.clone().expect("only with logins");
        let path = request.uri().path();
        if path == "/auth/logout" && request.method() == Method::POST {
            let _ = auth.logout(request.headers());
            let mut response = json_status(StatusCode::OK, json!({}));
            response.headers_mut().insert(header::SET_COOKIE, auth.cookie("").parse().unwrap());
            return response;
        }
        if let Some(error) = query.get("error") {
            let why = query.get("error_description").unwrap_or(error);
            return text(StatusCode::FORBIDDEN, format!("the login was refused: {why}\n"));
        }
        let (code, state) = (query.get("code").cloned(), query.get("state").cloned());
        let next = query.get("next").cloned().unwrap_or_else(|| "/".into());
        let login = path == "/auth/login";
        // The provider is asked over the network, which blocks.
        let reply = tokio::task::spawn_blocking(move || -> Result<(Option<String>, String)> {
            match (login, code, state) {
                (true, ..) => Ok((None, auth.login_url(&next)?)),
                (false, Some(code), Some(state)) => auth.callback(&code, &state).map(|(id, next)| (Some(id), next)),
                _ => bail!("not a login page"),
            }
        })
        .await;
        match reply {
            Ok(Ok((session, to))) => {
                let mut response = redirect(&to);
                if let Some(id) = session {
                    let cookie = self.auth.as_ref().unwrap().cookie(&id);
                    response.headers_mut().insert(header::SET_COOKIE, cookie.parse().unwrap());
                }
                response
            }
            Ok(Err(e)) => text(StatusCode::BAD_REQUEST, format!("cannot log in: {e:#}\n")),
            Err(e) => text(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\n")),
        }
    }

    fn where_link(&self) -> &'static str {
        if self.local { "open the link `carrel web` printed" } else { "open the link in the server's log" }
    }

    fn cookie(&self) -> String {
        format!("carrel-{}", self.port)
    }

    /// The library as a remote for `carrel sync` (`type = "carrel"`): `GET /sync` lists the files with their MD5s
    /// and what the caller may do; `GET`, `PUT` and `DELETE /sync/<path>` move one. Adding a file needs the
    /// right to add, overwriting one to edit, deleting one an admin. `If-Match` / `If-None-Match: *` say which
    /// version an upload replaces, so two clients can't overwrite each other unseen.
    async fn sync_call(
        &self,
        request: Request<hyper::body::Incoming>,
        from: Option<std::net::IpAddr>,
        trusted: bool,
    ) -> Response<Body> {
        use crate::auth::{Right, Role};
        let who = self.who(&request, trusted, from);
        let role = who.role;
        if role == Role::None {
            return json_status(StatusCode::UNAUTHORIZED, json!({"error": "send a token"}));
        }
        let method = request.method().clone();
        let path = request.uri().path().strip_prefix("/sync/").map(decode);
        let header =
            |name| request.headers().get(name).and_then(|v| v.to_str().ok()).map(|v| v.trim_matches('"').to_string());
        let (if_match, if_none) = (header(header::IF_MATCH), header(header::IF_NONE_MATCH).is_some_and(|v| v == "*"));
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(e) => return json_status(StatusCode::PAYLOAD_TOO_LARGE, json!({"error": e.to_string()})),
        };
        let (root, hashes) = (self.lib.root.clone(), self.hashes.clone());
        let (history, writes) = (self.history.clone().expect("a server's"), self.writes.clone());
        let name = who.user.clone().unwrap_or_else(|| "anonymous".into());
        let client = who.client.clone();
        // Where this person's `private/` is kept: `users/<name>/`, for the people the server knows.
        let own = who.user.clone().filter(|u| role.can(Right::Edit) && carrel::valid_id(u));
        let own = own.map(|u| format!("{}/{u}/", carrel::projects::USERS));
        let md5 = move |path: &str| -> Result<Option<String>> {
            let file = root.join(path);
            let Ok(meta) = fs::metadata(&file) else { return Ok(None) };
            let stamp = meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
            if let Some((size, at, md5)) = hashes.lock().unwrap().get(path)
                && (*size, *at) == (meta.len(), stamp)
            {
                return Ok(Some(md5.clone()));
            }
            let md5 = carrel::remote::md5_hex(&fs::read(&file)?);
            hashes.lock().unwrap().insert(path.into(), (meta.len(), stamp, md5.clone()));
            Ok(Some(md5))
        };
        let root = self.lib.root.clone();
        let reply = tokio::task::spawn_blocking(move || -> Result<Response<Body>> {
            let refused = |status, why: &str| Ok(json_status(status, json!({"error": why})));
            let users = format!("{}/", carrel::projects::USERS);
            let Some(path) = path else {
                let mut files = serde_json::Map::new();
                for path in carrel::remote::files(&root)? {
                    let shown = match &own {
                        _ if !path.starts_with(&users) => path.clone(),
                        Some(own) if path.starts_with(own.as_str()) => {
                            format!("{}/{}", carrel::projects::PRIVATE, &path[own.len()..])
                        }
                        _ => continue, // someone else's
                    };
                    if let Some(md5) = md5(&path)? {
                        files.insert(shown, md5.into());
                    }
                }
                let can =
                    json!({"edit": role.can(Right::Edit), "delete": role.can(Right::Edit), "private": own.is_some()});
                return Ok(json_status(StatusCode::OK, json!({"files": files, "can": can})));
            };
            if !carrel::remote::plain(&path) || path.starts_with(&users) {
                return refused(StatusCode::BAD_REQUEST, "not a path in the library");
            }
            // A private file: the person's own, kept out of the activity, theirs to change and delete.
            let private = path.strip_prefix(&format!("{}/", carrel::projects::PRIVATE)).map(String::from);
            let path = match (&private, &own) {
                (Some(rest), Some(own)) => format!("{own}{rest}"),
                (Some(_), None) => return refused(StatusCode::FORBIDDEN, "private files need a member's token"),
                (None, _) => path,
            };
            let private = private.is_some();
            let file = root.join(&path);
            let current = md5(&path)?;
            match method {
                Method::GET if current.is_some() => Ok(self::file(&file)),
                Method::GET => refused(StatusCode::NOT_FOUND, "no such file"),
                Method::PUT => {
                    let right = if current.is_some() { Right::Edit } else { Right::Add };
                    if !private && !role.can(right) {
                        return refused(StatusCode::FORBIDDEN, "changing files needs a member's token");
                    }
                    let sent = carrel::remote::md5_hex(&body);
                    // Whoever got there first wins; the same bytes again are no change at all.
                    let stale = match (&current, &if_match) {
                        (Some(have), Some(expected)) => have != expected,
                        (Some(_), None) => if_none,
                        (None, Some(_)) => true,
                        (None, None) => false,
                    };
                    if stale && current.as_ref() != Some(&sent) {
                        return refused(StatusCode::PRECONDITION_FAILED, "changed on the server meanwhile");
                    }
                    let parts = root.join(".carrel").join("parts");
                    fs::create_dir_all(&parts)?;
                    let part = parts.join(format!("upload-{}", crate::auth::random(8)));
                    fs::write(&part, &body)?;
                    let _one_at_a_time = writes.lock().unwrap_or_else(|e| e.into_inner());
                    fs::create_dir_all(file.parent().unwrap())?;
                    if private {
                        fs::rename(&part, &file)?;
                    } else {
                        let paper = path.split_once('/').map(|(p, _)| p);
                        let before = history.before(&name, client.as_deref(), "synced", paper, &[&path])?;
                        fs::rename(&part, &file)?;
                        history.after(before)?;
                    }
                    let mut response = json_status(StatusCode::OK, json!({}));
                    response.headers_mut().insert(header::ETAG, format!("\"{sent}\"").parse()?);
                    Ok(response)
                }
                // Deleting is asked for, and an admin other than who asks decides.
                Method::DELETE => {
                    if !role.can(Right::Edit) {
                        return refused(StatusCode::FORBIDDEN, "deleting needs a member's token");
                    }
                    if current.is_none() {
                        return Ok(json_status(StatusCode::OK, json!({})));
                    }
                    if private {
                        fs::remove_file(&file)?;
                        carrel::remote::remove_empty_parents(&file, &root);
                        return Ok(json_status(StatusCode::OK, json!({})));
                    }
                    let paper = path.split_once('/').map(|(p, _)| p);
                    if history.request_deletion(&name, paper, std::slice::from_ref(&path))? {
                        return refused(StatusCode::CONFLICT, "an admin turned this deletion down");
                    }
                    Ok(json_status(StatusCode::ACCEPTED, json!({"asked": true})))
                }
                _ => refused(StatusCode::METHOD_NOT_ALLOWED, "GET, PUT or DELETE"),
            }
        })
        .await;
        match reply {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => json_status(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": format!("{e:#}")})),
            Err(e) => json_status(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e.to_string()})),
        }
    }

    /// Ollama's API, as far as clients embed with it, by the server's Ollama and model: everyone's vectors
    /// then compare, and nobody needs an Ollama of their own.
    async fn ollama_call(
        &self,
        request: Request<hyper::body::Incoming>,
        from: Option<std::net::IpAddr>,
        trusted: bool,
    ) -> Response<Body> {
        if self.who(&request, trusted, from).role == crate::auth::Role::None {
            return json_status(StatusCode::UNAUTHORIZED, json!({"error": "send a token"}));
        }
        let Some(config) = self.lib.embeddings.clone() else {
            let why = "this carrel server doesn't compute embeddings; [embeddings] in its config turns them on";
            return json_status(StatusCode::NOT_FOUND, json!({"error": why}));
        };
        let endpoint = request.uri().path().trim_start_matches("/ollama/api/").to_string();
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(e) => return json_status(StatusCode::PAYLOAD_TOO_LARGE, json!({"error": e.to_string()})),
        };
        let reply = tokio::task::spawn_blocking(move || -> Result<Value> {
            let ollama = carrel::embed::Ollama::connect(&config)?;
            let mut sent: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body)? };
            match endpoint.as_str() {
                "tags" => Ok(json!({"models": [{"name": ollama.model, "model": ollama.model}]})),
                "show" => ollama.call("show", Some(sent)),
                "embed" => {
                    sent["model"] = json!(ollama.model);
                    ollama.call("embed", Some(sent))
                }
                _ => bail!("carrel passes on only Ollama's tags, show and embed"),
            }
        })
        .await;
        match reply {
            Ok(Ok(value)) => json_status(StatusCode::OK, value),
            Ok(Err(e)) => json_status(StatusCode::BAD_GATEWAY, json!({"error": format!("{e:#}")})),
            Err(e) => json_status(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e.to_string()})),
        }
    }

    /// `trusted`: from one of a server's trusted networks.
    pub async fn handle(
        &self,
        request: Request<hyper::body::Incoming>,
        from: Option<std::net::IpAddr>,
        trusted: bool,
    ) -> Response<Body> {
        let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default();
        let local = ["127.0.0.1", "localhost", "[::1]"].iter().any(|name| host == format!("{name}:{}", self.port));
        if self.local && !local {
            return text(StatusCode::FORBIDDEN, "carrel web only answers to localhost\n");
        }
        // A server lets trusted networks in without a cookie, so a name pointed at it (DNS rebinding) must not
        // pass for it.
        if let Some(site) = &self.site
            && host != site
        {
            return text(StatusCode::FORBIDDEN, format!("this server answers to {site}\n"));
        }
        let path = request.uri().path().to_string();
        let query = query(request.uri().query().unwrap_or_default());
        if path.starts_with("/auth/") && self.auth.as_ref().is_some_and(|a| a.login()) {
            return self.auth_page(&request, &query).await;
        }
        if !self.local && (path == "/sync" || path.starts_with("/sync/")) {
            return self.sync_call(request, from, trusted).await;
        }
        if !self.local && path.starts_with("/ollama/api/") {
            return self.ollama_call(request, from, trusted).await;
        }
        // The link `carrel web` prints: trade the token for a cookie, and drop it from the address bar.
        if let Some(sent) = query.get("token").filter(|_| self.link_works()) {
            if !same(sent, &self.token) {
                return text(StatusCode::FORBIDDEN, format!("that link is out of date; {}\n", self.where_link()));
            }
            let mut response = redirect(&path);
            let cookie =
                format!("{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000", self.cookie(), self.token);
            response.headers_mut().insert(header::SET_COOKIE, cookie.parse().expect("a valid header"));
            return response;
        }
        let Some(api) = path.strip_prefix("/api/") else {
            // Paper texts, notes and AI drafts are shown as HTML: should one slip past the sanitizer, it
            // still can't run scripts, post forms or frame the page. PDFs come from /api, without this.
            let mut response = asset(&path);
            let csp =
                "script-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
            response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, csp.parse().unwrap());
            return response;
        };
        use crate::auth::{Right, Role};
        let who = self.who(&request, trusted, from);
        let method = request.method().clone();
        let segments: Vec<String> = api.split('/').filter(|s| !s.is_empty()).map(decode).collect();
        let segments_ref: Vec<&str> = segments.iter().map(String::as_str).collect();
        if segments_ref == ["me"] {
            let can = |right| who.role.can(right);
            let login = self.auth.as_ref().is_some_and(|a| a.login());
            return json_status(
                StatusCode::OK,
                json!({
                    "user": who.user, "name": who.name, "role": who.role, "login": login, "server": !self.local,
                    "can": {"read": can(Right::Read), "add": can(Right::Add), "edit": can(Right::Edit), "admin": can(Right::Admin)},
                }),
            );
        }
        if !who.role.can(crate::auth::needed(method.as_str(), &segments_ref)) {
            return match who.role {
                Role::None if self.auth.as_ref().is_some_and(|a| a.login()) => {
                    json_status(StatusCode::UNAUTHORIZED, json!({"error": "log in"}))
                }
                Role::None => json_status(StatusCode::UNAUTHORIZED, json!({"error": self.where_link()})),
                Role::Anonymous => json_status(StatusCode::FORBIDDEN, json!({"error": "log in to change the library"})),
                _ => json_status(StatusCode::FORBIDDEN, json!({"error": "only admins may do that"})),
            };
        }
        // SameSite keeps the cookie off other sites' requests; Origin is the second lock on writes. Only its
        // name is compared: behind a proxy the page is https while carrel itself speaks http.
        let origin = request.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok());
        if request.method() != Method::GET && origin.is_some_and(|o| o.split_once("://").map(|(_, h)| h) != Some(host))
        {
            return json_status(StatusCode::FORBIDDEN, json!({"error": "not from this page"}));
        }
        if let (Some(auth), ["tokens", rest @ ..]) = (&self.auth, &segments_ref[..]) {
            let reply = match (method.as_str(), rest) {
                ("GET", []) => Ok(auth.tokens(&who, query.contains_key("all"))),
                ("POST", []) => {
                    let body = Limited::new(request.into_body(), 4096).collect().await.map(|b| b.to_bytes());
                    let name = body.ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                    let name = name.as_ref().and_then(|v| v["name"].as_str()).unwrap_or_default().to_string();
                    auth.create_token(&who, &name)
                }
                ("DELETE", [id]) => auth.revoke_token(&who, id).map(|()| json!({})),
                _ => return json_status(StatusCode::NOT_FOUND, json!({"error": "no such API call"})),
            };
            return match reply {
                Ok(value) => json_status(StatusCode::OK, value),
                Err(e) => json_status(StatusCode::BAD_REQUEST, json!({"error": format!("{e:#}")})),
            };
        }
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(e) => return json_status(StatusCode::PAYLOAD_TOO_LARGE, json!({"error": e.to_string()})),
        };
        // On a server, with the private projects of whoever asks.
        let lib = match self.local {
            true => self.lib.clone(),
            false => self.lib.for_user(who.user.as_deref().filter(|_| who.role.can(Right::Edit))),
        };
        let (history, writes) = (self.history.clone(), self.writes.clone());
        let name = who.user.clone().unwrap_or_else(|| "anonymous".into());
        let client = who.client.clone();
        let is_admin = who.role == Role::Admin;
        // The library blocks (files, SQLite, lookups online), so off the async threads.
        let reply = tokio::task::spawn_blocking(move || {
            let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
            let Some(history) = history else {
                return api_call(&lib, &method, &segments, &query, body);
            };
            let _one_at_a_time = (method != Method::GET).then(|| writes.lock().unwrap_or_else(|e| e.into_inner()));
            if let Some(reply) = activity_call(&lib, &history, &method, &segments, &query, &name, is_admin) {
                return reply;
            }
            if method == Method::GET {
                return api_call(&lib, &method, &segments, &query, body);
            }
            let (what, paper, folders) = described(&lib, &method, &segments);
            let folders: Vec<&str> = folders.iter().map(String::as_str).collect();
            let before = history.before(&name, client.as_deref(), &what, paper.as_deref(), &folders)?;
            let reply = api_call(&lib, &method, &segments, &query, body);
            history.after(before)?;
            reply
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

/// A server's `/api/activity` calls, and deleting a paper, which there is asked for; `None` for the rest.
fn activity_call(
    lib: &Library,
    history: &crate::history::History,
    method: &Method,
    path: &[&str],
    query: &HashMap<String, String>,
    who: &str,
    admin: bool,
) -> Option<Result<Reply>> {
    let reply = match (method.as_str(), path) {
        ("GET", ["activity"]) => {
            let limit = query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(200);
            ok(json!(history.entries(limit)))
        }
        ("GET", ["activity", id, "diff"]) => (|| {
            let (before, after) = history.diff(id, query.get("path").context("which file? ?path=")?)?;
            ok(json!({"before": before, "after": after}))
        })(),
        ("DELETE", ["papers", id]) => (|| {
            let p = lib.get(id)?;
            let files = carrel::remote::files(&p.dir)?.into_iter().map(|f| format!("{}/{f}", p.id)).collect::<Vec<_>>();
            if history.request_deletion(who, Some(&p.id), &files)? {
                bail!("an admin turned down deleting {} lately", p.id);
            }
            ok(json!({"asked": true}))
        })(),
        // Shared projects too; private ones are their owner's to delete.
        ("DELETE", ["projects", name]) if !lib.is_private(name) => (|| {
            lib.project(name)?;
            let file = format!("{}/{name}.txt", carrel::PROJECTS);
            if history.request_deletion(who, None, &[file])? {
                bail!("an admin turned down deleting the project {name} lately");
            }
            ok(json!({"deleted": false, "asked": true}))
        })(),
        ("POST", ["activity", id, "revert"]) if admin => history.revert(id, who).and_then(|()| ok(json!({}))),
        ("POST", ["activity", id, action @ ("accept" | "decline")]) if admin => {
            history.decide(id, who, *action == "accept").and_then(|()| ok(json!({})))
        }
        _ => return None,
    };
    Some(reply)
}

/// What a call changes, for the history: its description, its paper, and the top-level folders it may touch.
/// New papers it adds are noticed anyway.
fn described(lib: &Library, method: &Method, path: &[&str]) -> (String, Option<String>, Vec<String>) {
    use carrel::{authors, recommended};
    let what = match (method.as_str(), path) {
        (_, ["add" | "upload"]) | (_, ["inbox", "add"]) => "added a paper",
        (_, ["recommended", _, "add"]) => "added a recommendation",
        (_, ["recommended", _, "dismiss"]) => "dismissed a recommendation",
        (_, ["people", ..]) => "changed people",
        (_, ["papers", _, "notes"]) => "edited the notes",
        (_, ["papers", _, "update"]) => "updated the metadata",
        (_, ["papers", _, "keywords"]) => "changed the keywords",
        (_, ["papers", _, "projects", ..]) | (_, ["projects", ..]) => "changed a project",
        (_, ["papers", _, "drafts", _, "approve"]) => "approved a draft",
        (_, ["papers", _, "drafts", _, "reject"]) => "rejected a draft",
        (_, ["papers", _, "pdf"]) => "attached a PDF",
        (_, ["papers", _, "supplementary"]) => "added supplementary files",
        (_, ["papers", _, "extract"]) => "extracted the text again",
        _ => "changed",
    };
    let paper = match path {
        ["papers", id, ..] => lib.get(id).ok().map(|p| p.id),
        _ => None,
    };
    let mut folders: Vec<String> = paper.iter().cloned().collect();
    match path {
        ["papers", _, "projects", ..] | ["projects", ..] => folders.push(carrel::PROJECTS.into()),
        ["people", ..] => folders.push(authors::DIR.into()),
        ["recommended", ..] => folders.push(recommended::DIR.into()),
        _ => {}
    }
    (what.into(), paper, folders)
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
            let projects = lib.projects().into_iter().map(|name| project(lib, &name));
            ok(Value::Array(projects.collect()))
        }
        (m, ["projects"]) if *m == post => {
            let name = field("name")?;
            lib.create_project(&name, form()?["private"].as_bool().unwrap_or(false))?;
            ok(project(lib, &name))
        }
        (m, ["projects", name]) if m.as_str() == "PATCH" => {
            let change = form()?;
            let mut name = name.to_string();
            if let Some(new) = change["name"].as_str().filter(|new| *new != name) {
                lib.rename_project(&name, new)?;
                name = new.into();
            }
            if let Some(private) = change["private"].as_bool() {
                lib.set_private(&name, private)?;
            }
            ok(project(lib, &name))
        }
        (m, ["projects", name]) if m.as_str() == "DELETE" => {
            lib.delete_project(name)?;
            ok(json!({"deleted": true, "asked": false}))
        }
        (m, ["suggestions"]) if *m == get => {
            let within = query.get("project").map(|p| lib.project(p)).transpose()?;
            let mut suggestions = lib.graph()?.suggestions(within.as_deref());
            suggestions.truncate(query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50));
            ok(json!(suggestions))
        }
        // Papers recommended by alert emails, waiting to be added or dismissed.
        (m, ["recommended"]) if *m == get => {
            ok(json!(lib.recommendations(query.get("all").is_some_and(|a| a == "1" || a == "true"))))
        }
        (m, ["recommended", key, "add"]) if *m == post => {
            let (id, messages) = lib.add_recommended(key)?;
            ok(json!({"id": id, "messages": messages}))
        }
        (m, ["recommended", key, "dismiss"]) if *m == post => {
            lib.dismiss(key)?;
            ok(json!({}))
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
        (m, ["projects", name]) if m.as_str() == "DELETE" => {
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
fn project(lib: &Library, name: &str) -> Value {
    json!({"name": name, "papers": lib.project(name).unwrap_or_default(), "private": lib.is_private(name)})
}

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
    // Institutions shared by two or more of these papers, each with its papers and who wrote them there.
    let mut places: BTreeMap<String, (carrel::affiliations::Institution, BTreeMap<String, Vec<String>>)> =
        BTreeMap::new();
    for p in &papers {
        for author in carrel::affiliations::read(&p.dir).unwrap_or_default() {
            for place in author.institutions {
                let entry = places.entry(place.key()).or_insert_with(|| (place, BTreeMap::new()));
                entry.1.entry(p.id.clone()).or_default().push(author.name.clone());
            }
        }
    }
    let institutions = places.into_values().filter(|(_, papers)| papers.len() > 1).map(
        |(place, papers)| json!({"name": place.name, "country": place.country, "ror": place.ror, "papers": papers}),
    );
    Ok(json!({
        "nodes": nodes.collect::<Vec<_>>(),
        "cites": cites.map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        "authors": authors,
        "institutions": institutions.collect::<Vec<_>>(),
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

static UPLOADS: AtomicUsize = AtomicUsize::new(0);

/// A file sent by the browser, in a folder of its own that goes away with it.
struct Upload {
    dir: PathBuf,
    path: PathBuf,
}

impl Upload {
    fn new(name: &str, bytes: &[u8]) -> Result<Self> {
        let name = Path::new(name).file_name().context("no file name")?.to_string_lossy().into_owned();
        // A new folder only we can enter: in a shared /tmp, another user could have made the folder, or a
        // symlink by that name, first.
        let dir = loop {
            let n = UPLOADS.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("carrel-upload-{}-{n}", std::process::id()));
            match std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700).create(&dir) {
                Ok(()) => break dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
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
    fn upload_folders() {
        use std::os::unix::fs::PermissionsExt;
        let next = |k| {
            let n = UPLOADS.load(Ordering::Relaxed) + k;
            std::env::temp_dir().join(format!("carrel-upload-{}-{n}", std::process::id()))
        };
        // Another user got there first: a folder by the next name, a symlink to it by the one after.
        let (theirs, link) = (next(0), next(1));
        fs::create_dir(&theirs).unwrap();
        std::os::unix::fs::symlink(&theirs, &link).unwrap();
        let upload = Upload::new("x.pdf", b"private").unwrap();
        assert!(!theirs.join("x.pdf").exists() && upload.path.exists());
        assert_eq!(fs::metadata(&upload.dir).unwrap().permissions().mode() & 0o777, 0o700);
        drop(upload);
        fs::remove_file(link).unwrap();
        fs::remove_dir(theirs).unwrap();
    }

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
