//! Sharing a library through a remote: an S3 bucket (Garage, MinIO, AWS...) or a plain folder.
//!
//! Every file of the library is synced, except hidden ones (`.tmp`, `.carrel`, `.git`, ...). What both
//! sides held at the last sync is kept in `.carrel/sync.json`, so a sync knows which side changed a
//! file: changes go the other way, an edit beats a deletion, and a file changed on both sides is a
//! conflict left alone until `--keep local` or `--keep remote` settles it.

use crate::{Library, Secret};
use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use rusty_s3::S3Action;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

/// Where a library is shared; `[remote]` in the config.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum RemoteConfig {
    /// An S3 bucket. `AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`
    /// override the settings.
    S3 {
        endpoint: Option<String>,
        region: Option<String>,
        bucket: String,
        /// A folder in the bucket, to share one bucket between libraries.
        #[serde(default)]
        prefix: String,
        access_key: Option<String>,
        secret_key: Option<Secret>,
    },
    /// A folder, e.g. on a network drive or a USB stick.
    Folder { path: String },
}

/// Somewhere files can be stored by their path in the library, `a2020/a2020.pdf`.
pub trait Remote: Sync {
    /// Where, for messages; also tells the sync state of one remote from another's.
    fn name(&self) -> String;
    /// Every file and its version tag (its ETag: for files uploaded in one piece, their MD5).
    fn list(&self) -> Result<BTreeMap<String, String>>;
    fn download(&self, path: &str, to: &Path) -> Result<()>;
    /// Uploads `bytes` and returns the new version tag.
    fn upload(&self, path: &str, bytes: Vec<u8>) -> Result<String>;
    fn delete(&self, path: &str) -> Result<()>;
}

impl RemoteConfig {
    pub fn open(&self) -> Result<Box<dyn Remote>> {
        let env = |var: &str| std::env::var(var).ok().filter(|v| !v.trim().is_empty());
        Ok(match self {
            Self::Folder { path } => Box::new(Folder(crate::expand_home(path))),
            Self::S3 { endpoint, region, bucket, prefix, access_key, secret_key } => {
                let endpoint =
                    env("AWS_ENDPOINT_URL").or(endpoint.clone()).context("the S3 remote needs an endpoint")?;
                let region = env("AWS_REGION").or(region.clone()).unwrap_or_else(|| "us-east-1".into());
                let key =
                    env("AWS_ACCESS_KEY_ID").or(access_key.clone()).context("the S3 remote needs an access_key")?;
                let secret = match env("AWS_SECRET_ACCESS_KEY") {
                    Some(secret) => secret,
                    None => secret_key.as_ref().context("the S3 remote needs a secret_key")?.read()?,
                };
                let url = endpoint.parse().with_context(|| format!("invalid endpoint {endpoint:?}"))?;
                let style = rusty_s3::UrlStyle::Path;
                let bucket = rusty_s3::Bucket::new(url, style, bucket.clone(), region).context("invalid S3 bucket")?;
                let prefix = match prefix.trim_matches('/') {
                    "" => String::new(),
                    p => format!("{p}/"),
                };
                Box::new(S3 { bucket, credentials: rusty_s3::Credentials::new(key, secret), prefix })
            }
        })
    }
}

pub struct S3 {
    bucket: rusty_s3::Bucket,
    credentials: rusty_s3::Credentials,
    prefix: String,
}

/// Files transferred at once.
const WORKERS: usize = 8;

/// How long a signed request stays valid; generous, for big uploads on slow links.
const SIGNED_FOR: Duration = Duration::from_secs(3600);

impl S3 {
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .user_agent(concat!("carrel/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(30)))
            .build()
            .into()
    }
}

impl Remote for S3 {
    fn name(&self) -> String {
        format!("{}{}", self.bucket.base_url(), self.prefix)
    }

