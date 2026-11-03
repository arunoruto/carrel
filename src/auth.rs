//! Who is asking `carrel serve`, and what they may do. People log in with OpenID Connect (pocket-id,
//! Keycloak, ...): carrel runs the code flow with PKCE and asks the provider's userinfo endpoint, over
//! HTTPS, who logged in and in which groups, so it never has to check a token's signature itself. Group
//! `carrel` makes a member, `admin` an admin. Members make personal tokens for their machines (sync, MCP);
//! the server's own token counts as an admin. Trusted networks without either may read and add.
//!
//! Sessions and tokens are kept in the library's hidden `.carrel/`, as SHA-256 hashes: the files never hold
//! anything that lets one in.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long a login lasts.
const SESSION: Duration = Duration::from_secs(30 * 24 * 3600);
/// How long the way to the provider and back may take.
const PENDING: Duration = Duration::from_secs(600);
pub const COOKIE: &str = "carrel-session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Not let in.
    None,
    /// From a trusted network, without logging in.
    Anonymous,
    Member,
    Admin,
}

/// What a request needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Right {
    Read,
    /// New papers and files.
    Add,
    /// Changing what's there.
    Edit,
    /// Deleting papers.
    Admin,
}

impl Role {
    pub fn can(self, right: Right) -> bool {
        match right {
            Right::Read | Right::Add => self >= Role::Anonymous,
            Right::Edit => self >= Role::Member,
            Right::Admin => self == Role::Admin,
        }
    }

    fn of(groups: &[String]) -> Role {
        match () {
            _ if groups.iter().any(|g| g == "admin") => Role::Admin,
            _ if groups.iter().any(|g| g == "carrel") => Role::Member,
            _ => Role::None,
        }
    }
}

