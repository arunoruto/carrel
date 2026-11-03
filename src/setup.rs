//! Setting a machine up: `carrel connect`, and `carrel setup` with its TUI twin, which ask for the few settings
//! that matter one at a time, each with what it is now (or a sensible default) to take with Enter.

use crate::install::{self, Service};
use anyhow::{Context, Result, bail};
use carrel::Config;
use std::fs;
use std::io::{IsTerminal, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use toml_edit::{DocumentMut, InlineTable, Item, value};

/// A question of `carrel setup`; `answer` starts as the default, and `-` clears the setting.
pub struct Question {
    pub key: &'static str,
    pub answer: String,
}

/// The questions, answered with the config as it is, or what carrel would pick.
pub fn questions() -> Result<Vec<Question>> {
    let config = Config::load()?;
    let (server, name) = match &config.remote {
        Some(carrel::remote::RemoteConfig::Carrel { url, name, .. }) => (url.clone(), name.clone()),
        _ => (String::new(), None),
    };
    let yes = |b: bool| if b { "y" } else { "n" }.to_string();
    let local_ollama = || which("ollama").then(|| "http://localhost:11434".to_string());
    let ollama = match &config.embeddings {
        Some(e) => e.ollama.clone().unwrap_or("http://localhost:11434".into()),
        None => local_ollama().unwrap_or_default(),
    };
    // Unchanged where it is set; on with a new Docling, since that is mostly a server with a GPU.
    let formulas = config.formulas.unwrap_or(config.docling.is_none());
    let service = |s| install::installed(s).ok().flatten().map(yes).unwrap_or("elsewhere".into());
    // Connecting is mostly for syncing.
    let sync = match service(Service::Sync).as_str() {
        "elsewhere" => "elsewhere".to_string(),
        _ => "y".to_string(),
    };
    let answers = [
        ("library", config.library.unwrap_or_else(default_library)),
        ("server", server),
        ("name", carrel::remote::identity(name.as_deref())),
        ("token", String::new()),
        ("sync", sync),
        ("docling", config.docling.unwrap_or_default()),
        ("formulas", yes(formulas)),
        ("ollama", ollama),
        ("web", service(Service::Web)),
        ("consume", service(Service::Consume)),
    ];
    Ok(answers.into_iter().map(|(key, answer)| Question { key, answer: answer.to_string() }).collect())
}

/// Where the library is without `library` in the config.
fn default_library() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let documents = dirs::document_dir().unwrap_or(home.join("Documents")).join("carrel");
    match documents.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => documents.display().to_string(),
    }
}

fn answer<'a>(qs: &'a [Question], key: &str) -> &'a str {
    qs.iter().find(|q| q.key == key).map_or("", |q| q.answer.trim())
}

fn set(s: &str) -> Option<&str> {
    Some(s).filter(|s| !s.is_empty() && *s != "-")
}

fn server(qs: &[Question]) -> Option<String> {
    set(answer(qs, "server")).map(|url| {
        let url = url.trim_end_matches('/');
        if url.contains("://") { url.to_string() } else { format!("https://{url}") }
    })
}

/// The next question after `at` (from the first, with None) that applies, given the answers so far.
pub fn next(qs: &[Question], at: Option<usize>) -> Option<usize> {
    let from = at.map_or(0, |i| i + 1);
    (from..qs.len()).find(|&i| match qs[i].key {
        "name" | "token" => server(qs).is_some(),
        "sync" => server(qs).is_some() && qs[i].answer != "elsewhere",
        "formulas" => set(answer(qs, "docling")).is_some(),
        "web" | "consume" => qs[i].answer != "elsewhere", // Home Manager's, say
        _ => true,
    })
}

pub fn secret(q: &Question) -> bool {
    q.key == "token"
}

pub fn yes_no(q: &Question) -> bool {
    matches!(q.key, "sync" | "formulas" | "web" | "consume")
}

