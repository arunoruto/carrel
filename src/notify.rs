//! Telling the user what carrel did while they weren't looking: papers it found or added, a server that
//! can't reach Ollama. Each way of telling is a [`Notifier`]; email is the first.

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

pub trait Notifier {
    /// What and where, for messages: `email to x@y.org`.
    fn name(&self) -> String;
    fn send(&self, subject: &str, body: &str) -> Result<()>;
}

/// The notifiers the config sets up.
pub fn notifiers(config: &Config) -> Vec<Box<dyn Notifier>> {
    config.email.iter().map(|e| Box::new(e.clone()) as Box<dyn Notifier>).collect()
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
