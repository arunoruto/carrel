//! Helpers shared by the integration tests. Nothing here needs internet or Python.
#![allow(dead_code)]

use carrel::Library;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A temporary folder, deleted when dropped, also when a test fails.
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(name: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("carrel-{name}-{}-{nanos}", std::process::id()));
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