/// What to ask; the answer so far is the default.
pub fn prompt(qs: &[Question], i: usize) -> String {
    match qs[i].key {
        "library" => "Library folder".into(),
        "server" => "Your group's carrel server, like https://carrel.example.org (- for none)".into(),
        "name" => "Your name on it".into(),
        "token" => format!(
            "Token from {}/#/tokens; without one you can only download and add (Enter: the saved one, else none)",
            server(qs).unwrap_or_default()
        ),
        "docling" => {
            "Docling server, like http://gpu-box:5001 (- for none: Docling here if found, else PyMuPDF)".into()
        }
        "sync" => "Sync with it every 10 minutes in the background?".into(),
        "formulas" => "Formulas as LaTeX? About a minute each without a GPU".into(),
        "ollama" => "Ollama for semantic search, like http://localhost:11434 (- for none)".into(),
        "web" => "Run the web UI in the background?".into(),
        "consume" => "Add the PDFs you download as they arrive?".into(),
        key => key.into(),
    }
}

/// Takes `line` as the answer to question `i` (empty keeps the default); an error says what's wrong.
pub fn take(qs: &mut [Question], i: usize, line: &str) -> Result<()> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }
    if yes_no(&qs[i]) {
        match line.to_lowercase().as_str() {
            "y" | "yes" => qs[i].answer = "y".into(),
            "n" | "no" => qs[i].answer = "n".into(),
            _ => bail!("y or n"),
        }
        return Ok(());
    }
    qs[i].answer = line.into();
    Ok(())
}

/// Writes the answers to the config and sets up the services; returns what it did.
pub fn apply(qs: &[Question]) -> Result<Vec<String>> {
    let path = Config::path().context("no folder for the config")?;
    let old = fs::read_to_string(&path).unwrap_or_default();
    let mut doc: DocumentMut = old.parse().with_context(|| format!("{} is invalid", path.display()))?;
    let mut said = vec![];
    let library = doc.get("library").and_then(Item::as_str).map(String::from).unwrap_or_else(default_library);
    for key in ["library", "docling"] {
        match answer(qs, key) {
            "-" => drop(doc.remove(key)),
            "" => {}
            given if key == "library" && given == library => {} // also where it is without the setting
            given => doc[key] = value(given),
        }
    }
    if let Some(given) = set(answer(qs, "library"))
        && given != library
    {
        said.push(format!("the library is in {given} from carrel's next start"));
    }
    // Docling found on PATH has its formulas set by hand; they stay.
    if set(answer(qs, "docling")).is_some() {
        doc["formulas"] = value(answer(qs, "formulas") == "y");
    }
    match answer(qs, "ollama") {
        "-" => drop(doc.remove("embeddings")),
        "" => {}
        url => {
            if doc.get("embeddings").is_none() {
                doc["embeddings"] = Item::Table(new_table(&doc));
            }
            doc["embeddings"]["ollama"] = value(url);
        }
    }
    let server = server(qs);
    match &server {
        Some(url) => {
            let token = set(answer(qs, "token")).map(String::from).or_else(|| kept(url));
            let name = answer(qs, "name");
            let name = (name != carrel::remote::identity(None)).then_some(name);
            said.push(remote(&mut doc, url, token, name)?);
        }
        None if answer(qs, "server") == "-" => drop(doc.remove("remote")),
        None => {}
    }
    said.extend(save(&path, &old, &doc.to_string())?);
    let wanted = [
        (Service::Sync, server.as_ref().map(|_| answer(qs, "sync"))),
        (Service::Web, Some(answer(qs, "web"))),
        (Service::Consume, Some(answer(qs, "consume"))),
    ];
    for (service, want) in wanted {
        if install::installed(service)?.is_none() {
            continue; // Home Manager's, say
        }
        let result = match want {
            Some("y") => install::install(service, "127.0.0.1:7700").and_then(|_| install::status(service)),
            Some("n") if install::installed(service)? == Some(true) => {
                install::remove(service).map(|_| format!("carrel {}: removed", service.name()))
            }
            _ => continue,
        };
        said.push(result.unwrap_or_else(|e| format!("carrel {}: {e:#}", service.name())));
    }
    if server.is_some() {
        said.push("the library arrives with the first sync, within a minute; `carrel sync` fetches it now".into());
    }
    Ok(said)
}

