//! Helpers shared by the integration tests. Nothing here needs internet or Python.
#![allow(dead_code)]

use carrel::Library;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A temporary folder, deleted when dropped, also when a test fails.
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(name: &str) -> Self {
        // The count tells apart tests starting at once: macOS clocks only tick in microseconds.
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("carrel-{name}-{}-{nanos}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    /// Write a file (creating its folders) and return its path.
    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Starts like a PDF, which is all carrel checks; the fake extractor does the rest.
pub const FAKE_PDF: &[u8] = b"%PDF-1.4\n% test file\n";

/// An extractor that writes `text` (no single quotes) and one figure: no Python, no internet.
pub fn fake_extractor(text: &str) -> String {
    format!("printf '%s\\n' '{text}' > fulltext.md && mkdir -p figures && printf png > figures/page-0001-00.png")
}

/// A library in `tmp/lib` with its search index in `tmp` (not in ~/.cache, which a build sandbox
/// may not have) and the fake extractor. Text without a heading, DOI or arXiv id keeps `add` offline.
pub fn library(tmp: &Tmp) -> Library {
    let mut lib = Library::open(tmp.path("lib")).unwrap();
    lib.index = tmp.path("index.sqlite");
    lib.extractor = fake_extractor("Plain text without a heading or identifier.");
    lib
}

/// A paper written straight into the library: `fields` is the inside of a BibTeX entry.
pub fn paper(lib: &Library, id: &str, fields: &str) {
    let dir = lib.root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{id}.bib")), format!("@article{{{id}, {fields}}}\n")).unwrap();
}

pub fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

/// An Ollama stand-in serving `models` (name, whether it embeds) at the returned URL. Its embeddings put
/// each word in one of 64 dimensions, so texts sharing words are close. Counts the texts it embedded.
pub fn fake_ollama(models: &[(&str, bool)]) -> (String, std::sync::Arc<AtomicUsize>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let models: Vec<(String, bool)> = models.iter().map(|(m, e)| (m.to_string(), *e)).collect();
    let embedded = std::sync::Arc::new(AtomicUsize::new(0));
    let count = embedded.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut head, mut length) = (String::new(), 0);
            reader.read_line(&mut head).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let path = head.split_whitespace().nth(1).unwrap_or_default();
            let (status, reply) = match path {
                "/api/tags" => {
                    let names: Vec<_> =
                        models.iter().map(|(m, _)| serde_json::json!({"name": format!("{m}:latest")})).collect();
                    (200, serde_json::json!({ "models": names }))
                }
                "/api/show" => match models.iter().find(|(m, _)| body["model"] == m.as_str()) {
                    Some((_, embeds)) => {
                        let capabilities = if *embeds { vec!["embedding"] } else { vec!["completion"] };
                        (200, serde_json::json!({ "capabilities": capabilities }))
                    }
                    None => (404, serde_json::json!({ "error": "model not found" })),
                },
                "/api/embed" => {
                    let texts: Vec<String> = serde_json::from_value(body["input"].clone()).unwrap();
                    count.fetch_add(texts.len(), Ordering::Relaxed);
                    let vectors: Vec<Vec<f32>> = texts
                        .iter()
                        .map(|text| {
                            let mut v = vec![0.0; 64];
                            for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
                                let hash = word
                                    .to_lowercase()
                                    .bytes()
                                    .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
                                v[hash as usize % 64] += 1.0;
                            }
                            v
                        })
                        .collect();
                    (200, serde_json::json!({ "embeddings": vectors }))
                }
                _ => (404, serde_json::json!({ "error": "not found" })),
            };
            let reply = reply.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (url, embedded)
}

/// A mail server taking mail without login, as a university relay does. Returns its address and the
/// messages it got, each as everything after DATA up to the lone dot.
pub fn fake_smtp() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let mails = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let got = mails.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            stream.write_all(b"220 test ESMTP\r\n").unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let reply: &[u8] = match line.trim().split(' ').next().unwrap_or_default() {
                    "EHLO" => b"250-test\r\n250 8BITMIME\r\n",
                    "DATA" => {
                        stream.write_all(b"354 go on\r\n").unwrap();
                        let mut mail = String::new();
                        loop {
                            let mut l = String::new();
                            reader.read_line(&mut l).unwrap();
                            if l == ".\r\n" {
                                break;
                            }
                            mail.push_str(&l);
                        }
                        got.lock().unwrap().push(mail);
                        b"250 queued\r\n"
                    }
                    "QUIT" => b"221 bye\r\n",
                    _ => b"250 ok\r\n",
                };
                stream.write_all(reply).unwrap();
                line.clear();
            }
        }
    });
    (addr, mails)
}

/// The text of a mail from carrel: its base64 body, decoded.
pub fn mail_body(mail: &str) -> String {
    use base64::Engine;
    let body = mail.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(body).unwrap()).unwrap()
}
