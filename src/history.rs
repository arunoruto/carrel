//! What changed in a server's library, by whom, and the way back. Every change through the web UI, its API or
//! `/sync` is applied at once and kept here: the files it touched as they were before and after, so an admin
//! can revert it for [`KEEP`]. Deleting goes the other way round: it is only asked for, and an admin other
//! than the one asking accepts it, which moves the files to the archive, from where they can come back until
//! they too are older than [`KEEP`].
//!
//! Everything lives in the library's hidden `.carrel/`: `history/log.json` and, per entry,
//! `history/<entry>/{before,after}/<path>`; deleted files in `archive/<entry>/<path>`. Hidden, so never synced.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long changes stay revertible and deleted files restorable.
pub const KEEP: u64 = 30 * 24 * 3600;
/// Changes by the same person to the same thing within this make one entry.
const MERGE: u64 = 10 * 60;
/// Files up to this size are kept as they were after a change too, for diffs.
const AFTER: u64 = 1 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Seconds since 1970.
    pub at: u64,
    pub who: String,
    /// The machine or person the client said it was, unchecked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    pub what: String,
    /// The paper's id, when it was about one.
    pub paper: Option<String>,
    pub files: Vec<Touched>,
    /// For deletions: `pending`, `accepted`, `declined` or `restored`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Who reverted the change, or decided on the deletion, and when.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Touched {
    pub path: String,
    /// `added`, `changed` or `removed`; for deletions `delete`.
    pub change: String,
    /// The file's MD5 after the change, to tell whether a later one touched it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl Entry {
    fn is_deletion(&self) -> bool {
        self.status.is_some()
    }
}

/// The files under some folders of the library as they were before a change: the scope to compare with after.
pub struct Before {
    who: String,
    client: Option<String>,
    what: String,
    paper: Option<String>,
    folders: Vec<String>,
    /// Top-level folders then, to notice new papers.
    tops: Vec<String>,
    /// Path → MD5.
    files: BTreeMap<String, String>,
    /// Where copies of the files went.
    copies: PathBuf,
}

pub struct History {
    root: PathBuf,
    log: Mutex<Vec<Entry>>,
}

