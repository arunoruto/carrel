//! Papers turn up elsewhere first, mostly in Downloads. `carrel consume` lists the new PDFs there and
//! moves the ones the user picks into the library. Unattended (from a timer, or in `carrel serve`) it adds
//! those it can identify when `auto` is on, else it only tells about them, through the notifiers.

use crate::{Library, xdg_dir};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `[consume]` in the config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Folders to look in; the Downloads folder by default.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Add the PDFs it can identify as papers without asking, and tell what it did.
    #[serde(default)]
    pub auto: bool,
    /// Only PDFs changed in the last this many days (30 by default), so years of downloads don't pour in.
    pub days: Option<u64>,
}

impl Config {
    pub fn folders(&self) -> Vec<PathBuf> {
        match self.folders.is_empty() {
            true => dirs::download_dir().into_iter().collect(),
            false => self.folders.iter().map(|f| crate::expand_home(f)).collect(),
        }
    }

    pub fn days(&self) -> u64 {
        self.days.unwrap_or(30)
    }
}

/// A PDF found in one of the folders.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    size: u64,
    modified: u64,
    /// Why it looks like a paper (`DOI 10.1029/... in the file`), when it does. A hint only; adding
    /// decides from the text.
    pub clue: Option<String>,
}

/// The PDFs in `folders` (not their subfolders) changed in the last `days` days, oldest first.
pub fn find(folders: &[PathBuf], days: u64) -> Vec<Found> {
    let since = SystemTime::now() - Duration::from_secs(days.saturating_mul(86400).min(1 << 40));
    let mut found: Vec<Found> = folders
        .iter()
        .flat_map(|dir| fs::read_dir(dir).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")))
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            let modified = meta.modified().ok().filter(|m| *m >= since)?;
            let modified = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
            let path = e.path();
            Some(Found { clue: clue(&path), path, size: meta.len(), modified })
        })
        .collect();
    found.sort_by_key(|f| f.modified);
    found
}

/// An arXiv id in the file name (`2301.01234v2.pdf`), or a DOI or "arXiv" in the parts of the file that
/// aren't compressed (its metadata, mostly; the DOI may also be one it cites).
fn clue(path: &Path) -> Option<String> {
    let name = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Some(id) = crate::fetch::arxiv_id(&name) {
        return Some(format!("arXiv id {id} in its name"));
    }
    let bytes = fs::read(path).ok()?;
    let lower = bytes.to_ascii_lowercase();
    // "doi:10.1029/...", "doi.org/10.1029/...", "<prism:doi>10.1029/..."
    let doi = lower.windows(3).enumerate().find_map(|(i, w)| {
        let rest = &lower[i + 3..lower.len().min(i + 40)];
        let s = rest.windows(3).position(|w| w == b"10.").filter(|_| w == b"doi")?;
        let digits = rest[s + 3..].iter().take_while(|c| c.is_ascii_digit()).count();
        (digits >= 4 && rest.get(s + 3 + digits) == Some(&b'/')).then_some(i + 3 + s)
    });
    if let Some(start) = doi {
        let doi = bytes[start..].iter().take_while(|c| c.is_ascii_graphic() && !b"\"<>)]\\".contains(c)).take(60);
        let doi: String = doi.map(|&c| c as char).collect();
        return Some(format!("DOI {} in the file", doi.trim_end_matches(['.', ',', ';'])));
    }
    lower.windows(5).any(|w| w == b"arxiv").then(|| "mentions arXiv".into())
}

/// What became of the PDFs seen so far, so each is asked or told about once.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Seen {
    files: BTreeMap<String, Mark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    /// Told about in a notification, or tried unattended; still offered by `carrel consume`.
    Told,
    /// The user said no; not offered again.
    Skipped,
}

impl Seen {
    fn path() -> Option<PathBuf> {
        let dir = xdg_dir("XDG_STATE_HOME", dirs::state_dir().or_else(dirs::data_local_dir));
        dir.map(|dir| dir.join("carrel").join("consume.json"))
    }

    pub fn load() -> Self {
        let text = Self::path().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Saves, forgetting files that are gone.
    pub fn save(&mut self) -> Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        self.files.retain(|key, _| key.split('\t').next().is_some_and(|p| Path::new(p).exists()));
        fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// A changed file counts as new.
    fn key(f: &Found) -> String {
        format!("{}\t{}\t{}", f.path.display(), f.size, f.modified)
    }

    pub fn get(&self, f: &Found) -> Option<Mark> {
        self.files.get(&Self::key(f)).copied()
    }

    pub fn set(&mut self, f: &Found, mark: Mark) {
        self.files.insert(Self::key(f), mark);
    }
}

impl Library {
    /// Move a found PDF into the library: add it, then delete the original. With `known_only`, PDFs
    /// without a DOI or arXiv id are refused.
    pub fn consume(&self, f: &Found, known_only: bool) -> Result<(String, Vec<String>)> {
        let (id, mut messages) = self.add_from(&f.path.to_string_lossy(), known_only)?;
        if let Err(e) = fs::remove_file(&f.path) {
            messages.push(format!("cannot delete {} after adding it: {e}", f.path.display()));
        }
        Ok((id, messages))
    }
}

/// `carrel consume` without anyone to ask: adds the PDFs it can identify when `auto` is on, and returns
/// a message about the PDFs new since last time (subject and body), if there are any. With `again`, PDFs
/// told about before count as new.
pub fn unattended(lib: &Library, config: &Config, auto: bool, again: bool) -> Result<Option<(String, String)>> {
    let mut seen = Seen::load();
    let new: Vec<Found> = find(&config.folders(), config.days())
        .into_iter()
        .filter(|f| seen.get(f).is_none() || again && seen.get(f) == Some(Mark::Told))
        .collect();
    if new.is_empty() {
        return Ok(None);
    }
    let (mut added, mut left) = (vec![], vec![]);
    for f in &new {
        seen.set(f, Mark::Told);
        if !auto {
            let hint = f.clue.as_ref().map_or(String::new(), |clue| format!("  ({clue})"));
            left.push(format!("  {}{hint}", f.path.display()));
            continue;
        }
        match lib.consume(f, true) {
            Ok((id, messages)) => {
                let title = lib.get(&id).map(|p| p.bib.title()).unwrap_or_default();
                added.push(format!("  {id}  {title}\n    from {}", f.path.display()));
                added.extend(messages.iter().map(|m| format!("    {m}")));
            }
            Err(e) => left.push(format!("  {}: {e:#}", f.path.display())),
        }
    }
    seen.save()?;
    let mut body = vec![];
    if !added.is_empty() {
        body.push(format!("Added to the library:\n{}", added.join("\n")));
    }
    if !left.is_empty() {
        let heading = if auto { "Not added" } else { "New, waiting for `carrel consume`" };
        body.push(format!("{heading}:\n{}", left.join("\n")));
    }
    let subject = match (added.len(), left.len()) {
        (0, n) => format!("carrel found {n} new PDF{}", if n == 1 { "" } else { "s" }),
        (n, _) => format!("carrel added {n} paper{}", if n == 1 { "" } else { "s" }),
    };
    Ok(Some((subject, body.join("\n\n") + "\n")))
}