/// `carrel setup`: asks on the terminal (or reads the answers, a line each, from a pipe), then applies.
pub fn ask() -> Result<()> {
    let terminal = std::io::stdin().is_terminal();
    println!("Enter takes the answer in [brackets]; - leaves a setting out.");
    let mut qs = questions()?;
    let mut at = next(&qs, None);
    while let Some(i) = at {
        let shown = match &qs[i].answer {
            answer if secret(&qs[i]) || answer.is_empty() => String::new(),
            answer => format!(" [{answer}]"),
        };
        let ask = format!("{}{shown}: ", prompt(&qs, i));
        if terminal && secret(&qs[i]) && kept(&server(&qs).unwrap_or_default()).is_none() {
            let _ = crate::open(Path::new(&format!("{}/#/tokens", server(&qs).unwrap_or_default())));
        }
        let line = if secret(&qs[i]) { read_secret(&ask)? } else { read_line(&ask)? };
        if let Err(e) = take(&mut qs, i, &line) {
            if !terminal {
                bail!("{line:?}: {e:#}");
            }
            println!("  {e:#}");
            continue;
        }
        // A server that doesn't answer is better known now than after the other questions.
        if qs[i].key == "server"
            && let Some(url) = server(&qs)
            && let Err(e) = server_me(&url, None)
        {
            if !terminal {
                return Err(e);
            }
            println!("  {e:#}");
            continue;
        }
        at = next(&qs, Some(i));
    }
    for line in apply(&qs)? {
        println!("{line}");
    }
    println!("`carrel config` shows what carrel found");
    Ok(())
}

/// `carrel connect`: the server as the remote, with the token from `token_file`, the one kept from before or
/// one asked for.
pub fn connect(url: &str, token_file: Option<std::path::PathBuf>, web: bool, no_service: bool) -> Result<()> {
    let url = url.trim_end_matches('/').to_string();
    let token = match (token_file, kept(&url)) {
        (Some(file), _) => Some(fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?),
        (None, Some(saved)) => Some(saved),
        (None, None) if std::io::stdin().is_terminal() => {
            let tokens = format!("{url}/#/tokens");
            println!("A token lets this machine share changes too: make one for it at {tokens} (opening it now).");
            let _ = crate::open(Path::new(&tokens));
            Some(read_secret("Paste it here, or press Enter to connect without one: ")?)
        }
        (None, None) => None,
    };
    let path = Config::path().context("no folder for the config")?;
    let old = fs::read_to_string(&path).unwrap_or_default();
    let mut doc: DocumentMut = old.parse().with_context(|| format!("{} is invalid", path.display()))?;
    let name = doc.get("remote").and_then(|r| r.get("name")).and_then(Item::as_str).map(String::from);
    println!("{}", remote(&mut doc, &url, token.filter(|t| !t.trim().is_empty()), name.as_deref())?);
    for line in save(&path, &old, &doc.to_string())? {
        println!("{line}");
    }
    if no_service {
        println!("`carrel sync` fetches the library");
        return Ok(());
    }
    let services = [Service::Sync].into_iter().chain(web.then_some(Service::Web));
    for service in services {
        match install::install(service, "127.0.0.1:7700") {
            Ok(_) => println!("{}", install::status(service)?),
            Err(e) => println!("cannot set up the background service: {e:#}"),
        }
    }
    println!("the library arrives with the first sync, within a minute; `carrel sync` fetches it now");
    Ok(())
}

/// Where the server's token is kept, so connecting again needs no new one.
fn token_file() -> Result<std::path::PathBuf> {
    let config = Config::path().context("no folder for the config")?;
    Ok(config.with_file_name("tokens").join("carrel.txt"))
}