impl History {
    pub fn open(root: &Path) -> Self {
        let log = fs::read(root.join(".carrel/history/log.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        History { root: root.into(), log: Mutex::new(log.unwrap_or_default()) }
    }

    fn dir(&self) -> PathBuf {
        self.root.join(".carrel").join("history")
    }

    fn save(&self, log: &[Entry]) -> Result<()> {
        fs::create_dir_all(self.dir())?;
        let tmp = self.dir().join("log.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(log)?)?;
        fs::rename(tmp, self.dir().join("log.json"))?;
        Ok(())
    }

    /// The newest entries first.
    pub fn entries(&self, limit: usize) -> Vec<Entry> {
        self.log.lock().unwrap().iter().rev().take(limit).cloned().collect()
    }

    /// Notes the files under `folders` (top-level folders of the library, such as a paper's) before a change.
    pub fn before(
        &self,
        who: &str,
        client: Option<&str>,
        what: &str,
        paper: Option<&str>,
        folders: &[&str],
    ) -> Result<Before> {
        let copies = self.dir().join(format!(".new-{}", crate::auth::random(8)));
        let mut files = BTreeMap::new();
        for folder in folders {
            for path in under(&self.root, folder)? {
                let bytes = fs::read(self.root.join(&path))?;
                files.insert(path.clone(), carrel::remote::md5_hex(&bytes));
                let to = copies.join("before").join(&path);
                fs::create_dir_all(to.parent().unwrap())?;
                fs::write(to, bytes)?;
            }
        }
        Ok(Before {
            who: who.into(),
            client: client.map(String::from),
            what: what.into(),
            paper: paper.map(String::from),
            folders: folders.iter().map(|f| f.to_string()).collect(),
            tops: tops(&self.root),
            files,
            copies,
        })
    }

    /// After the change: records what it did, if anything.
    pub fn after(&self, before: Before) -> Result<()> {
        let mut now = BTreeMap::new();
        let mut folders = before.folders.clone();
        folders.extend(tops(&self.root).into_iter().filter(|t| !before.tops.contains(t)));
        for folder in &folders {
            for path in under(&self.root, folder)? {
                now.insert(path.clone(), carrel::remote::md5_hex(&fs::read(self.root.join(&path))?));
            }
        }
        let mut touched = vec![];
        for (path, md5) in &now {
            match before.files.get(path) {
                Some(old) if old == md5 => {}
                Some(_) => {
                    touched.push(Touched { path: path.clone(), change: "changed".into(), after: Some(md5.clone()) })
                }
                None => touched.push(Touched { path: path.clone(), change: "added".into(), after: Some(md5.clone()) }),
            }
        }
        for path in before.files.keys().filter(|p| !now.contains_key(*p)) {
            touched.push(Touched { path: path.clone(), change: "removed".into(), after: None });
        }
        if touched.is_empty() {
            let _ = fs::remove_dir_all(&before.copies);
            return Ok(());
        }
        let paper = before.paper.clone().or_else(|| {
            let firsts: Vec<&str> = touched.iter().filter_map(|t| t.path.split_once('/').map(|(f, _)| f)).collect();
            firsts.first().filter(|f| firsts.iter().all(|g| g == *f)).map(|f| f.to_string())
        });
        let mut log = self.log.lock().unwrap();
        let at = now_secs();
        // The same person going on with the same thing: one entry, with the oldest "before" of each file.
        let merge = log.last_mut().filter(|e| {
            !e.is_deletion()
                && e.decided.is_none()
                && e.who == before.who
                && e.client == before.client
                && e.what == before.what
                && e.paper == paper
                && at - e.at < MERGE
        });
        let (entry, fresh) = match merge {
            Some(entry) => (entry, false),
            None => {
                let id = format!("{at}-{}", crate::auth::random(3));
                let entry = Entry {
                    id,
                    at,
                    who: before.who.clone(),
                    client: before.client.clone(),
                    what: before.what.clone(),
                    paper,
                    files: vec![],
                    status: None,
                    decided: None,
                };
                log.push(entry);
                (log.last_mut().unwrap(), true)
            }
        };
        let dir = self.dir().join(&entry.id);
        for t in touched {
            let kept = dir.join("before").join(&t.path);
            match entry.files.iter_mut().find(|e| e.path == t.path) {
                // Seen before in this entry: its "before" stays the older one.
                Some(seen) => {
                    seen.after = t.after.clone();
                    seen.change = match (seen.change.as_str(), t.change.as_str()) {
                        ("added", "removed") => "removed".into(),
                        ("added", _) => "added".into(),
                        (_, "removed") => "removed".into(),
                        _ => "changed".into(),
                    };
                }
                None => {
                    let copy = before.copies.join("before").join(&t.path);
                    if copy.exists() {
                        fs::create_dir_all(kept.parent().unwrap())?;
                        fs::rename(copy, &kept)?;
                    }
                    entry.files.push(t.clone());
                }
            }
            let after = dir.join("after").join(&t.path);
            let _ = fs::remove_file(&after);
            let file = self.root.join(&t.path);
            if t.after.is_some() && fs::metadata(&file).is_ok_and(|m| m.len() <= AFTER) {
                fs::create_dir_all(after.parent().unwrap())?;
                fs::copy(&file, &after)?;
            }
        }
        if !fresh {
            entry.at = at;
        }
        let _ = fs::remove_dir_all(&before.copies);
        self.save(&log)
    }

    /// A text file before and after a change, for a diff.
    pub fn diff(&self, id: &str, path: &str) -> Result<(Option<String>, Option<String>)> {
        let log = self.log.lock().unwrap();
        log.iter().find(|e| e.id == id && e.files.iter().any(|f| f.path == path)).context("no such change")?;
        let read = |side| {
            fs::read(self.dir().join(id).join(side).join(path)).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        Ok((read("before"), read("after")))
    }

    /// Undoes a change, or brings back what a deletion archived. Refused for a file changed again since: that
    /// later change comes first.
    pub fn revert(&self, id: &str, by: &str) -> Result<()> {
        let mut log = self.log.lock().unwrap();
        let entry = log.iter().find(|e| e.id == id).context("no such change")?.clone();
        if entry.is_deletion() {
            if entry.status.as_deref() != Some("accepted") {
                bail!("only accepted deletions can be undone");
            }
            for t in &entry.files {
                let (from, to) = (self.root.join(".carrel/archive").join(id).join(&t.path), self.root.join(&t.path));
                if to.exists() {
                    bail!("{} is there again; move it away first", t.path);
                }
                if from.exists() {
                    fs::create_dir_all(to.parent().unwrap())?;
                    fs::rename(from, to)?;
                }
            }
            let e = log.iter_mut().find(|e| e.id == id).unwrap();
            e.status = Some("restored".into());
            e.decided = Some((by.into(), now_secs()));
            return self.save(&log);
        }
        if entry.decided.is_some() {
            bail!("this change was reverted already");
        }
        for t in &entry.files {
            let file = self.root.join(&t.path);
            let current = fs::read(&file).ok().map(|b| carrel::remote::md5_hex(&b));
            if current != t.after {
                bail!("{} changed again since; revert that first", t.path);
            }
        }
        for t in &entry.files {
            let (file, kept) = (self.root.join(&t.path), self.dir().join(id).join("before").join(&t.path));
            match t.change.as_str() {
                "added" => {
                    fs::remove_file(&file)?;
                    carrel::remote::remove_empty_parents(&file, &self.root);
                }
                _ => {
                    fs::create_dir_all(file.parent().unwrap())?;
                    fs::copy(&kept, &file).with_context(|| format!("the old {} is gone", t.path))?;
                }
            }
        }
        let e = log.iter_mut().find(|e| e.id == id).unwrap();
        e.decided = Some((by.into(), now_secs()));
        self.save(&log)
    }

    /// Asks to delete files; while one person's request for a paper is open, more files join it. Returns
    /// whether a deletion of these was declined lately, in which case nothing is asked.
    pub fn request_deletion(&self, who: &str, paper: Option<&str>, paths: &[String]) -> Result<bool> {
        let mut log = self.log.lock().unwrap();
        let declined = log
            .iter()
            .any(|e| e.status.as_deref() == Some("declined") && e.files.iter().any(|f| paths.contains(&f.path)));
        if declined {
            return Ok(true);
        }
        let open = log
            .iter_mut()
            .rev()
            .find(|e| e.status.as_deref() == Some("pending") && e.who == who && e.paper.as_deref() == paper);
        let entry = match open {
            Some(entry) => entry,
            None => {
                let at = now_secs();
                log.push(Entry {
                    id: format!("{at}-{}", crate::auth::random(3)),
                    at,
                    who: who.into(),
                    client: None,
                    what: "asked to delete".into(),
                    paper: paper.map(String::from),
                    files: vec![],
                    status: Some("pending".into()),
                    decided: None,
                });
                log.last_mut().unwrap()
            }
        };
        for path in paths {
            if !entry.files.iter().any(|f| &f.path == path) {
                entry.files.push(Touched { path: path.clone(), change: "delete".into(), after: None });
            }
        }
        self.save(&log)?;
        Ok(false)
    }

    /// Accepts a deletion, archiving its files, or declines it. Someone else than who asked has to decide.
    pub fn decide(&self, id: &str, by: &str, accept: bool) -> Result<()> {
        let mut log = self.log.lock().unwrap();
        let entry = log.iter_mut().find(|e| e.id == id).context("no such request")?;
        if entry.status.as_deref() != Some("pending") {
            bail!("this request was decided already");
        }
        if entry.who == by {
            bail!("someone else has to decide on your own request");
        }
        if accept {
            for t in &entry.files {
                let (from, to) = (self.root.join(&t.path), self.root.join(".carrel/archive").join(id).join(&t.path));
                if from.exists() {
                    fs::create_dir_all(to.parent().unwrap())?;
                    fs::rename(&from, &to)?;
                    carrel::remote::remove_empty_parents(&from, &self.root);
                }
            }
        }
        entry.status = Some(if accept { "accepted" } else { "declined" }.into());
        entry.decided = Some((by.into(), now_secs()));
        self.save(&log)
    }

    /// Forgets what is older than [`KEEP`], with its copies and archived files.
    pub fn purge(&self) -> Result<usize> {
        let mut log = self.log.lock().unwrap();
        let cutoff = now_secs().saturating_sub(KEEP);
        let old: Vec<String> = log
            .iter()
            .filter(|e| e.at < cutoff && e.status.as_deref() != Some("pending"))
            .map(|e| e.id.clone())
            .collect();
        for id in &old {
            let _ = fs::remove_dir_all(self.dir().join(id));
            let _ = fs::remove_dir_all(self.root.join(".carrel/archive").join(id));
        }
        log.retain(|e| !old.contains(&e.id));
        if !old.is_empty() {
            self.save(&log)?;
        }
        Ok(old.len())
    }
}

/// The files under a top-level folder of the library, skipping figures (they come again from the PDF) and
/// hidden ones; a file's path is that file alone.
fn under(root: &Path, folder: &str) -> Result<Vec<String>> {
    let dir = root.join(folder);
    if dir.is_file() || (!dir.exists() && folder.contains('/')) {
        return Ok(vec![folder.to_string()].into_iter().filter(|_| dir.is_file()).collect());
    }
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let files = carrel::remote::files(&dir)?;
    Ok(files.into_iter().filter(|p| !p.starts_with("figures/")).map(|p| format!("{folder}/{p}")).collect())
}

fn tops(root: &Path) -> Vec<String> {
    let entries = fs::read_dir(root).into_iter().flatten().flatten();
    entries
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (PathBuf, History) {
        let root =
            std::env::temp_dir().join(format!("carrel-history-{}-{}", std::process::id(), crate::auth::random(4)));
        fs::create_dir_all(root.join("a2020")).unwrap();
        fs::write(root.join("a2020/a2020.bib"), "old").unwrap();
        let history = History::open(&root);
        (root, history)
    }

    #[test]
    fn changes_and_reverts() {
        let (root, history) = library();
        let before = history.before("alice", None, "edited the notes", Some("a2020"), &["a2020"]).unwrap();
        fs::write(root.join("a2020/a2020.bib"), "new").unwrap();
        fs::write(root.join("a2020/notes.md"), "hi").unwrap();
        history.after(before).unwrap();
        // More of the same within minutes joins it; nothing changed makes no entry.
        let before = history.before("alice", None, "edited the notes", Some("a2020"), &["a2020"]).unwrap();
        fs::write(root.join("a2020/notes.md"), "hello").unwrap();
        history.after(before).unwrap();
        history.after(history.before("bob", None, "looked", None, &["a2020"]).unwrap()).unwrap();
        let entries = history.entries(10);
        assert_eq!(entries.len(), 1, "{entries:?}");
        let changes: Vec<_> = entries[0].files.iter().map(|f| (f.path.as_str(), f.change.as_str())).collect();
        assert_eq!(changes, [("a2020/a2020.bib", "changed"), ("a2020/notes.md", "added")]);
        assert_eq!(history.diff(&entries[0].id, "a2020/a2020.bib").unwrap(), (Some("old".into()), Some("new".into())));

        // A file changed again since blocks the revert; then it goes back, and only once.
        fs::write(root.join("a2020/notes.md"), "later").unwrap();
        assert!(history.revert(&entries[0].id, "admin").unwrap_err().to_string().contains("changed again"));
        fs::write(root.join("a2020/notes.md"), "hello").unwrap();
        history.revert(&entries[0].id, "admin").unwrap();
        assert_eq!(fs::read_to_string(root.join("a2020/a2020.bib")).unwrap(), "old");
        assert!(!root.join("a2020/notes.md").exists());
        assert!(history.revert(&entries[0].id, "admin").is_err());

        // One file alone, as a sync uploads it.
        let before = history.before("bob", None, "synced", Some("a2020"), &["a2020/a2020.bib"]).unwrap();
        fs::write(root.join("a2020/a2020.bib"), "synced").unwrap();
        history.after(before).unwrap();
        assert_eq!(history.entries(1)[0].files[0].change, "changed");
        let before = history.before("bob", None, "synced", Some("a2020"), &["a2020/new.md"]).unwrap();
        fs::write(root.join("a2020/new.md"), "n").unwrap();
        history.after(before).unwrap();
        assert_eq!(history.entries(1)[0].files.len(), 2, "merged, with the new file added");

        // A new paper shows up as added, though it wasn't in the scope.
        let before = history.before("alice", None, "added a paper", None, &[]).unwrap();
        fs::create_dir_all(root.join("b2021")).unwrap();
        fs::write(root.join("b2021/b2021.bib"), "b").unwrap();
        history.after(before).unwrap();
        assert_eq!(history.entries(1)[0].paper.as_deref(), Some("b2021"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletions() {
        let (root, history) = library();
        let paths = vec!["a2020/a2020.bib".to_string()];
        assert!(!history.request_deletion("alice", Some("a2020"), &paths).unwrap());
        assert!(!history.request_deletion("alice", Some("a2020"), &paths).unwrap(), "asking again is the same request");
        let id = history.entries(10)[0].id.clone();
        assert_eq!(history.entries(10).len(), 1);
        assert!(history.decide(&id, "alice", true).unwrap_err().to_string().contains("someone else"));
        history.decide(&id, "bob", true).unwrap();
        assert!(!root.join("a2020").exists(), "archived, and no empty folder left");
        assert!(root.join(".carrel/archive").join(&id).join("a2020/a2020.bib").exists());
        history.revert(&id, "bob").unwrap();
        assert_eq!(fs::read_to_string(root.join("a2020/a2020.bib")).unwrap(), "old");

        // Declined: asking again for the same files is answered with no.
        assert!(!history.request_deletion("carol", Some("a2020"), &paths).unwrap());
        let id = history.entries(1)[0].id.clone();
        history.decide(&id, "bob", false).unwrap();
        assert!(history.request_deletion("carol", Some("a2020"), &paths).unwrap());
        assert!(root.join("a2020/a2020.bib").exists());
        assert_eq!(History::open(&root).entries(10).len(), 2, "kept across restarts");
        fs::remove_dir_all(root).unwrap();
    }
}
