//! Setting carrel up on a machine without Nix: `carrel service` runs `carrel web` and `carrel consume` in
//! the background (systemd user units on Linux, launchd agents on macOS), and `carrel integrate` connects
//! AI agents to the library over MCP. Home Manager's `services.carrel` does the former for Nix users.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What `carrel service` can run.
#[derive(Clone, Copy, PartialEq, Debug, ValueEnum)]
pub enum Service {
    /// `carrel web`, the library in your browser
    Web,
    /// `carrel consume` whenever a PDF lands in Downloads, and hourly
    Consume,
}

/// The AI agents `carrel integrate` knows.
#[derive(Clone, Copy, PartialEq, Debug, ValueEnum)]
pub enum Agent {
    Claude,
    Opencode,
    Codex,
    Gemini,
    Cursor,
}

pub const AGENTS: [Agent; 5] = [Agent::Claude, Agent::Opencode, Agent::Codex, Agent::Gemini, Agent::Cursor];

/// Environment variables a service inherits from the shell it was installed from, so it finds the same
/// library, Docling and uv; tokens belong in the config instead.
const PASSED_ON: [&str; 5] = ["PATH", "CARREL_HOME", "CARREL_DOCLING", "CARREL_EXTRACTOR", "OLLAMA_HOST"];

/// carrel's path for services and agents to run: as found on PATH when that is this program
/// (`~/.nix-profile/bin/carrel` outlives the store path it points to), else this program's own path.
fn exe() -> Result<PathBuf> {
    let me = std::env::current_exe()?;
    let real = fs::canonicalize(&me)?;
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("carrel"))
        .find(|c| fs::canonicalize(c).is_ok_and(|c| c == real));
    Ok(on_path.unwrap_or(me))
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("no home folder")
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program).args(args).status().with_context(|| format!("running {program}"))?;
    match status.success() {
        true => Ok(()),
        false => bail!("`{program} {}` failed", args.join(" ")),
    }
}

fn quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).output().is_ok_and(|o| o.status.success())
}

// --- Services ---

impl Service {
    fn name(self) -> &'static str {
        match self {
            Service::Web => "web",
            Service::Consume => "consume",
        }
    }

    fn args(self, listen: &str) -> Vec<String> {
        match self {
            Service::Web => ["web", "--no-open", "--listen", listen].map(String::from).to_vec(),
            Service::Consume => vec!["consume".into()],
        }
    }

    fn label(self) -> String {
        format!("io.github.arunoruto.carrel.{}", self.name())
    }
}

/// What a service needs to know to be written down.
struct Unit {
    exe: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    folders: Vec<PathBuf>,
}

impl Unit {
    fn new(service: Service, listen: &str) -> Result<Self> {
        let env = PASSED_ON.iter().filter_map(|k| Some((k.to_string(), std::env::var(k).ok()?))).collect();
        let folders = carrel::Config::load()?.consume.unwrap_or_default().folders();
        Ok(Self { exe: exe()?, args: service.args(listen), env, folders })
    }
}

/// systemd quoting: words in double quotes, `%` doubled.
fn sd_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%"))
}