/// The kept token, when `url` takes it for a member or admin.
fn kept(url: &str) -> Option<String> {
    let saved = fs::read_to_string(token_file().ok()?).ok()?.trim().to_string();
    let who = server_me(url, Some(&saved)).ok()?;
    matches!(who["role"].as_str(), Some("member" | "admin")).then_some(saved)
}

/// Makes `url` the config's `[remote]`, checking the token with it; says who it takes this machine for.
fn remote(doc: &mut DocumentMut, url: &str, token: Option<String>, name: Option<&str>) -> Result<String> {
    let anonymous = server_me(url, None)?;
    if anonymous["server"] != true {
        bail!("{url} is a carrel web for one machine, not a server");
    }
    doc.remove("remote");
    let mut table = new_table(doc);
    table["type"] = value("carrel");
    table["url"] = value(url);
    let said = match token.map(|t| t.trim().to_string()) {
        Some(token) => {
            let who = server_me(url, Some(&token))?;
            if !matches!(who["role"].as_str(), Some("member" | "admin")) {
                bail!("{url} doesn't know that token; make one on its Tokens page");
            }
            // Beside the other tokens of `[tokens]`.
            let file = token_file()?;
            fs::create_dir_all(file.parent().unwrap())?;
            let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&file)?;
            out.write_all(token.as_bytes())?;
            let mut secret = InlineTable::new();
            secret.insert("file", file.to_string_lossy().as_ref().into());
            table["token"] = value(secret);
            format!("connected as {} ({})", who["user"].as_str().unwrap_or("?"), who["role"].as_str().unwrap_or("?"))
        }
        None => {
            if anonymous["role"] != "anonymous" {
                bail!("{url} lets this machine in only with a token; make one on its Tokens page");
            }
            format!(
                "connected without a token, as {}: you can download and add papers; changes you make stay here",
                carrel::remote::identity(name)
            )
        }
    };
    if let Some(name) = name {
        table["name"] = value(name);
    }
    doc["remote"] = Item::Table(table);
    Ok(said)
}

/// A table to add to `doc`, a blank line after what is there.
fn new_table(doc: &DocumentMut) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    if !doc.is_empty() {
        table.decor_mut().set_prefix("\n");
    }
    table
}

/// Writes the config, keeping the old one beside it; puts the old one back when the new one doesn't load.
fn save(path: &Path, old: &str, new: &str) -> Result<Vec<String>> {
    if old == new {
        return Ok(vec![]);
    }
    let mut said = vec![];
    if !old.trim().is_empty() {
        // As private as the config: it may hold secrets.
        let backup = path.with_extension("toml.bak");
        let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&backup)?;
        out.write_all(old.as_bytes())?;
        said.push(format!("updated {}; the old config is in config.toml.bak", path.display()));
    }
    fs::create_dir_all(path.parent().context("no folder for the config")?)?;
    fs::write(path, new)?;
    if let Err(e) = Config::load() {
        fs::write(path, old)?;
        return Err(e.context("the new config doesn't load; left it as it was"));
    }
    Ok(said)
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Who a carrel server takes this machine for, with `token` or without.
pub fn server_me(url: &str, token: Option<&str>) -> Result<serde_json::Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let mut request = agent.get(format!("{url}/api/me")).header("X-Carrel-Client", carrel::remote::identity(None));
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call().with_context(|| format!("cannot reach {url}"))?;
    response.body_mut().read_json().with_context(|| format!("{url} doesn't answer like a carrel server"))
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        bail!("stopped before the end; nothing was changed");
    }
    Ok(line.trim().to_string())
}

/// A line from the terminal, not shown as it is typed.
fn read_secret(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    // SAFETY: termios is plain data, filled by tcgetattr before use; fd 0 is stdin, a terminal here.
    let mut was: libc::termios = unsafe { std::mem::zeroed() };
    let hidden = unsafe { libc::tcgetattr(0, &mut was) } == 0;
    if hidden {
        let mut quiet = was;
        quiet.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) };
    }
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if hidden {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &was) };
        println!();
    }
    read?;
    Ok(line.trim().to_string())
}