    fn list(&self) -> Result<BTreeMap<String, String>> {
        let mut files = BTreeMap::new();
        let mut token = None;
        loop {
            let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
            if !self.prefix.is_empty() {
                action.with_prefix(self.prefix.as_str());
            }
            if let Some(token) = &token {
                action.with_continuation_token(String::clone(token));
            }
            let response = Self::agent().get(action.sign(SIGNED_FOR).as_str()).call().map_err(s3_error)?;
            let text = response.into_body().read_to_string()?;
            let page = rusty_s3::actions::ListObjectsV2::parse_response(&text).context("unexpected S3 listing")?;
            for object in page.contents {
                if let Some(path) = object.key.strip_prefix(&self.prefix) {
                    files.insert(path.to_string(), object.etag.unwrap_or_default().trim_matches('"').to_string());
                }
            }
            match page.next_continuation_token {
                Some(next) => token = Some(next),
                None => return Ok(files),
            }
        }
    }

    fn download(&self, path: &str, to: &Path) -> Result<()> {
        let key = format!("{}{path}", self.prefix);
        let url = self.bucket.get_object(Some(&self.credentials), &key).sign(SIGNED_FOR);
        let response = Self::agent().get(url.as_str()).call().map_err(s3_error)?;
        std::io::copy(&mut response.into_body().as_reader(), &mut fs::File::create(to)?)?;
        Ok(())
    }

    fn upload(&self, path: &str, bytes: Vec<u8>) -> Result<String> {
        let key = format!("{}{path}", self.prefix);
        let url = self.bucket.put_object(Some(&self.credentials), &key).sign(SIGNED_FOR);
        // ponytail: one PUT per file (S3 allows 5 GB); multipart when someone shares bigger files.
        let response = Self::agent().put(url.as_str()).send(bytes).map_err(s3_error)?;
        let etag = response.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or_default();
        Ok(etag.trim_matches('"').to_string())
    }

    fn delete(&self, path: &str) -> Result<()> {
        let key = format!("{}{path}", self.prefix);
        let url = self.bucket.delete_object(Some(&self.credentials), &key).sign(SIGNED_FOR);
        Self::agent().delete(url.as_str()).call().map_err(s3_error)?;
        Ok(())
    }
}

/// S3 explains refusals in the body: `<Code>AccessDenied</Code>`.
fn s3_error(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::StatusCode(403) => anyhow::anyhow!("the S3 server refused access; check the remote's keys"),
        ureq::Error::StatusCode(404) => anyhow::anyhow!("not found on the S3 server; check the bucket's name"),
        e => anyhow::Error::new(e).context("S3"),
    }
}

/// A folder as a remote; its version tags are MD5s, like S3's.
pub struct Folder(pub PathBuf);

impl Remote for Folder {
    fn name(&self) -> String {
        self.0.display().to_string()
    }

    fn list(&self) -> Result<BTreeMap<String, String>> {
        if !self.0.is_dir() {
            bail!("the remote folder {} doesn't exist", self.0.display());
        }
        local_files(&self.0)?.into_iter().map(|path| Ok((path.clone(), md5_file(&self.0.join(&path))?))).collect()
    }

    fn download(&self, path: &str, to: &Path) -> Result<()> {
        fs::copy(self.0.join(path), to)?;
        Ok(())
    }

    fn upload(&self, path: &str, bytes: Vec<u8>) -> Result<String> {
        let to = self.0.join(path);
        fs::create_dir_all(to.parent().unwrap())?;
        fs::write(&to, &bytes)?;
        Ok(md5_hex(&bytes))
    }

    fn delete(&self, path: &str) -> Result<()> {
        let file = self.0.join(path);
        fs::remove_file(&file)?;
        remove_empty_parents(&file, &self.0);
        Ok(())
    }
}

/// The library's files as `/`-separated paths, skipping hidden files and folders.
fn local_files(root: &Path) -> Result<Vec<String>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&entry.path(), &format!("{prefix}{name}/"), out)?;
            } else if kind.is_file() {
                out.push(format!("{prefix}{name}"));
            }
        }
        Ok(())
    }
    let mut out = vec![];
    walk(root, "", &mut out)?;
    Ok(out)
}