/// The systemd user units for a service: file name and contents.
fn systemd(service: Service, unit: &Unit) -> Vec<(String, String)> {
    let exec = std::iter::once(unit.exe.to_string_lossy().into_owned())
        .chain(unit.args.iter().cloned())
        .map(|w| sd_quote(&w).replace('$', "$$")) // no variables expanded
        .collect::<Vec<_>>()
        .join(" ");
    let env: String =
        unit.env.iter().map(|(k, v)| format!("Environment={}\n", sd_quote(&format!("{k}={v}")))).collect();
    match service {
        Service::Web => vec![(
            "carrel-web.service".into(),
            format!(
                "[Unit]\nDescription=carrel: the library in your browser\n\n[Service]\n{env}ExecStart={exec}\n\
                 Restart=on-failure\n\n[Install]\nWantedBy=default.target\n"
            ),
        )],
        Service::Consume => {
            let watched: String =
                                // Unquoted: systemd takes the rest of the line as the path, spaces and all.
                unit.folders.iter().map(|f| format!("PathChanged={}\n", f.to_string_lossy().replace('%', "%%"))).collect();
            vec![
                (
                    "carrel-consume.service".into(),
                    format!("[Unit]\nDescription=carrel: pick up new papers\n\n[Service]\nType=oneshot\n{env}ExecStart={exec}\n"),
                ),
                (
                    "carrel-consume.path".into(),
                    format!("[Unit]\nDescription=carrel: watch for new papers\n\n[Path]\n{watched}\n[Install]\nWantedBy=default.target\n"),
                ),
                (
                    // A run still going misses changes meanwhile; the timer catches those.
                    "carrel-consume.timer".into(),
                    "[Unit]\nDescription=carrel: look for new papers hourly\n\n[Timer]\nOnCalendar=hourly\nPersistent=true\n\n\
                     [Install]\nWantedBy=timers.target\n"
                        .into(),
                ),
            ]
        }
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The launchd agent for a service.
fn launchd(service: Service, unit: &Unit, logs: &Path) -> String {
    let strings = |items: &mut dyn Iterator<Item = String>| -> String {
        items.map(|s| format!("    <string>{}</string>\n", xml(&s))).collect()
    };
    let args = strings(&mut std::iter::once(unit.exe.to_string_lossy().into_owned()).chain(unit.args.iter().cloned()));
    let env: String =
        unit.env.iter().map(|(k, v)| format!("    <key>{}</key><string>{}</string>\n", xml(k), xml(v))).collect();
    let log = xml(&logs.join(format!("carrel-{}.log", service.name())).to_string_lossy());
    let when = match service {
        // Restarted when it fails, as systemd's Restart=on-failure.
        Service::Web => "  <key>KeepAlive</key>\n  <dict><key>SuccessfulExit</key><false/></dict>\n".to_string(),
        Service::Consume => format!(
            "  <key>WatchPaths</key>\n  <array>\n{}  </array>\n  <key>StartInterval</key><integer>3600</integer>\n",
            strings(&mut unit.folders.iter().map(|f| f.to_string_lossy().into_owned()))
        ),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}  </array>\n  \
         <key>EnvironmentVariables</key>\n  <dict>\n{env}  </dict>\n  <key>RunAtLoad</key><true/>\n{when}  \
         <key>StandardOutPath</key><string>{log}</string>\n  <key>StandardErrorPath</key><string>{log}</string>\n</dict>\n</plist>\n",
        service.label()
    )
}

fn systemd_dir() -> Result<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|d| d.is_absolute());
    Ok(config.unwrap_or(home()?.join(".config")).join("systemd/user"))
}

fn plist(service: Service) -> Result<PathBuf> {
    Ok(home()?.join("Library/LaunchAgents").join(format!("{}.plist", service.label())))
}

fn gui() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

/// The units systemctl starts: the path and timer for consume, which start its service.
fn systemd_started(service: Service) -> &'static [&'static str] {
    match service {
        Service::Web => &["carrel-web.service"],
        Service::Consume => &["carrel-consume.path", "carrel-consume.timer"],
    }
}

/// Home Manager's units are links into the Nix store; those are its to change.
fn managed_elsewhere(dir: &Path, service: Service) -> Result<()> {
    match systemd_started(service).iter().find(|unit| dir.join(unit).is_symlink()) {
        Some(unit) => {
            bail!("{} is set up elsewhere (Home Manager's services.carrel?); change it there", dir.join(unit).display())
        }
        None => Ok(()),
    }
}

/// Writes the service's files and starts it; returns what was written.
pub fn install(service: Service, listen: &str) -> Result<Vec<PathBuf>> {
    let unit = Unit::new(service, listen)?;
    if cfg!(target_os = "macos") {
        let path = plist(service)?;
        let logs = home()?.join("Library/Logs");
        fs::create_dir_all(path.parent().unwrap())?;
        fs::create_dir_all(&logs)?;
        fs::write(&path, launchd(service, &unit, &logs))?;
        let _ = quiet("launchctl", &["bootout", &format!("{}/{}", gui(), service.label())]); // an older one
        run("launchctl", &["bootstrap", &gui(), &path.to_string_lossy()])?;
        return Ok(vec![path]);
    }
    let dir = systemd_dir()?;
    fs::create_dir_all(&dir)?;
    managed_elsewhere(&dir, service)?;
    let mut written = vec![];
    for (name, contents) in systemd(service, &unit) {
        fs::write(dir.join(&name), contents)?;
        written.push(dir.join(name));
    }
    run("systemctl", &["--user", "daemon-reload"])?;
    run("systemctl", &[&["--user", "enable", "--now"][..], systemd_started(service)].concat())?;
    if service == Service::Web {
        run("systemctl", &["--user", "restart", "carrel-web.service"])?; // a running one picks up the new unit
    }
    Ok(written)
}

