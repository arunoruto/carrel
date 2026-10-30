//! Telling the user what carrel did while they weren't looking: papers it found or added, a server that
//! can't reach Ollama. Each way of telling is a [`Notifier`]: email, the desktop's notifications, Telegram.

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `[notify]` in the config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub email: Option<Email>,
    /// A notification on this computer's desktop: `notify-send` on Linux, Notification Center on macOS.
    #[serde(default)]
    pub desktop: bool,
    pub telegram: Option<Telegram>,
}

/// Mail through an SMTP server that takes it without login or TLS, like a university's internal relay.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Email {
    /// `host:port`; port 25 when left out.
    pub server: String,
    pub from: String,
    pub to: String,
}

/// A message from a Telegram bot: make one with @BotFather, send it a message, and its token and your chat id
/// (from `https://api.telegram.org/bot<token>/getUpdates`) go here.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telegram {
    pub token: crate::Secret,
    /// Your chat's number, or `@name` for a public channel.
    pub chat_id: ChatId,
    /// A self-hosted Bot API server instead of api.telegram.org.
    #[serde(default = "telegram_api")]
    pub api: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum ChatId {
    Number(i64),
    Name(String),
}

fn telegram_api() -> String {
    "https://api.telegram.org".into()
}

pub trait Notifier {
    /// What and where, for messages: `email to x@y.org`.
    fn name(&self) -> String;
    fn send(&self, subject: &str, body: &str) -> Result<()>;
}

/// The notifiers the config sets up.
pub fn notifiers(config: &Config) -> Vec<Box<dyn Notifier>> {
    let mut all: Vec<Box<dyn Notifier>> = vec![];
    if let Some(email) = &config.email {
        all.push(Box::new(email.clone()));
    }
    if config.desktop {
        all.push(Box::new(Desktop));
    }
    if let Some(telegram) = &config.telegram {
        all.push(Box::new(telegram.clone()));
    }
    all
}

/// Tell through every notifier; the ones that fail are returned with why.
pub fn send(config: &Config, subject: &str, body: &str) -> Vec<String> {
    let failed = notifiers(config).into_iter().filter_map(|n| n.send(subject, body).err().map(|e| (n.name(), e)));
    failed.map(|(name, e)| format!("{name}: {e:#}")).collect()
}

impl Notifier for Email {
    fn name(&self) -> String {
        format!("email to {}", self.to)
    }