fn md5_file(path: &Path) -> Result<String> {
    Ok(md5_hex(&fs::read(path)?))
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn remove_empty_parents(file: &Path, root: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// What both sides held after the last sync, in `.carrel/sync.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    remote: String,
    files: BTreeMap<String, Synced>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Synced {
    md5: String,
    etag: String,
    /// The local file's size and modification time then, to skip hashing it again while unchanged.
    size: u64,
    mtime: u128,
}

/// What a sync does with one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Upload,
    Download,
    DeleteRemote,
    DeleteLocal,
    /// Changed on both sides; left alone.
    Conflict,
}

/// Which side wins conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Keep {
    Local,
    Remote,
}

/// The action for a file from its MD5 here, its tag on the remote and what both were at the last sync.
fn decide(local: Option<&str>, remote: Option<&str>, base: Option<&Synced>, keep: Option<Keep>) -> Option<Action> {
    let action = match (local, remote, base) {
        (Some(l), Some(r), Some(b)) => match (l != b.md5, r != b.etag) {
            (false, false) => return None,
            (true, false) => Action::Upload,
            (false, true) => Action::Download,
            (true, true) if l == r => return None,
            (true, true) => Action::Conflict,
        },
        (Some(l), Some(r), None) if l == r => return None,
        (Some(_), Some(_), None) => Action::Conflict,
        // An edit beats a deletion.
        (Some(l), None, Some(b)) if l == b.md5 => Action::DeleteLocal,
        (Some(_), None, _) => Action::Upload,
        (None, Some(r), Some(b)) if r == b.etag => Action::DeleteRemote,
        (None, Some(_), _) => Action::Download,
        (None, None, _) => return None,
    };
    Some(match (action, keep) {
        (Action::Conflict, Some(Keep::Local)) => Action::Upload,
        (Action::Conflict, Some(Keep::Remote)) => Action::Download,
        (action, _) => action,
    })
}

/// The outcome of a sync, file by file.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub done: Vec<(String, Action)>,
    pub failed: Vec<(String, String)>,
    /// Remote keys that aren't plain paths inside the library (`../x`, `/x`, folder markers `x/`).
    pub skipped: Vec<String>,
}

impl Report {
    pub fn count(&self, action: Action) -> usize {
        self.done.iter().filter(|(_, a)| *a == action).count()
    }

    /// "upload 3, download 1, delete 0 here and 0 remotely".
    pub fn summary(&self) -> String {
        let count = |a| self.count(a);
        let (up, down) = (count(Action::Upload), count(Action::Download));
        let (here, there) = (count(Action::DeleteLocal), count(Action::DeleteRemote));
        format!("upload {up}, download {down}, delete {here} here and {there} remotely")
    }
}

impl Library {
    fn state_path(&self) -> PathBuf {
        self.root.join(".carrel").join("sync.json")
    }