/// Stops the service and deletes its files; false when it wasn't installed.
pub fn remove(service: Service) -> Result<bool> {
    if cfg!(target_os = "macos") {
        let path = plist(service)?;
        let _ = quiet("launchctl", &["bootout", &format!("{}/{}", gui(), service.label())]);
        return Ok(fs::remove_file(path).is_ok());
    }
    let dir = systemd_dir()?;
    let files: Vec<PathBuf> = systemd(service, &Unit { exe: "".into(), args: vec![], env: vec![], folders: vec![] })
        .into_iter()
        .map(|(name, _)| dir.join(name))
        .filter(|f| f.exists())
        .collect();
    if files.is_empty() {
        return Ok(false);
    }
    managed_elsewhere(&dir, service)?;
    let _ = quiet("systemctl", &[&["--user", "disable", "--now"][..], systemd_started(service)].concat());
    files.iter().try_for_each(fs::remove_file)?;
    run("systemctl", &["--user", "daemon-reload"])?;
    Ok(true)
}

/// A line about the service: whether it is installed and running.
pub fn status(service: Service) -> Result<String> {
    let name = format!("carrel {}", service.name());
    if cfg!(target_os = "macos") {
        let path = plist(service)?;
        return Ok(match path.exists() {
            false => format!("{name}: not installed"),
            true if quiet("launchctl", &["print", &format!("{}/{}", gui(), service.label())]) => {
                format!("{name}: loaded ({})", path.display())
            }
            true => format!("{name}: installed but not loaded ({})", path.display()),
        });
    }
    let unit = systemd_started(service)[0];
    let active = quiet("systemctl", &["--user", "is-active", "--quiet", unit]);
    let file = systemd_dir()?.join(unit);
    Ok(match (file.exists(), active) {
        (true, true) => format!("{name}: running ({})", file.display()),
        (true, false) => format!("{name}: installed, not running; `systemctl --user status {unit}` says why"),
        (false, true) => format!("{name}: running, set up elsewhere (Home Manager?)"),
        (false, false) => format!("{name}: not installed"),
    })
}

// --- AI agents ---

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Opencode => "opencode",
            Agent::Codex => "codex",
            Agent::Gemini => "gemini",
            Agent::Cursor => "cursor",
        }
    }

    /// Its config file, for the agents carrel edits itself; Claude Code's goes through `claude mcp`.
    fn config(self) -> Result<PathBuf> {
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|d| d.is_absolute());
        Ok(match self {
            Agent::Claude => home()?.join(".claude.json"),
            Agent::Opencode => {
                let dir = xdg.unwrap_or(home()?.join(".config")).join("opencode");
                match dir.join("opencode.jsonc").exists() {
                    true => dir.join("opencode.jsonc"),
                    false => dir.join("opencode.json"),
                }
            }
            Agent::Codex => {
                std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(home()?.join(".codex")).join("config.toml")
            }
            Agent::Gemini => home()?.join(".gemini/settings.json"),
            Agent::Cursor => home()?.join(".cursor/mcp.json"),
        })
    }

    /// The entry for carrel in the agent's JSON config, and where it goes.
    fn entry(self, exe: &str) -> (&'static str, Value) {
        match self {
            Agent::Opencode => ("mcp", json!({ "type": "local", "command": [exe, "mcp"], "enabled": true })),
            _ => ("mcpServers", json!({ "command": exe, "args": ["mcp"] })),
        }
    }
}

/// Adds or (with `entry` None) removes carrel in a JSON config; false when nothing changed.
fn edit_json(text: &str, section: &str, entry: Option<Value>) -> Result<(String, bool)> {
    let mut config: Value = match text.trim() {
        "" => json!({}),
        // ponytail: plain JSON only; a config with comments is left alone, its user pastes the entry
        text => serde_json::from_str(text).context("not plain JSON (comments?), so carrel leaves it alone")?,
    };
    let servers = config.as_object_mut().context("not a JSON object")?.entry(section).or_insert(json!({}));
    let servers = servers.as_object_mut().with_context(|| format!("its {section:?} is not an object"))?;
    let changed = match entry {
        Some(entry) => servers.insert("carrel".into(), entry.clone()).is_none_or(|old| old != entry),
        None => servers.remove("carrel").is_some(),
    };
    Ok((serde_json::to_string_pretty(&config)? + "\n", changed))
}

