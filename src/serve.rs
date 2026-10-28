//! `carrel serve`: a library on a server, for a group and its AIs. It serves MCP over HTTP and, every few
//! minutes, syncs with the remote and does the slow work for everyone: extracting text and figures, and
//! looking up references.

use anyhow::{Context, Result, bail};
use carrel::remote::RemoteConfig;
use carrel::{Config, Library};
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
    let remote = config.remote;
    std::thread::spawn(move || work(&worker, remote.as_ref(), every));

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
fn work(lib: &Library, remote: Option<&RemoteConfig>, every: Duration) {
    let mut tried = HashSet::new();
    loop {
        if let Err(e) = round(lib, remote, &mut tried) {
            log(&format!("{e:#}"));
        }
        std::thread::sleep(every);
    }
}

fn round(lib: &Library, remote: Option<&RemoteConfig>, tried: &mut HashSet<(&'static str, String)>) -> Result<()> {
    let sync = |lib: &Library| -> Result<()> {
        let Some(remote) = remote else { return Ok(()) };
        let report = lib.sync(&*remote.open()?, None, false, |_, _| {})?;
        if !report.done.is_empty() || !report.failed.is_empty() {
            log(&format!("synced: {}", report.summary()));
        }
        for (path, error) in &report.failed {
            log(&format!("sync failed for {path}: {error}"));
        }
        Ok(())
    };
    sync(lib)?;
    let mut changed = false;
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
        }
    }
    if changed {
        sync(lib)?;
    }
    Ok(())
}

/// A line for the log, with the time in UTC.
fn log(message: &str) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    println!("{h:02}:{m:02}:{s:02} {message}");
}