    /// Syncs the library with `remote`, calling `progress` before each file; with `dry_run`, only says
    /// what it would do.
    pub fn sync(
        &self,
        remote: &dyn Remote,
        keep: Option<Keep>,
        dry_run: bool,
        mut progress: impl FnMut(&str, Action) + Send,
    ) -> Result<Report> {
        let carrel = self.root.join(".carrel");
        fs::create_dir_all(&carrel)?;
        // Held until the sync returns; two at once would download into the same parts and state.
        let lock = fs::File::create(carrel.join("sync.lock"))?;
        // A moment's patience: a process this one starts (an extractor, from another thread) holds a copy of the
        // lock from fork to exec, which is no sync at all.
        let mut tries = 0;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if tries < 40 => tries += 1,
                Err(fs::TryLockError::WouldBlock) => bail!("another sync is running in this library; try again later"),
                Err(fs::TryLockError::Error(e)) => return Err(e).context("cannot lock .carrel/sync.lock"),
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let name = remote.name();
        let mut state: State = fs::read(self.state_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|s: &State| s.remote == name)
            .unwrap_or_else(|| State { remote: name, files: BTreeMap::new() });

        // Local files, hashing only those changed since the last sync.
        let mut local = BTreeMap::new();
        for path in local_files(&self.root)? {
            let meta = fs::metadata(self.root.join(&path))?;
            let (size, mtime) = (meta.len(), mtime(&meta));
            let md5 = match state.files.get(&path) {
                Some(s) if s.size == size && s.mtime == mtime => s.md5.clone(),
                _ => md5_file(&self.root.join(&path))?,
            };
            local.insert(path, (md5, size, mtime));
        }
        let mut outcome = Report::default();
        let mut remote_files = remote.list()?;
        // Anyone with the bucket's keys picks the keys: never let one point outside the library.
        remote_files.retain(|path, _| {
            if path.contains('\\') || path.split('/').any(|c| matches!(c, "" | "." | "..")) {
                outcome.skipped.push(path.clone());
                return false;
            }
            // Hidden files stay local on both sides, `.carrel/` above all.
            !path.split('/').any(|c| c.starts_with('.'))
        });
        // An empty remote would trash every file synced before: more likely a wrong bucket or prefix,
        // or a wiped remote, than everyone deleting everything.
        let synced_here = state.files.keys().filter(|p| local.contains_key(*p)).count();
        if remote_files.is_empty() && synced_here > 0 {
            bail!(
                "{} is empty, but {synced_here} files here were synced with it; check the bucket and prefix. \
                 To delete them here too, delete them yourself; to upload them again, delete .carrel/sync.json",
                remote.name()
            );
        }

        let paths: BTreeSet<String> =
            local.keys().chain(remote_files.keys()).chain(state.files.keys()).cloned().collect();
        let mut plan = vec![];
        for path in paths {
            let l = local.get(&path).map(|(md5, ..)| md5.as_str());
            let r = remote_files.get(&path).map(String::as_str);
            match decide(l, r, state.files.get(&path), keep) {
                Some(action) => plan.push((path, action)),
                // In sync: remember both sides so the next sync can tell who changed what.
                None => match (local.get(&path), r) {
                    (Some((md5, size, mtime)), Some(etag)) => {
                        let synced = Synced { md5: md5.clone(), etag: etag.into(), size: *size, mtime: *mtime };
                        state.files.insert(path, synced);
                    }
                    _ => {
                        state.files.remove(&path);
                    }
                },
            }
        }

        let (listed, plan): (Vec<_>, Vec<_>) = plan.into_iter().partition(|(_, a)| dry_run || *a == Action::Conflict);
        for (path, action) in listed {
            progress(&path, action);
            outcome.done.push((path, action));
        }
        let next = AtomicUsize::new(0);
        let shared = Mutex::new((state, outcome, progress));
        std::thread::scope(|scope| {
            for _ in 0..WORKERS {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((path, action)) = plan.get(i) else { break };
                        (shared.lock().unwrap().2)(path, *action);
                        let result = self.apply(remote, path, *action, remote_files.get(path), i);
                        let mut guard = shared.lock().unwrap();
                        let (state, outcome, _) = &mut *guard;
                        match result {
                            Ok(Some(synced)) => drop(state.files.insert(path.clone(), synced)),
                            Ok(None) => drop(state.files.remove(path)),
                            Err(e) => {
                                outcome.failed.push((path.clone(), format!("{e:#}")));
                                continue;
                            }
                        }
                        outcome.done.push((path.clone(), *action));
                        // Saved as it goes, so an interrupted sync resumes where it stopped.
                        if outcome.done.len() % 100 == 0 {
                            let _ = self.save_state(state);
                        }
                    }
                });
            }
        });
        let (state, outcome, _) = shared.into_inner().unwrap();
        if !dry_run {
            self.save_state(&state)?;
            let _ = fs::remove_dir_all(carrel.join("parts"));
        }
        Ok(outcome)
    }

    /// Does one file's action; what both sides now hold, if anything.
    fn apply(
        &self,
        remote: &dyn Remote,
        path: &str,
        action: Action,
        etag: Option<&String>,
        n: usize,
    ) -> Result<Option<Synced>> {
        let file = self.root.join(path);
        match action {
            Action::Upload => {
                // Record what was sent, and the metadata from before reading it: an edit meanwhile then
                // shows as a change at the next sync.
                let meta = fs::metadata(&file)?;
                let bytes = fs::read(&file)?;
                let md5 = md5_hex(&bytes);
                let etag = remote.upload(path, bytes)?;
                return Ok(Some(Synced { md5, etag, size: meta.len(), mtime: mtime(&meta) }));
            }
            Action::Download => {
                fs::create_dir_all(file.parent().unwrap())?;
                let part = self.root.join(".carrel").join("parts").join(format!("{}-{n}", std::process::id()));
                fs::create_dir_all(part.parent().unwrap())?;
                remote.download(path, &part)?;
                // Whatever was here goes to the trash, so a sync never loses a file for good.
                if file.exists() {
                    self.trash(path)?;
                }
                fs::rename(&part, &file)?;
                return synced(&file, etag.cloned().unwrap_or_default()).map(Some);
            }
            Action::DeleteRemote => remote.delete(path)?,
            Action::DeleteLocal => {
                self.trash(path)?;
                remove_empty_parents(&file, &self.root);
            }
            Action::Conflict => {}
        }
        Ok(None)
    }

    /// Moves a file into `.carrel/trash`, keeping its path.
    fn trash(&self, path: &str) -> Result<()> {
        let to = self.root.join(".carrel").join("trash").join(path);
        fs::create_dir_all(to.parent().unwrap())?;
        fs::rename(self.root.join(path), &to).with_context(|| format!("cannot move {path} to the trash"))
    }

    fn save_state(&self, state: &State) -> Result<()> {
        let path = self.state_path();
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(state)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

fn synced(file: &Path, etag: String) -> Result<Synced> {
    let meta = fs::metadata(file)?;
    Ok(Synced { md5: md5_file(file)?, etag, size: meta.len(), mtime: mtime(&meta) })
}

fn mtime(meta: &fs::Metadata) -> u128 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deciding() {
        let base = Synced { md5: "m".into(), etag: "e".into(), size: 0, mtime: 0 };
        let b = Some(&base);
        let cases = [
            (Some("m"), Some("e"), b, None),
            (Some("x"), Some("e"), b, Some(Action::Upload)),
            (Some("m"), Some("x"), b, Some(Action::Download)),
            (Some("x"), Some("x"), b, None), // the same change on both sides
            (Some("x"), Some("y"), b, Some(Action::Conflict)),
            (Some("m"), None, b, Some(Action::DeleteLocal)),
            (Some("x"), None, b, Some(Action::Upload)), // an edit beats a deletion
            (None, Some("e"), b, Some(Action::DeleteRemote)),
            (None, Some("x"), b, Some(Action::Download)),
            (None, None, b, None),
            (Some("m"), None, None, Some(Action::Upload)),
            (None, Some("e"), None, Some(Action::Download)),
            (Some("a"), Some("a"), None, None), // S3 ETags are MD5s: the same file
            (Some("a"), Some("b"), None, Some(Action::Conflict)),
        ];
        for (local, remote, base, expected) in cases {
            assert_eq!(decide(local, remote, base, None), expected, "{local:?} {remote:?} {base:?}");
        }
        assert_eq!(decide(Some("x"), Some("y"), b, Some(Keep::Local)), Some(Action::Upload));
        assert_eq!(decide(Some("x"), Some("y"), b, Some(Keep::Remote)), Some(Action::Download));
    }
}