const CODEX_TABLE: &str = "[mcp_servers.carrel]";

/// Adds or removes carrel in Codex's config.toml as text, so its comments stay.
fn edit_toml(text: &str, exe: Option<&str>) -> Result<(String, bool)> {
    // Lines up to the next table header belong to carrel's table.
    let mut kept = String::new();
    let mut inside = false;
    let mut found = false;
    for line in text.lines() {
        let header = line.trim_start().starts_with('[');
        if header {
            // Its sub-tables (`[mcp_servers.carrel.env]`) too when removing it; kept when updating it.
            let sub = exe.is_none() && line.trim().starts_with("[mcp_servers.carrel.");
            inside = line.trim() == CODEX_TABLE || sub;
            found |= inside;
        }
        if !inside {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let Some(exe) = exe else { return Ok((kept, found)) };
    let table = format!("{CODEX_TABLE}\ncommand = {}\nargs = [\"mcp\"]\n", toml::Value::from(exe));
    let sep = if kept.trim().is_empty() || kept.ends_with("\n\n") { "" } else { "\n" };
    let new = format!("{}{sep}{table}", kept.trim_start_matches('\n'));
    new.parse::<toml::Table>().context("its config.toml doesn't parse")?;
    let changed = !text.contains(&table);
    Ok((if changed { new } else { text.to_string() }, changed))
}

/// Connects the agent to carrel (or disconnects it); a line saying what happened.
pub fn integrate(agent: Agent, remove: bool) -> Result<String> {
    let exe = exe()?.to_string_lossy().into_owned();
    if agent == Agent::Claude {
        if !quiet("claude", &["--version"]) {
            bail!("claude is not on PATH");
        }
        let _ = quiet("claude", &["mcp", "remove", "--scope", "user", "carrel"]);
        if remove {
            return Ok("removed carrel from Claude Code".into());
        }
        run("claude", &["mcp", "add", "--scope", "user", "carrel", "--", &exe, "mcp"])?;
        return Ok("Claude Code now has carrel, in every project".into());
    }
    let path = agent.config()?;
    let text = fs::read_to_string(&path).unwrap_or_default();
    let (new, changed) = match agent {
        Agent::Codex => edit_toml(&text, (!remove).then_some(exe.as_str())),
        _ => {
            let (section, entry) = agent.entry(&exe);
            edit_json(&text, section, (!remove).then_some(entry))
        }
    }
    .with_context(|| format!("editing {}", path.display()))?;
    if changed {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, new)?;
    }
    Ok(match (remove, changed) {
        (false, true) => format!("{}: added carrel to {}; restart it to see the tools", agent.name(), path.display()),
        (false, false) => format!("{}: already has carrel ({})", agent.name(), path.display()),
        (true, true) => format!("{}: removed carrel from {}", agent.name(), path.display()),
        (true, false) => format!("{}: didn't have carrel", agent.name()),
    })
}

/// Whether the agent has carrel; None when the agent isn't set up on this machine.
pub fn integrated(agent: Agent) -> Result<Option<bool>> {
    if agent == Agent::Claude {
        return Ok(quiet("claude", &["--version"]).then(|| quiet("claude", &["mcp", "get", "carrel"])));
    }
    let path = agent.config()?;
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(path.parent().unwrap().exists().then_some(false));
    };
    Ok(Some(match agent {
        Agent::Codex => text.lines().any(|l| l.trim() == CODEX_TABLE),
        _ => serde_json::from_str::<Value>(&text).is_ok_and(|v| !v[agent.entry("").0]["carrel"].is_null()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> Unit {
        Unit {
            exe: "/home/me/.local/bin/carrel".into(),
            args: Service::Consume.args("127.0.0.1:7700"),
            env: vec![("PATH".into(), "/usr/bin:/home/me/100%".into())],
            folders: vec!["/home/me/Down loads".into(), "/home/me/Papers".into()],
        }
    }

    #[test]
    fn systemd_units() {
        let units = systemd(Service::Consume, &unit());
        let names: Vec<&str> = units.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["carrel-consume.service", "carrel-consume.path", "carrel-consume.timer"]);
        assert!(units[0].1.contains("ExecStart=\"/home/me/.local/bin/carrel\" \"consume\"\n"));
        assert!(units[0].1.contains("Environment=\"PATH=/usr/bin:/home/me/100%%\"\n"));
        assert!(units[1].1.contains("PathChanged=/home/me/Down loads\nPathChanged=/home/me/Papers\n"));
        let web = Unit { args: Service::Web.args("127.0.0.1:7701"), ..unit() };
        let web = &systemd(Service::Web, &web)[0].1;
        assert!(web.contains("\"web\" \"--no-open\" \"--listen\" \"127.0.0.1:7701\"\nRestart=on-failure"));
    }

    #[test]
    fn launchd_agents() {
        let plist = launchd(Service::Consume, &unit(), Path::new("/Users/me/Library/Logs"));
        assert!(plist.contains("<string>io.github.arunoruto.carrel.consume</string>"));
        assert!(plist.contains("<string>/home/me/.local/bin/carrel</string>\n    <string>consume</string>"));
        assert!(plist.contains("<key>WatchPaths</key>\n  <array>\n    <string>/home/me/Down loads</string>"));
        assert!(plist.contains("<key>PATH</key><string>/usr/bin:/home/me/100%</string>"));
        assert!(plist.contains("/Users/me/Library/Logs/carrel-consume.log"));
        let web = launchd(Service::Web, &Unit { args: Service::Web.args("a&b"), ..unit() }, Path::new("/l"));
        assert!(web.contains("<string>a&amp;b</string>") && web.contains("<key>SuccessfulExit</key><false/>"));
        assert!(!web.contains("WatchPaths"));
    }

    #[test]
    fn json_configs() {
        let (section, entry) = Agent::Opencode.entry("/bin/carrel");
        let (text, changed) =
            edit_json("{\"theme\": \"x\", \"mcp\": {\"other\": {}}}", section, Some(entry.clone())).unwrap();
        assert!(changed);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mcp"]["carrel"]["command"], json!(["/bin/carrel", "mcp"]));
        assert_eq!(v["mcp"]["other"], json!({}));
        assert!(text.find("theme").unwrap() < text.find("mcp").unwrap(), "keys keep their order");
        assert!(!edit_json(&text, section, Some(entry)).unwrap().1, "already there");
        let (text, changed) = edit_json(&text, section, None).unwrap();
        assert!(changed && !text.contains("carrel"));
        let (text, _) = edit_json("", "mcpServers", Some(Agent::Gemini.entry("c").1)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["mcpServers"]["carrel"]["args"], json!(["mcp"]));
        assert!(edit_json("{ // a comment\n}", "mcp", None).is_err());
    }

    #[test]
    fn codex_config() {
        let before = "# mine\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        let (with, changed) = edit_toml(before, Some("/bin/car\"rel")).unwrap();
        assert!(changed && with.starts_with(before));
        let table: toml::Table = with.parse().unwrap();
        assert_eq!(table["mcp_servers"]["carrel"]["command"].as_str(), Some("/bin/car\"rel"));
        assert!(with.contains("command = \"x\"\n\n[mcp_servers.carrel]\n"));
        assert!(!edit_toml(&with, Some("/bin/car\"rel")).unwrap().1, "already there");
        let (moved, changed) = edit_toml(&with, Some("/usr/bin/carrel")).unwrap();
        assert!(changed && moved.matches(CODEX_TABLE).count() == 1 && moved.contains("/usr/bin/carrel"));
        let (without, changed) = edit_toml(&with, None).unwrap();
        assert!(changed);
        assert_eq!(without.trim_end(), before.trim_end());
        // Settings added to it by hand: kept when updating, gone with it.
        let env = format!("{with}\n[mcp_servers.carrel.env]\nCARREL_LIBRARY = \"/x\"\n");
        let (moved, _) = edit_toml(&env, Some("/usr/bin/carrel")).unwrap();
        let table: toml::Table = moved.parse().unwrap();
        assert_eq!(table["mcp_servers"]["carrel"]["env"]["CARREL_LIBRARY"].as_str(), Some("/x"));
        assert_eq!(edit_toml(&env, None).unwrap().0.trim_end(), before.trim_end());
        assert_eq!(edit_toml("", Some("c")).unwrap().0, "[mcp_servers.carrel]\ncommand = \"c\"\nargs = [\"mcp\"]\n");
    }
}