/// What an API call needs; the paths are the segments after `/api/`.
pub fn needed(method: &str, path: &[&str]) -> Right {
    match (method, path) {
        ("GET", ["tokens" | "activity", ..]) => Right::Edit,
        ("GET", _) => Right::Read,
        ("POST", ["activity", ..]) => Right::Admin,
        (
            "POST",
            ["add" | "upload" | "projects"] | ["recommended", _, "add"] | ["papers", _, "pdf" | "supplementary"],
        ) => Right::Add,
        // On a server deleting is only asked for; an admin decides.
        _ => Right::Edit,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Who {
    pub user: Option<String>,
    pub name: Option<String>,
    pub role: Role,
    /// The machine or person a client says it is (`X-Carrel-Client`), unchecked.
    #[serde(skip)]
    pub client: Option<String>,
    #[serde(skip)]
    groups: Vec<String>,
}

impl Who {
    pub fn role(role: Role) -> Self {
        Who { user: None, name: None, role, client: None, groups: vec![] }
    }

    pub fn named(user: &str, role: Role) -> Self {
        Who { user: Some(user.into()), name: None, role, client: None, groups: vec![] }
    }
}

struct Oidc {
    issuer: String,
    client_id: String,
    secret: String,
    /// `<url>/auth/callback`, as registered with the provider.
    redirect: String,
    endpoints: Mutex<Option<Endpoints>>,
}

#[derive(Clone, Deserialize)]
struct Endpoints {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    user: String,
    name: Option<String>,
    groups: Vec<String>,
    expires: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Token {
    id: String,
    name: String,
    user: String,
    groups: Vec<String>,
    hash: String,
    created: String,
    #[serde(default)]
    last: Option<Used>,
}

/// A token's last use.
#[derive(Clone, Serialize, Deserialize)]
struct Used {
    at: u64,
    from: Option<String>,
    /// The program: `carrel/0.6.0 sync`, `claude-code/2.1`, a browser.
    agent: Option<String>,
    /// The machine or person it said it was.
    client: Option<String>,
}

pub struct Auth {
    oidc: Option<Oidc>,
    /// The library's `.carrel/`.
    dir: PathBuf,
    /// The server's own token (`CARREL_SERVE_TOKEN`).
    server_token: Option<String>,
    /// Secure cookies, as browsers reach the server over HTTPS.
    pub secure: bool,
    sessions: Mutex<HashMap<String, Session>>,
    tokens: Mutex<Vec<Token>>,
    /// Logins on their way: state → (PKCE verifier, where to go after, since).
    pending: Mutex<HashMap<String, (String, String, Instant)>>,
}

impl Auth {
    /// `url` is where browsers reach the server; OIDC needs it for the way back.
    pub fn new(
        dir: PathBuf,
        oidc: Option<carrel::Oidc>,
        url: Option<&str>,
        server_token: Option<String>,
    ) -> Result<Self> {
        let oidc = match oidc {
            None => None,
            Some(c) => {
                let url =
                    url.context("[serve.oidc] needs [serve] url (or CARREL_SERVE_URL): where browsers reach carrel")?;
                let secret = match std::env::var("CARREL_OIDC_SECRET").ok().filter(|s| !s.trim().is_empty()) {
                    Some(secret) => secret.trim().to_string(),
                    None => {
                        c.client_secret.context("[serve.oidc] needs client_secret, or CARREL_OIDC_SECRET")?.read()?
                    }
                };
                Some(Oidc {
                    issuer: c.issuer.trim_end_matches('/').into(),
                    client_id: c.client_id,
                    secret,
                    redirect: format!("{}/auth/callback", url.trim_end_matches('/')),
                    endpoints: Mutex::new(None),
                })
            }
        };
        fn read<T: serde::de::DeserializeOwned + Default>(path: PathBuf) -> T {
            fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
        }
        let sessions: HashMap<String, Session> = read(dir.join("sessions.json"));
        let tokens: Vec<Token> = read(dir.join("tokens.json"));
        Ok(Auth {
            oidc,
            secure: url.is_some_and(|u| u.starts_with("https://")),
            dir,
            server_token,
            sessions: Mutex::new(sessions),
            tokens: Mutex::new(tokens),
            pending: Mutex::new(HashMap::new()),
        })
    }

    pub fn login(&self) -> bool {
        self.oidc.is_some()
    }

    /// Who sent a request: by its bearer token, its session cookie, or else the network it came from (`from`).
    pub fn who(&self, headers: &hyper::HeaderMap, trusted: bool, from: Option<std::net::IpAddr>) -> Who {
        let header = |name| headers.get(name).and_then(|v| v.to_str().ok()).map(|v| v.chars().take(100).collect());
        let bearer = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
        if let Some(sent) = bearer.map(str::trim) {
            if self.server_token.as_deref().is_some_and(|t| same(t, sent)) {
                return Who::named("server", Role::Admin);
            }
            let hash = sha256(sent);
            let mut tokens = self.tokens.lock().unwrap();
            if let Some(t) = tokens.iter_mut().find(|t| same(&t.hash, &hash)) {
                let who = Who { groups: t.groups.clone(), ..Who::named(&t.user, Role::of(&t.groups)) };
                // Where it was used last, written down at most once a minute.
                let stale = t.last.as_ref().is_none_or(|l| now().saturating_sub(l.at) >= 60);
                let from = from.map(|ip| ip.to_string());
                t.last = Some(Used { at: now(), from, agent: header("user-agent"), client: header("x-carrel-client") });
                if stale {
                    let _ = save(&self.dir.join("tokens.json"), &*tokens);
                }
                return who;
            }
        }
        let cookies = headers.get_all(hyper::header::COOKIE).iter().filter_map(|c| c.to_str().ok());
        let session = cookies.flat_map(|c| c.split(';')).find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")));
        if let Some(id) = session {
            let sessions = self.sessions.lock().unwrap();
            if let Some(s) = sessions.get(&sha256(id)).filter(|s| s.expires > now()) {
                let who = Who::named(&s.user, Role::of(&s.groups));
                return Who { name: s.name.clone(), groups: s.groups.clone(), ..who };
            }
        }
        Who::role(if trusted { Role::Anonymous } else { Role::None })
    }

    fn endpoints(&self, oidc: &Oidc) -> Result<Endpoints> {
        let mut cached = oidc.endpoints.lock().unwrap();
        if let Some(e) = &*cached {
            return Ok(e.clone());
        }
        let url = format!("{}/.well-known/openid-configuration", oidc.issuer);
        let mut response = carrel::fetch::agent().get(&url).call().with_context(|| format!("cannot read {url}"))?;
        let found: Endpoints =
            response.body_mut().read_json().with_context(|| format!("{url} is not what OIDC says"))?;
        *cached = Some(found.clone());
        Ok(found)
    }

    /// Where to send the browser to log in; `next` is where it lands afterwards.
    pub fn login_url(&self, next: &str) -> Result<String> {
        let oidc = self.oidc.as_ref().context("this server has no login; set [serve.oidc]")?;
        let endpoints = self.endpoints(oidc)?;
        let (state, verifier) = (random(16), random(32));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        // Only paths on this server: never a way to send someone elsewhere after logging in.
        let next = if next.starts_with('/') && !next.starts_with("//") { next } else { "/" };
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, (_, _, since)| since.elapsed() < PENDING);
        pending.insert(state.clone(), (verifier, next.into(), Instant::now()));
        let query = [
            ("response_type", "code"),
            ("client_id", &oidc.client_id),
            ("redirect_uri", &oidc.redirect),
            ("scope", "openid profile email groups"),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ];
        let query: Vec<String> = query.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
        let sep = if endpoints.authorization_endpoint.contains('?') { '&' } else { '?' };
        Ok(format!("{}{sep}{}", endpoints.authorization_endpoint, query.join("&")))
    }

    /// Back from the provider: trades the code for who logged in, and starts a session. Returns the session
    /// cookie's value and where to go.
    pub fn callback(&self, code: &str, state: &str) -> Result<(String, String)> {
        let oidc = self.oidc.as_ref().context("this server has no login")?;
        let (verifier, next, since) =
            self.pending.lock().unwrap().remove(state).context("that login is unknown or too old; log in again")?;
        if since.elapsed() > PENDING {
            bail!("that login took too long; log in again");
        }
        let endpoints = self.endpoints(oidc)?;
        let basic = base64::engine::general_purpose::STANDARD.encode(format!(
            "{}:{}",
            encode(&oidc.client_id),
            encode(&oidc.secret)
        ));
        let agent = carrel::fetch::agent();
        let mut response = agent
            .post(&endpoints.token_endpoint)
            .header("Authorization", &format!("Basic {basic}"))
            .send_form([
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &oidc.redirect),
                ("code_verifier", &verifier),
            ])
            .context("the provider refused the login")?;
        let tokens: Value = response.body_mut().read_json()?;
        let access = tokens["access_token"].as_str().context("the provider sent no access token")?;
        let mut response = agent
            .get(&endpoints.userinfo_endpoint)
            .header("Authorization", &format!("Bearer {access}"))
            .call()
            .context("the provider wouldn't say who logged in")?;
        let info: Value = response.body_mut().read_json()?;
        let user = info["preferred_username"].as_str().or(info["sub"].as_str()).context("no user in the login")?;
        let groups: Vec<String> =
            info["groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect();
        if Role::of(&groups) == Role::None {
            bail!("{user} is in neither of carrel's groups, carrel and admin; ask an admin of the login provider");
        }
        let id = random(32);
        let session = Session {
            user: user.into(),
            name: info["name"].as_str().map(String::from),
            groups,
            expires: now() + SESSION.as_secs(),
        };
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > now());
        sessions.insert(sha256(&id), session);
        save(&self.dir.join("sessions.json"), &*sessions)?;
        Ok((id, next))
    }

    pub fn logout(&self, headers: &hyper::HeaderMap) -> Result<()> {
        let cookies = headers.get_all(hyper::header::COOKIE).iter().filter_map(|c| c.to_str().ok());
        let ids: Vec<String> = cookies
            .flat_map(|c| c.split(';'))
            .filter_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")).map(sha256))
            .collect();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|hash, _| !ids.contains(hash));
        save(&self.dir.join("sessions.json"), &*sessions)
    }

    /// `Set-Cookie` for a session, or to end one with an empty `id`.
    pub fn cookie(&self, id: &str) -> String {
        let age = if id.is_empty() { 0 } else { SESSION.as_secs() };
        let secure = if self.secure { "; Secure" } else { "" };
        format!("{COOKIE}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}")
    }

    /// A member's tokens, or with `all` an admin's view of everyone's, without their secrets.
    pub fn tokens(&self, who: &Who, all: bool) -> Value {
        let tokens = self.tokens.lock().unwrap();
        let all = all && who.role == Role::Admin;
        let shown = tokens.iter().filter(|t| all || Some(&t.user) == who.user.as_ref());
        let entry = |t: &Token| serde_json::json!({"id": t.id, "name": t.name, "user": t.user, "created": t.created, "last": t.last});
        Value::Array(shown.map(entry).collect())
    }

    /// A new token for `who`, with their groups as they are now; the secret is in the answer only.
    pub fn create_token(&self, who: &Who, name: &str) -> Result<Value> {
        let user = who.user.clone().context("log in to make a token")?;
        let name = name.trim();
        if name.is_empty() || name.len() > 100 {
            bail!("give the token a name, such as the machine it is for");
        }
        let secret = format!("carrel_{}", random(30));
        let token = Token {
            id: random(6),
            name: name.into(),
            user,
            groups: who.groups.clone(),
            hash: sha256(&secret),
            created: date(now()),
            last: None,
        };
        let answer = serde_json::json!({"id": token.id, "name": token.name, "created": token.created, "token": secret});
        let mut tokens = self.tokens.lock().unwrap();
        tokens.push(token);
        save(&self.dir.join("tokens.json"), &*tokens)?;
        Ok(answer)
    }

    pub fn revoke_token(&self, who: &Who, id: &str) -> Result<()> {
        let mut tokens = self.tokens.lock().unwrap();
        let before = tokens.len();
        // An admin may take back anyone's.
        tokens.retain(|t| !(t.id == id && (Some(&t.user) == who.user.as_ref() || who.role == Role::Admin)));
        if tokens.len() == before {
            bail!("no such token of yours");
        }
        save(&self.dir.join("tokens.json"), &*tokens)
    }
}