    fn send(&self, subject: &str, body: &str) -> Result<()> {
        let server = if self.server.contains(':') { self.server.clone() } else { format!("{}:25", self.server) };
        let addr = server.to_socket_addrs()?.next().with_context(|| format!("cannot find {server}"))?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .with_context(|| format!("cannot reach {server}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut smtp = Smtp { reader: BufReader::new(stream.try_clone()?), writer: stream };
        smtp.expect(None, 220)?;
        if smtp.expect(Some(&format!("EHLO {}", hostname())), 250).is_err() {
            smtp.expect(Some(&format!("HELO {}", hostname())), 250)?;
        }
        smtp.expect(Some(&format!("MAIL FROM:<{}>", self.from)), 250)?;
        smtp.expect(Some(&format!("RCPT TO:<{}>", self.to)), 250)?;
        smtp.expect(Some("DATA"), 354)?;
        smtp.writer.write_all(message(&self.from, &self.to, subject, body).as_bytes())?;
        smtp.expect(None, 250)?;
        let _ = smtp.expect(Some("QUIT"), 221);
        Ok(())
    }
}

pub struct Desktop;

impl Notifier for Desktop {
    fn name(&self) -> String {
        "desktop".into()
    }

    fn send(&self, subject: &str, body: &str) -> Result<()> {
        use std::process::Command;
        // Text goes as arguments, never into a script, so nothing in a paper's title can run.
        let commands: Vec<Command> = match cfg!(target_os = "macos") {
            true => {
                let mut c = Command::new("osascript");
                let script = "on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run";
                c.args(["-e", script, subject, body]);
                vec![c]
            }
            false => {
                // The notification service over D-Bus: busctl comes with systemd, notify-send often isn't installed.
                let mut busctl = Command::new("busctl");
                busctl.args([
                    "--user",
                    "call",
                    "--",
                    "org.freedesktop.Notifications",
                    "/org/freedesktop/Notifications",
                ]);
                busctl.args(["org.freedesktop.Notifications", "Notify", "susssasa{sv}i", "carrel", "0"]);
                busctl.args(["accessories-dictionary", subject, body, "0", "0", "-1"]);
                let mut notify_send = Command::new("notify-send");
                notify_send.args(["--app-name=carrel", "--icon=accessories-dictionary", "--", subject, body]);
                vec![busctl, notify_send]
            }
        };
        let mut why = vec![];
        for mut command in commands {
            let program = command.get_program().to_string_lossy().into_owned();
            match command.output() {
                Ok(output) if output.status.success() => return Ok(()),
                Ok(output) => why.push(format!("{program} failed: {}", String::from_utf8_lossy(&output.stderr).trim())),
                Err(e) => why.push(format!("cannot run {program}: {e}")),
            }
        }
        bail!("{}", why.join("; "))
    }
}

impl Notifier for Telegram {
    fn name(&self) -> String {
        match &self.chat_id {
            ChatId::Number(n) => format!("telegram to {n}"),
            ChatId::Name(name) => format!("telegram to {name}"),
        }
    }

    fn send(&self, subject: &str, body: &str) -> Result<()> {
        let token = self.token.read()?;
        let url = format!("{}/bot{token}/sendMessage", self.api.trim_end_matches('/'));
        // Plain text: no parse mode, so titles with `_` or `<` arrive as written. Telegram takes 4096 characters.
        let text: String = format!("{subject}\n\n{}", body.trim_end()).chars().take(4096).collect();
        let message =
            serde_json::json!({"chat_id": self.chat_id, "text": text, "link_preview_options": {"is_disabled": true}});
        for attempt in 0..2 {
            let response = crate::fetch::agent()
                .post(&url)
                .config()
                .http_status_as_error(false)
                .build()
                .send_json(&message)
                // ureq's errors name the URL, which holds the token.
                .map_err(|e| anyhow::anyhow!("cannot reach {}: {}", self.api, e.to_string().replace(&token, "…")))?;
            let status = response.status().as_u16();
            let reply: serde_json::Value = response.into_body().read_json().unwrap_or_default();
            if reply["ok"] == true {
                return Ok(());
            }
            let wait = reply["parameters"]["retry_after"].as_u64();
            if status == 429 && attempt == 0 && wait.is_some_and(|s| s <= 30) {
                std::thread::sleep(Duration::from_secs(wait.unwrap_or(1)));
                continue;
            }
            let why = reply["description"].as_str().unwrap_or("no reason given").replace(&token, "…");
            bail!("Telegram refused the message ({status}): {why}");
        }
        unreachable!("the loop returns")
    }
}

struct Smtp {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Smtp {
    /// Send a command (if any) and read the reply, which must have the status `code` (or 251 for 250).
    fn expect(&mut self, command: Option<&str>, code: u16) -> Result<()> {
        if let Some(command) = command {
            write!(self.writer, "{command}\r\n")?;
        }
        // A reply's lines are `250-...` up to the last, `250 ...`.
        let mut reply = String::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                bail!("the mail server hung up");
            }
            reply.push_str(&line);
            if line.as_bytes().get(3) != Some(&b'-') {
                break;
            }
        }
        let status: u16 = reply.get(..3).and_then(|s| s.parse().ok()).unwrap_or(0);
        if status != code && !(code == 250 && status == 251) {
            let command = command.map_or("connecting".into(), |c| c.split(':').next().unwrap_or(c).to_string());
            bail!("the mail server refused {command}: {}", reply.trim());
        }
        Ok(())
    }
}

/// The message as sent after DATA, ending with the lone dot. The body goes as base64, which no server
/// mangles and which has no line starting with a dot.
fn message(from: &str, to: &str, subject: &str, body: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    // Encoded words for anything not ASCII, a few characters each so no header line gets too long.
    let subject = match subject.is_ascii() {
        true => subject.to_string(),
        false => {
            let chars: Vec<char> = subject.chars().collect();
            let words = chars.chunks(20).map(|c| format!("=?UTF-8?B?{}?=", b64.encode(c.iter().collect::<String>())));
            words.collect::<Vec<_>>().join("\r\n ")
        }
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let body = b64.encode(body.replace("\r\n", "\n").replace('\n', "\r\n"));
    let lines: Vec<&str> = body.as_bytes().chunks(76).map(|l| std::str::from_utf8(l).unwrap_or_default()).collect();
    format!(
        "From: carrel <{from}>\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {}\r\nMessage-ID: <{}.{}@carrel>\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n\
         {}\r\n.\r\n",
        date(now.as_secs()),
        now.as_nanos(),
        std::process::id(),
        lines.join("\r\n"),
    )
}

/// An email date, in UTC: `Wed, 28 Oct 2026 18:50:00 +0000`.
fn date(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let (weekday, month) = (WEEKDAYS[days.rem_euclid(7) as usize], MONTHS[month as usize - 1]);
    format!("{weekday}, {day} {month} {year} {h:02}:{m:02}:{s:02} +0000")
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; gethostname writes at most that and NUL-terminates
    // unless truncated, which the length cap below handles.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let name = std::ffi::CStr::from_bytes_until_nul(&buf).ok().and_then(|n| n.to_str().ok()).filter(|_| ok);
    name.filter(|n| !n.is_empty()).unwrap_or("localhost").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A Bot API that answers each request with the next of `replies`, and hands back what it was sent.
    fn fake_telegram(replies: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = vec![];
            for (status, reply) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut head, mut length) = (String::new(), 0);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                requests.push(head + &String::from_utf8(body).unwrap());
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (api, server)
    }

    fn bot(api: String) -> Telegram {
        Telegram { token: crate::Secret::Value("123:SECRET".into()), chat_id: ChatId::Number(42), api }
    }

    #[test]
    fn telegram() {
        let (api, server) = fake_telegram(vec![
            (429, r#"{"ok":false,"description":"Too Many Requests","parameters":{"retry_after":0}}"#),
            (200, r#"{"ok":true,"result":{}}"#),
        ]);
        bot(api).send("2 new papers", &format!("{}\n", "x".repeat(5000))).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2, "retried after 429");
        assert!(requests[1].starts_with("POST /bot123:SECRET/sendMessage "));
        let body: serde_json::Value = serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["chat_id"], 42);
        let text = body["text"].as_str().unwrap();
        assert!(text.starts_with("2 new papers\n\nxxx") && text.chars().count() == 4096, "cut to Telegram's limit");

        let (api, server) = fake_telegram(vec![(401, r#"{"ok":false,"description":"Unauthorized for 123:SECRET"}"#)]);
        let error = format!("{:#}", bot(api).send("s", "b").unwrap_err());
        server.join().unwrap();
        assert!(error.contains("Unauthorized") && !error.contains("SECRET"), "{error}");

        let error = format!("{:#}", bot("http://127.0.0.1:9".into()).send("s", "b").unwrap_err());
        assert!(!error.contains("SECRET"), "{error}");

        let config: Config =
            toml::from_str("desktop = true\n[telegram]\ntoken = { file = \"/t\" }\nchat_id = \"@chan\"").unwrap();
        let names: Vec<String> = notifiers(&config).iter().map(|n| n.name()).collect();
        assert_eq!(names, ["desktop", "telegram to @chan"]);
    }

    #[test]
    fn dates() {
        assert_eq!(date(0), "Thu, 1 Jan 1970 00:00:00 +0000");
        assert_eq!(date(1_803_840_600), "Sun, 28 Feb 2027 18:50:00 +0000");
        assert_eq!(date(951_782_400), "Tue, 29 Feb 2000 00:00:00 +0000", "a leap day");
    }

    #[test]
    fn messages() {
        let m = message("a@x.org", "b@y.org", "Größe", "line one\n.leading dot\n");
        assert!(m.contains("Subject: =?UTF-8?B?R3LDtsOfZQ==?=\r\n"));
        assert!(m.ends_with("\r\n.\r\n") && m.matches("\r\n.\r\n").count() == 1, "one end, no dot lines inside");
        let body = m.split("\r\n\r\n").nth(1).unwrap().trim_end_matches("\r\n.\r\n").replace("\r\n", "");
        let body = base64::engine::general_purpose::STANDARD.decode(body).unwrap();
        assert_eq!(String::from_utf8(body).unwrap(), "line one\r\n.leading dot\r\n");
    }
}
