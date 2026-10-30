//! `carrel serve`: a library on a server, for a group and its AIs. It serves MCP over HTTP and, every few
//! minutes, syncs with the remote and does the slow work for everyone: extracting text and figures,
//! looking up references, and computing embeddings. Its notifiers only hear about its own trouble.

use anyhow::{Context, Result, bail};
use carrel::remote::RemoteConfig;
use carrel::{Config, Library, consume, notify};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn run(lib: Library, listen: Option<String>, every: Option<u64>) -> Result<()> {
    let config = Config::load()?;
    let listen = listen.or(config.serve.listen).unwrap_or_else(|| "127.0.0.1:8080".into());
    let addr: SocketAddr = listen.parse().with_context(|| format!("cannot listen on {listen:?}; try 0.0.0.0:8080"))?;
    let token = match std::env::var("CARREL_SERVE_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
        Some(token) => Some(token.trim().to_string()),
        None => config.serve.token.as_ref().map(|t| t.read()).transpose()?,
    };
    let networks = |list: &[String], name| -> Result<Vec<crate::mcp::Network>> {
        list.iter().map(|n| n.parse()).collect::<Result<_>>().with_context(|| format!("in [serve] {name}"))
    };
    let trusted = networks(&config.serve.trusted, "trusted")?;
    let proxies = networks(&config.serve.proxies, "proxies")?;
    if token.is_none() && trusted.is_empty() && !addr.ip().is_loopback() {
        bail!(
            "the library would be open to anyone on the network; set CARREL_SERVE_TOKEN or [serve] token, \
             or name the networks to let in with [serve] trusted"
        );
    }
    let every = Duration::from_secs(60 * every.or(config.serve.every).unwrap_or(5).max(1));

    let worker = lib.clone();
    let jobs = Jobs { remote: config.remote, consume: config.consume, notify: config.notify };
    std::thread::spawn(move || work(&worker, &jobs, every));

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("cannot listen on {addr}"))?;
        log(&format!("serving {} at http://{}/mcp", lib.root.display(), listener.local_addr()?));
        if !config.serve.trusted.is_empty() {
            log(&format!("no token needed from {}", config.serve.trusted.join(", ")));
        }
        tokio::select! {
            served = crate::mcp::serve_http(lib, listener, token, trusted, proxies) => served,
            () = stopped() => {
                log("stopping");
                Ok(())
            }
        }
    })
}

/// When asked to stop: Ctrl-C, or SIGTERM from `docker stop` and systemd. In a container carrel is PID 1,
/// which the kernel only stops for signals it handles.
async fn stopped() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Syncs and catches up on work, round after round. Each paper gets one try per job while the server runs,
/// so a paper no source knows isn't asked about every few minutes.
fn work(lib: &Library, jobs: &Jobs, every: Duration) {
    let mut state = State::default();
    loop {
        if let Err(e) = round(lib, jobs, &mut state) {
            log(&format!("{e:#}"));
        }
        std::thread::sleep(every);
    }
}

/// The config's parts the rounds need.
struct Jobs {
    remote: Option<RemoteConfig>,
    consume: Option<consume::Config>,
    notify: notify::Config,
}

#[derive(Default)]
struct State {
    /// (job, paper) pairs tried already.
    tried: HashSet<(&'static str, String)>,
    /// What's wrong with Ollama, told once until it changes.
    ollama: Option<String>,
}

fn round(lib: &Library, jobs: &Jobs, state: &mut State) -> Result<()> {
    let tried = &mut state.tried;
    let sync = |lib: &Library| -> Result<()> {
        let Some(remote) = &jobs.remote else { return Ok(()) };
        let report = lib.sync(&*remote.open()?, None, false, |_, _| {})?;
        if !report.done.is_empty() || !report.failed.is_empty() {
            log(&format!("synced: {}", report.summary()));
        }
        for path in &report.skipped {
            log(&format!("sync skipped {path:?} on the remote: not a file path inside the library"));
        }
        for (path, error) in &report.failed {
            log(&format!("sync failed for {path}: {error}"));
        }
        Ok(())
    };
    sync(lib)?;
    let mut changed = false;
    if let Some(config) = &jobs.consume
        && let Some((subject, body)) = consume::unattended(lib, config, config.auto, false)?
    {
        // Logged, not notified: a server's drop folder fills with everyone's papers, and the notifiers
        // are for the server's own trouble.
        changed = true;
        log(&format!("{subject}\n{}", body.trim_end()));
    }
    for p in lib.papers().0 {
        if p.pdf().is_some() && p.fulltext().is_none() && tried.insert(("text", p.id.clone())) {
            changed = true;
            match lib.extract(&p.id) {
                Ok(()) => log(&format!("{}: extracted text and figures", p.id)),
                Err(e) => log(&format!("{}: extraction failed: {e:#}", p.id)),
            }
        }
        if !p.dir.join(carrel::graph::FILE).exists() && tried.insert(("refs", p.id.clone())) {
            // A failure is most often "no source has its references": for the doctor, not the log.
            if let Ok((n, note)) = lib.look_up_references(&p) {
                changed = true;
                let note = note.map_or(String::new(), |note| format!("{note}; "));
                log(&format!("{}: {note}{n} references", p.id));
            }
        } else if carrel::affiliations::read(&p.dir).is_none()
            && tried.insert(("institutions", p.id.clone()))
            && let Ok(n) = lib.look_up_affiliations(&p)
        {
            changed = true;
            log(&format!("{}: {n} institutions", p.id));
        }
    }
    if let Some(config) = &lib.embeddings {
        match carrel::embed::Ollama::connect(config) {
            Ok(ollama) => {
                if state.ollama.take().is_some() {
                    let news = format!("Ollama works again: {} on {}", ollama.model, ollama.url);
                    log(&news);
                    tell(&jobs.notify, "carrel: embeddings work again", &news);
                }
                // After extraction, so new full texts are embedded in the same round.
                for p in lib.papers().0 {
                    if lib.embedded(&p, &ollama.model) || state.tried.contains(&("embed", p.id.clone())) {
                        continue;
                    }
                    match lib.embed(&ollama, &p) {
                        Ok(_) => {
                            changed = true;
                            log(&format!("{}: embedded with {}", p.id, ollama.model));
                        }
                        Err(e) => {
                            state.tried.insert(("embed", p.id.clone()));
                            log(&format!("{}: embedding failed: {e:#}", p.id));
                        }
                    }
                }
            }
            Err(e) => {
                let problem = format!("no embeddings: {e:#}");
                if state.ollama.as_ref() != Some(&problem) {
                    log(&problem);
                    let hint =
                        "Semantic search misses new papers until this is fixed; check [embeddings] in the config.";
                    tell(&jobs.notify, "carrel: no embeddings", &format!("{problem}\n\n{hint}\n"));
                    state.ollama = Some(problem);
                }
            }
        }
    }
    if changed {
        sync(lib)?;
    }
    Ok(())
}

fn tell(config: &notify::Config, subject: &str, body: &str) {
    for failed in notify::send(config, subject, body) {
        log(&format!("cannot notify: {failed}"));
    }
}

/// A line for the log, with the time in UTC.
fn log(message: &str) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    println!("{h:02}:{m:02}:{s:02} {message}");
}