fn save(path: &std::path::Path, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("tmp");
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// `n` random bytes, as hex.
pub fn random(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    use std::io::Read;
    fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).expect("/dev/urandom");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares every byte, so the time taken doesn't tell how much of a guess was right.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// Percent-encoding for query strings and forms: everything but RFC 3986's unreserved characters.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `YYYY-MM-DD` of a Unix time (UTC), by Howard Hinnant's days-to-civil.
fn date(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights() {
        assert!(Role::Anonymous.can(needed("GET", &["papers"])));
        assert!(Role::Anonymous.can(needed("POST", &["add"])));
        assert!(Role::Anonymous.can(needed("POST", &["papers", "a2020", "pdf"])));
        assert!(!Role::Anonymous.can(needed("PUT", &["papers", "a2020", "notes"])));
        assert!(!Role::Anonymous.can(needed("GET", &["tokens"])));
        assert!(Role::Member.can(needed("PUT", &["papers", "a2020", "notes"])));
        assert!(Role::Member.can(needed("DELETE", &["papers", "a2020"])), "asking to");
        assert!(!Role::Member.can(needed("POST", &["activity", "1-a", "accept"])));
        assert!(Role::Admin.can(needed("POST", &["activity", "1-a", "revert"])));
        assert!(!Role::Anonymous.can(needed("GET", &["activity"])));
        assert!(!Role::None.can(Right::Read));
        assert_eq!(Role::of(&["carrel".into()]), Role::Member);
        assert_eq!(Role::of(&["carrel".into(), "admin".into()]), Role::Admin);
        assert_eq!(Role::of(&["staff".into()]), Role::None);
    }

    #[test]
    fn helpers() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_793_646_761), "2026-11-02");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(encode("a b/c?d=é"), "a%20b%2Fc%3Fd%3D%C3%A9");
        assert_eq!(random(4).len(), 8);
    }
}
