//! carrel: a paper library stored as plain files, one folder per paper, named by its citation key.
//!
//! ```text
//! <library>/vaswani2017attention/
//!   vaswani2017attention.bib   metadata and approved keywords; its key is the folder name
//!   vaswani2017attention.pdf
//!   fulltext.md                extracted text
//!   figures/                   extracted figures
//!   summary.md  notes.md
//!   references.json            what it cites, each with its source (graph.rs)
//!   drafts/                    AI proposals waiting for review
//! ```
//!
//! The files are the source of truth; the search index in the cache directory is rebuilt from them.
//! Every frontend (CLI, TUI, MCP) goes through [`Library`].

pub mod authors;
pub mod bib;
mod doctor;
pub mod fetch;
pub mod graph;
mod ilibrarian;
mod index;
mod projects;
pub mod refs;
mod update;

pub use doctor::Issue;

use anyhow::{Context, Result, bail};
use bib::Entry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// The parts of a paper an AI may write. Its writes land in `drafts/` until a human approves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Summary,
    Keywords,
    Notes,
    References,
}

impl Field {
    pub const ALL: [Field; 4] = [Field::Summary, Field::Keywords, Field::Notes, Field::References];

    pub fn name(self) -> &'static str {
        match self {
            Field::Summary => "summary",
            Field::Keywords => "keywords",
            Field::Notes => "notes",
            Field::References => "references",
        }
    }

    /// File name of its draft; all but keywords (which go into the .bib) keep that name once approved.
    pub fn file(self) -> &'static str {
        match self {
            Field::Summary => "summary.md",
            Field::Keywords => "keywords.txt",
            Field::Notes => "notes.md",
            Field::References => graph::FILE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Paper {
    pub id: String,
    pub dir: PathBuf,
    pub bib: Entry,
}

impl Paper {
    fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(name)).ok()
    }

    pub fn bib_path(&self) -> PathBuf {
        self.dir.join(format!("{}.bib", self.id))
    }

    pub fn pdf(&self) -> Option<PathBuf> {
        Some(self.dir.join(format!("{}.pdf", self.id))).filter(|p| p.exists())
    }

    /// Extracted full text (Markdown).
    pub fn fulltext(&self) -> Option<String> {
        self.read("fulltext.md")
    }

    /// The approved version of a field.
    pub fn get(&self, field: Field) -> Option<String> {
        match field {
            Field::Keywords => Some(self.bib.keywords().join("\n")).filter(|k| !k.is_empty()),
            _ => self.read(field.file()),
        }
    }

    pub fn draft(&self, field: Field) -> Option<String> {
        self.read(&format!("drafts/{}", field.file()))
    }

    pub fn drafts(&self) -> Vec<Field> {
        Field::ALL.into_iter().filter(|f| self.dir.join("drafts").join(f.file()).exists()).collect()
    }

    pub fn figures(&self) -> Vec<String> {
        let entries = fs::read_dir(self.dir.join("figures")).into_iter().flatten().flatten();
        let mut names: Vec<String> = entries.filter_map(|e| e.file_name().into_string().ok()).collect();
        names.sort();
        names
    }

    /// Files in `supplementary/`, with the text extracted from supplementary PDFs as `<name>.md`.
    pub fn supplementary(&self) -> Vec<String> {
        let entries = fs::read_dir(self.dir.join(SUPPLEMENTARY)).into_iter().flatten().flatten();
        let mut names: Vec<String> = entries.filter_map(|e| e.file_name().into_string().ok()).collect();
        names.sort();
        names
    }

    /// "Vaswani et al." style.
    pub fn short_authors(&self) -> String {
        match self.bib.authors().as_slice() {
            [] => String::new(),
            [a] => bib::family(a).to_string(),
            [a, b] => format!("{} & {}", bib::family(a), bib::family(b)),
            [a, ..] => format!("{} et al.", bib::family(a)),
        }
    }

    /// The entry as stored, ready to be concatenated into a bibliography.
    pub fn bibtex(&self) -> String {
        fs::read_to_string(self.bib_path()).unwrap_or_else(|_| self.bib.to_bib(&self.id))
    }

    fn save(&self) -> Result<()> {
        fs::write(self.bib_path(), self.bib.to_bib(&self.id))?;
        Ok(())
    }
}

/// `$XDG_CONFIG_HOME`-style variables count on every platform, as command-line users expect (the
/// `dirs` crate ignores them on macOS); otherwise `fallback`.
fn xdg_dir(var: &str, fallback: Option<PathBuf>) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|dir| dir.is_absolute()).or(fallback)
}

/// The default citation key: `vaswani2017attention`.
pub const DEFAULT_KEY: &str = "{{ author | lower }}{{ year }}{{ word | lower }}";

/// Settings from `~/.config/carrel/config.toml`; all optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The library folder. `CARREL_HOME` overrides it.
    pub library: Option<String>,
    /// Template for the citation keys of new papers, which also name their folder and files.
    pub key: Option<String>,
    /// Shell command that turns `$PDF` into `fulltext.md` (and `figures/`) in the current directory.
    /// `CARREL_EXTRACTOR` overrides it.
    pub extractor: Option<String>,
}

impl Config {
    /// `$XDG_CONFIG_HOME/carrel/config.toml`, by default `~/.config/carrel/config.toml`, on macOS too.
    pub fn path() -> Option<PathBuf> {
        let dir = xdg_dir("XDG_CONFIG_HOME", dirs::home_dir().map(|home| home.join(".config")));
        dir.map(|dir| dir.join("carrel").join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let Some(path) = Self::path().filter(|p| p.exists()) else { return Ok(Self::default()) };
        let text = fs::read_to_string(&path)?;
        toml::from_str(&text).with_context(|| format!("{} is invalid", path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    pub root: PathBuf,
    /// The search index, a cache of the files.
    pub index: PathBuf,
    /// Template for new citation keys.
    pub key: String,
    /// Shell command extracting text and figures, see [`DEFAULT_EXTRACTOR`].
    pub extractor: String,
}

impl Library {
    /// `$CARREL_HOME`, else the config's `library`, else `carrel` in your Documents folder.
    pub fn open_default() -> Result<Self> {
        let config = Config::load()?;
        let configured = config.library.map(|dir| match dir.strip_prefix("~/") {
            Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
            None => PathBuf::from(dir),
        });
        let root = match std::env::var_os("CARREL_HOME").map(PathBuf::from).or(configured) {
            Some(dir) => dir,
            None => dirs::document_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
                .context("cannot find your Documents folder; set CARREL_HOME")?
                .join("carrel"),
        };
        let mut lib = Self::open(root)?;
        if let Some(key) = config.key {
            // Fail now rather than on the next add.
            citekey(&key, &Entry::new("misc")).context("the key template in your config is invalid")?;
            lib.key = key;
        }
        if let Some(cmd) = std::env::var("CARREL_EXTRACTOR").ok().or(config.extractor) {
            lib.extractor = cmd;
        }
        Ok(lib)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).with_context(|| format!("cannot create a library at {}", root.display()))?;
        let root = root.canonicalize()?;
        let cache = xdg_dir("XDG_CACHE_HOME", dirs::cache_dir()).unwrap_or_else(std::env::temp_dir).join("carrel");
        let mut hash = DefaultHasher::new();
        root.hash(&mut hash);
        let index = cache.join(format!("index-{:016x}.sqlite", hash.finish()));
        Ok(Self { root, index, key: DEFAULT_KEY.into(), extractor: DEFAULT_EXTRACTOR.into() })
    }

    pub fn get(&self, id: &str) -> Result<Paper> {
        // Ids also arrive from AI tool calls: never let one point outside the library.
        if !valid_id(id) {
            bail!("invalid paper id {id:?}");
        }
        let dir = self.root.join(id);
        let path = dir.join(format!("{id}.bib"));
        let src = fs::read_to_string(&path).with_context(|| match dir.is_dir() {
            true => format!("{} is missing", path.display()),
            false => format!("no paper with id {id:?}"),
        })?;
        let (key, bib) = Entry::parse(&src).with_context(|| format!("{} is invalid", path.display()))?;
        if key != id {
            bail!("{}: the key is {key:?} but the folder is {id:?}; `carrel rename` changes both", path.display());
        }
        Ok(Paper { id: id.to_string(), dir, bib })
    }

    /// Where a paper's .bib is, whether or not it loads (for fixing one that doesn't).
    pub fn bib_path(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            bail!("invalid paper id {id:?}");
        }
        Ok(self.root.join(id).join(format!("{id}.bib")))
    }

    /// The library's paper folders, loadable or not.
    fn folders(&self) -> Vec<String> {
        let entries = fs::read_dir(&self.root).into_iter().flatten().flatten().filter(|e| e.path().is_dir());
        let names = entries.map(|e| e.file_name().to_string_lossy().into_owned());
        let reserved = [projects::DIR, authors::DIR];
        let mut names: Vec<String> =
            names.filter(|name| !name.starts_with('.') && !reserved.contains(&name.as_str())).collect();
        names.sort();
        names
    }

    /// All papers sorted by id, plus a message for each folder that failed to load.
    pub fn papers(&self) -> (Vec<Paper>, Vec<String>) {
        let (mut papers, mut broken) = (vec![], vec![]);
        for id in self.folders() {
            match self.get(&id) {
                Ok(p) => papers.push(p),
                Err(e) => broken.push(format!("{e:#}")),
            }
        }
        (papers, broken)
    }

    /// Add a paper from a PDF, DOI, arXiv id/URL or exact title. Returns its id and messages worth showing.
    pub fn add(&self, source: &str) -> Result<(String, Vec<String>)> {
        let tmp = Staging::new(&self.root)?;
        let ids = match fetch::classify(source)? {
            fetch::Source::Pdf(path) => {
                copy_pdf(&path, &tmp.0.join("paper.pdf"))?;
                fetch::Ids::default()
            }
            fetch::Source::Arxiv(id) => fetch::Ids { arxiv: Some(id), ..Default::default() },
            fetch::Source::Doi(doi) => fetch::Ids { doi: Some(doi), ..Default::default() },
            fetch::Source::Title(title) => fetch::by_title(&title)?,
        };
        self.add_staged(tmp, ids, source)
    }

    /// Add a search result: metadata from its DOI when it has one, the PDF from arXiv when it is there.
    pub fn add_ids(&self, ids: &fetch::Ids) -> Result<(String, Vec<String>)> {
        self.add_staged(Staging::new(&self.root)?, ids.clone(), &ids.source())
    }

    fn add_staged(&self, tmp: Staging, mut ids: fetch::Ids, source: &str) -> Result<(String, Vec<String>)> {
        let mut messages = vec![];
        let pdf = tmp.0.join("paper.pdf");
        if !pdf.exists()
            && let Some(id) = &ids.arxiv
        {
            fetch::download_arxiv(id, &pdf)?;
        }
        let has_pdf = pdf.exists();
        if has_pdf && let Err(e) = run_extractor(&self.extractor, &pdf, &tmp.0) {
            messages.push(format!("{e:#}"));
        }

        // Identify a PDF: ids printed on its first page, else its title.
        let text = fs::read_to_string(tmp.0.join("fulltext.md")).unwrap_or_default();
        let title = guess_title(&text);
        if ids.is_empty() {
            ids = fetch::find_ids(&text);
        }
        if ids.is_empty()
            && let Some(t) = &title
        {
            match fetch::exact_title(t) {
                Ok((found, _)) => ids = found.unwrap_or_default(),
                Err(e) => messages.push(format!("title lookup failed: {e:#}")),
            }
        }
        let mut bib = if ids.is_empty() {
            messages.push("no DOI or arXiv id found; fix the metadata with `carrel edit`".into());
            Entry::new("misc")
        } else {
            match fetch::metadata(&ids) {
                Ok(bib) => bib,
                Err(e) if has_pdf => {
                    messages.push(format!("metadata lookup failed: {e:#}"));
                    Entry::new("misc")
                }
                Err(e) => return Err(e),
            }
        };
        if bib.title().is_empty() {
            bib.set("title", Some(&title.unwrap_or_else(|| file_stem(source))));
        }
        if bib.doi().is_none() {
            bib.set("doi", ids.doi.as_deref());
        }
        if bib.arxiv().is_none() && ids.arxiv.is_some() {
            bib.set("eprint", ids.arxiv.as_deref());
            bib.set("archiveprefix", Some("arXiv"));
        }

        if let Some(old) = self.papers().0.into_iter().find(|p| same_paper(&p.bib, &bib)) {
            if !has_pdf || old.pdf().is_some() {
                bail!("already in the library as {}", old.id);
            }
            move_outputs(&tmp.0, &old.dir, &old.id)?;
            messages.push("attached the PDF to the existing entry".into());
            return Ok((old.id, messages));
        }
        let known = fetch::Ids { doi: bib.doi(), arxiv: bib.arxiv() };
        if !known.is_empty() {
            match graph::look_up(&refs::sources(), &known, &bib.title()) {
                Ok(found) => graph::write(&tmp.0, &found)?,
                Err(e) => messages.push(format!("references: {e:#}")),
            }
        }
        let id = self.unique_id(&citekey(&self.key, &bib)?);
        fs::write(tmp.0.join(format!("{id}.bib")), bib.to_bib(&id))?;
        if has_pdf {
            fs::rename(&pdf, tmp.0.join(format!("{id}.pdf")))?;
        }
        fs::rename(&tmp.0, self.root.join(&id))?;
        Ok((id, messages))
    }

    fn unique_id(&self, base: &str) -> String {
        let mut candidates = std::iter::once(base.to_string()).chain(('b'..='z').map(|c| format!("{base}{c}")));
        let free = candidates.find(|id| valid_id(id) && !self.root.join(id).exists());
        free.unwrap_or_else(|| format!("{base}-{}", std::process::id()))
    }

    /// Give a paper its PDF, copied from `pdf` or else downloaded from arXiv, and extract text and
    /// figures. Returns messages worth showing.
    pub fn attach(&self, id: &str, pdf: Option<&Path>) -> Result<Vec<String>> {
        let p = self.get(id)?;
        if let Some(existing) = p.pdf() {
            bail!("{id} already has a PDF; delete {} first to replace it", existing.display());
        }
        let tmp = Staging::new(&self.root)?;
        match pdf {
            Some(pdf) => copy_pdf(pdf, &tmp.0.join("paper.pdf"))?,
            None => {
                let arxiv = p.bib.arxiv().context("no arXiv id to download the PDF from; give a file")?;
                fetch::download_arxiv(&arxiv, &tmp.0.join("paper.pdf"))?;
            }
        }
        let messages = run_extractor(&self.extractor, &tmp.0.join("paper.pdf"), &tmp.0).err().map(|e| format!("{e:#}"));
        move_outputs(&tmp.0, &p.dir, id)?;
        Ok(messages.into_iter().collect())
    }

    /// Import the entries of a .bib file (from a LaTeX project, Zotero, JabRef, ...) as they are, keeping
    /// their keys so existing citations still work. Papers already in the library are matched instead.
    /// With `pdfs`, papers with an arXiv id get their PDF. Returns the ids in file order and messages.
    pub fn import_bib(&self, path: &Path, pdfs: bool) -> Result<(Vec<String>, Vec<String>)> {
        let src = fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let (mut ids, mut messages) = (vec![], vec![]);
        let mut known = self.papers().0;
        let macros = Entry::macros(&src);
        for entry in Entry::parse_all(&src) {
            let (key, mut bib) = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    messages.push(format!("skipped an entry: {e:#}"));
                    continue;
                }
            };
            bib.expand(&macros);
            if let Some(old) = known.iter().find(|p| same_paper(&p.bib, &bib)) {
                messages.push(format!("{key}: already in the library as {}", old.id));
                ids.push(old.id.clone());
                continue;
            }
            let id = match valid_id(&key) && !self.root.join(&key).exists() {
                true => key.clone(),
                false => {
                    let id = self.unique_id(&citekey(&self.key, &bib)?);
                    messages.push(format!("{key}: imported as {id}, since {key:?} is taken or can't name a folder"));
                    id
                }
            };
            fs::create_dir_all(self.root.join(&id))?;
            fs::write(self.root.join(&id).join(format!("{id}.bib")), bib.to_bib(&id))?;
            if pdfs
                && bib.arxiv().is_some()
                && let Err(e) = self.attach(&id, None)
            {
                messages.push(format!("{id}: no PDF: {e:#}"));
            }
            known.push(self.get(&id)?);
            ids.push(id);
        }
        Ok((ids, messages))
    }

    /// Note which candidates (search results, citations) are already in the library.
    pub fn mark_known(&self, candidates: &mut [fetch::Candidate]) {
        let papers = self.papers().0;
        for c in candidates {
            let mut bib = Entry::new("misc");
            bib.set("title", Some(&c.title));
            bib.set("doi", c.ids.doi.as_deref());
            bib.set("eprint", c.ids.arxiv.as_deref());
            c.in_library = papers.iter().find(|p| same_paper(&p.bib, &bib)).map(|p| p.id.clone());
        }
    }

    /// The papers a paper cites (its [references](Library::references)), or with `cited_by` the papers
    /// citing it (Semantic Scholar).
    pub fn related(&self, id: &str, cited_by: bool) -> Result<Vec<fetch::Candidate>> {
        if !cited_by {
            return Ok(self.references(id, false)?.into_iter().map(|(r, found)| r.candidate(found)).collect());
        }
        let bib = self.get(id)?.bib;
        let mut found = fetch::related(&fetch::Ids { doi: bib.doi(), arxiv: bib.arxiv() }, true)?;
        self.mark_known(&mut found);
        Ok(found)
    }

    /// Add supplementary files (local paths or http(s) URLs) to a paper. PDFs get their text extracted
    /// next to them (`appendix.pdf` → `appendix.md`, `appendix-figures/`). Returns messages worth showing.
    pub fn add_supplementary(&self, id: &str, sources: &[String]) -> Result<Vec<String>> {
        let dir = self.get(id)?.dir.join(SUPPLEMENTARY);
        fs::create_dir_all(&dir)?;
        let mut messages = vec![];
        for source in sources {
            let url = source.starts_with("http://") || source.starts_with("https://");
            let name = source.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
            let name = name.split(['?', '#']).next().unwrap_or_default();
            let name = if name.is_empty() || name.starts_with('.') { "download" } else { name };
            let target = dir.join(name);
            if target.exists() {
                bail!("{} already exists", target.display());
            }
            let copied = match url {
                true => fetch::download(source, &target),
                false => fs::copy(source, &target).map(drop).with_context(|| format!("cannot read {source}")),
            };
            if let Err(e) = copied {
                let _ = fs::remove_file(&target); // don't leave half a download behind
                return Err(e);
            }
            let stem = Path::new(name).file_stem().map_or(name.into(), |s| s.to_string_lossy().into_owned());
            if fs::read(&target)?.starts_with(b"%PDF") && dir.join(format!("{stem}.md")).exists() {
                messages.push(format!("{name}: not extracting its text, {stem}.md already exists"));
            } else if fs::read(&target)?.starts_with(b"%PDF") {
                let tmp = Staging::new(&self.root)?;
                match run_extractor(&self.extractor, &target, &tmp.0) {
                    Ok(()) => {
                        fs::rename(tmp.0.join("fulltext.md"), dir.join(format!("{stem}.md")))?;
                        if tmp.0.join("figures").exists() {
                            fs::rename(tmp.0.join("figures"), dir.join(format!("{stem}-figures")))?;
                        }
                    }
                    Err(e) => messages.push(format!("{name}: {e:#}")),
                }
            }
            messages.push(format!("added {name}"));
        }
        Ok(messages)
    }

    /// Re-run text and figure extraction for a paper.
    pub fn extract(&self, id: &str) -> Result<()> {
        let p = self.get(id)?;
        let pdf = p.pdf().context("this paper has no PDF")?;
        let tmp = Staging::new(&self.root)?;
        run_extractor(&self.extractor, &pdf, &tmp.0)?;
        let _ = fs::remove_dir_all(p.dir.join("figures")); // even if the new extraction has none
        move_outputs(&tmp.0, &p.dir, id)
    }

    /// Change a paper's key: its folder, its file names, the key inside its .bib and its project entries.
    /// Without a new key, the key template makes one from the metadata. Returns the new key.
    pub fn rename(&self, id: &str, new: Option<&str>) -> Result<String> {
        let p = self.get(id)?;
        let new = match new {
            Some(new) => new.to_string(),
            None => match citekey(&self.key, &p.bib)? {
                key if key == id => return Ok(key), // already what the template gives
                key => self.unique_id(&key),
            },
        };
        let dir = self.root.join(&new);
        if !valid_id(&new) || dir.exists() {
            bail!("{new:?} is not a valid new id, or already taken");
        }
        let had_pdf = p.pdf().is_some();
        fs::rename(&p.dir, &dir)?;
        if had_pdf {
            fs::rename(dir.join(format!("{id}.pdf")), dir.join(format!("{new}.pdf")))?;
        }
        fs::write(dir.join(format!("{new}.bib")), p.bib.to_bib(&new))?;
        fs::remove_file(dir.join(format!("{id}.bib")))?;
        self.replace_in_projects(id, Some(&new))?;
        Ok(new)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        fs::remove_dir_all(self.get(id)?.dir)?;
        self.replace_in_projects(id, None)
    }

    /// Store an AI proposal as a draft; a human approves or rejects it later.
    pub fn propose(&self, id: &str, field: Field, content: &str) -> Result<()> {
        let p = self.get(id)?;
        let content = match field {
            Field::Keywords => normalize_keywords(content),
            Field::References => graph::ai_draft(content)?,
            _ => content.trim().to_string(),
        };
        if content.is_empty() {
            bail!("the draft is empty");
        }
        fs::create_dir_all(p.dir.join("drafts"))?;
        fs::write(p.dir.join("drafts").join(field.file()), content + "\n")?;
        Ok(())
    }

    /// Every draft waiting for review.
    pub fn drafts(&self) -> Vec<(Paper, Field)> {
        let papers = self.papers().0.into_iter();
        papers.flat_map(|p| p.drafts().into_iter().map(move |f| (p.clone(), f))).collect()
    }

    /// Accept a draft: a summary replaces the current one; keywords, notes and references not there yet are added.
    pub fn approve(&self, id: &str, field: Field) -> Result<()> {
        let mut p = self.get(id)?;
        let draft = p.dir.join("drafts").join(field.file());
        let text = fs::read_to_string(&draft).with_context(|| format!("{id} has no {} draft", field.name()))?;
        match field {
            Field::Summary => fs::rename(&draft, p.dir.join(field.file()))?,
            Field::Keywords => {
                let mut keywords = p.bib.keywords();
                for k in text.lines().map(str::trim).filter(|k| !k.is_empty()) {
                    if !keywords.iter().any(|old| old.eq_ignore_ascii_case(k)) {
                        keywords.push(k.to_string());
                    }
                }
                p.bib.set("keywords", Some(&keywords.join(", ")));
                p.save()?;
                fs::remove_file(&draft)?;
            }
            Field::Notes => {
                let old = p.get(Field::Notes).filter(|n| !n.trim().is_empty());
                let notes = old.map_or(text.clone(), |old| format!("{}\n\n{text}", old.trim_end()));
                fs::write(p.dir.join(field.file()), notes)?;
                fs::remove_file(&draft)?;
            }
            Field::References => {
                let drafted: Vec<graph::Reference> = serde_json::from_str(&text)?;
                graph::write(&p.dir, &graph::merge(graph::read(&p.dir)?.unwrap_or_default(), drafted))?;
                fs::remove_file(&draft)?;
            }
        }
        let _ = fs::remove_dir(p.dir.join("drafts")); // only succeeds once it is empty
        Ok(())
    }

    pub fn reject(&self, id: &str, field: Field) -> Result<()> {
        let p = self.get(id)?;
        fs::remove_file(p.dir.join("drafts").join(field.file()))
            .with_context(|| format!("{id} has no {} draft", field.name()))?;
        let _ = fs::remove_dir(p.dir.join("drafts"));
        Ok(())
    }

    /// Papers matching the query, best first, each with a snippet of where it matched. Words may be
    /// limited to a field (`author:hinton year:2015`) and `project:thesis` searches one project.
    /// A query without words lists the papers.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(Paper, String)>> {
        let mut project = None;
        let words: Vec<&str> = query
            .split_whitespace()
            .filter(|w| w.strip_prefix("project:").map(|name| project = Some(name)).is_none())
            .collect();
        let allowed = project.map(|name| self.project(name)).transpose()?;
        let wanted = |p: &Paper| allowed.as_ref().is_none_or(|ids| ids.contains(&p.id));
        let papers = self.papers().0;
        if words.is_empty() {
            return Ok(papers.into_iter().filter(wanted).take(limit).map(|p| (p, String::new())).collect());
        }
        let hits =
            index::search(&self.index, &papers, &words.join(" "), if project.is_some() { usize::MAX } else { limit })?;
        let mut by_id: HashMap<String, Paper> = papers.into_iter().map(|p| (p.id.clone(), p)).collect();
        let found = hits.into_iter().filter_map(|(id, snippet)| Some((by_id.remove(&id)?, snippet)));
        Ok(found.filter(|(p, _)| wanted(p)).take(limit).collect())
    }
}

/// Keys usable as folder names: ASCII letters, digits and `-_.:`, not starting with a dot.
/// Not starting with a dot (hidden) or a dash (a command-line flag), and not the projects folder.
/// `length` characters (0: 20000) of `text` from character `offset`, headed by where they are and how to
/// go on, so that an AI can read a long text in pieces.
pub fn chunk(text: &str, offset: usize, length: usize) -> String {
    let total = text.chars().count();
    let start = offset.min(total);
    let piece: String = text.chars().skip(start).take(if length == 0 { 20_000 } else { length }).collect();
    let end = start + piece.chars().count();
    let next = if end < total { format!("continue with offset {end}") } else { "the end".into() };
    format!("[characters {start}-{end} of {total}; {next}]\n\n{piece}")
}

fn valid_id(id: &str) -> bool {
    let chars = id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c));
    chars && !id.is_empty() && !id.starts_with(['.', '-']) && id != projects::DIR && id != authors::DIR
}

/// The folder for a paper's supplementary material.
pub const SUPPLEMENTARY: &str = "supplementary";

const EXTRACT_PY: &str = include_str!("extract.py");

/// Override with `CARREL_EXTRACTOR` or the config's `extractor`: any shell command that reads `$PDF`
/// and writes `fulltext.md` (and optionally `figures/`) into the current directory. Packagers can bake
/// in another default at build time with `CARREL_DEFAULT_EXTRACTOR` (the Nix package does).
pub const DEFAULT_EXTRACTOR: &str = match option_env!("CARREL_DEFAULT_EXTRACTOR") {
    Some(cmd) => cmd,
    None => r#"uv run --quiet --no-project --with pymupdf4llm==1.28.2 python -I -c "$CARREL_EXTRACT_PY" "$PDF""#,
};

fn run_extractor(cmd: &str, pdf: &Path, out: &Path) -> Result<()> {
    let output = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(out)
        .env("PDF", pdf)
        .env("CARREL_EXTRACT_PY", EXTRACT_PY)
        .stdin(Stdio::null())
        .output()
        .context("cannot run the extractor")?;
    if !output.status.success() || !out.join("fulltext.md").exists() {
        bail!("text extraction failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn copy_pdf(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).with_context(|| format!("cannot read {}", from.display()))?;
    if !fs::read(to)?.starts_with(b"%PDF") {
        bail!("{} is not a PDF", from.display());
    }
    Ok(())
}

/// Move a staged PDF and its extraction output into a paper's folder.
fn move_outputs(from: &Path, to: &Path, id: &str) -> Result<()> {
    let names =
        [("paper.pdf", format!("{id}.pdf")), ("fulltext.md", "fulltext.md".into()), ("figures", "figures".into())];
    for (src, dst) in names {
        if from.join(src).exists() {
            let _ = fs::remove_dir_all(to.join(&dst)); // old generated figures; files are replaced by the rename
            fs::rename(from.join(src), to.join(dst))?;
        }
    }
    Ok(())
}

/// Work folder inside the library (so moving out of it is a rename), deleted on drop.
struct Staging(PathBuf);

impl Staging {
    fn new(root: &Path) -> Result<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = root.join(".tmp").join(format!("{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn same_paper(a: &Entry, b: &Entry) -> bool {
    let eq = |x: Option<String>, y: Option<String>| matches!((x, y), (Some(x), Some(y)) if x.eq_ignore_ascii_case(&y));
    eq(a.doi(), b.doi()) || eq(a.arxiv(), b.arxiv()) || fetch::same_title(&a.title(), &b.title())
}

/// Words a key skips (shorter ones are skipped anyway): English, German, French, Spanish, Italian.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "towards", "toward", "via", "how", "der", "die", "das", "den", "dem",
    "des", "ein", "eine", "einer", "und", "für", "fur", "von", "mit", "zur", "zum", "les", "une", "pour", "avec",
    "dans", "sur", "los", "las", "una", "para", "con", "del", "por", "gli", "della", "per",
];

/// A citation key from the key template, a Jinja template (minijinja) over these variables:
/// `author` (first author's family name), `authors`, `year`, `title`, `word` (first meaningful title
/// word), `words`, `venue`, `doi`, `arxiv`, `type`. Missing ones are empty. The result is reduced to
/// ASCII letters, digits and `-_.:`, so it works as a BibTeX/Typst key and a folder name.
fn citekey(template: &str, bib: &Entry) -> Result<String> {
    let mut vars: BTreeMap<&str, minijinja::Value> = BTreeMap::new();
    let mut set = |name, value: Option<String>| {
        if let Some(value) = value {
            vars.insert(name, value.into());
        }
    };
    // ASCII from the start, so filters like `lower` see what ends up in the key.
    let ascii = |s: &str| deunicode::deunicode(s).trim().to_string();
    let authors: Vec<String> = bib.authors().iter().map(|a| ascii(bib::family(a))).collect();
    let title = ascii(&bib.title());
    let words: Vec<String> = title
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| w.chars().count() > 2 && !STOPWORDS.contains(&w.to_lowercase().as_str()))
        .collect();
    set("author", authors.first().cloned());
    set("word", words.first().cloned());
    set("year", bib.year().map(|y| y.to_string()));
    set("title", Some(title.clone()));
    set("venue", bib.venue());
    set("doi", bib.doi());
    set("arxiv", bib.arxiv());
    set("type", Some(bib.kind.clone()));
    vars.insert("authors", authors.into());
    vars.insert("words", words.into());
    let rendered = minijinja::Environment::new().render_str(template, vars)?;
    let key: String =
        deunicode::deunicode(&rendered).chars().filter(|c| c.is_ascii_alphanumeric() || "-_.:".contains(*c)).collect();
    let key = key.trim_start_matches(['.', '-', ':']);
    Ok(if key.is_empty() { "paper".into() } else { key.into() })
}

/// The first Markdown heading of the extracted text, which is the title for most papers.
fn guess_title(text: &str) -> Option<String> {
    let heading = text.lines().take(60).find_map(|l| l.trim().strip_prefix('#'))?;
    let title = heading.replace("**", "");
    Some(title.trim_matches(|c: char| c == '#' || c.is_whitespace()).to_string()).filter(|t| t.len() > 3)
}

fn file_stem(source: &str) -> String {
    Path::new(source).file_stem().map_or(source.into(), |s| s.to_string_lossy().into_owned())
}

fn normalize_keywords(text: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let words = text.split([',', ';', '\n']).map(|k| k.trim().trim_start_matches(['-', '*', '•']).trim());
    words.filter(|k| !k.is_empty() && seen.insert(k.to_lowercase())).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let mut bib = Entry::new("article");
        bib.set("title", Some("The Müller Effect: A Study"));
        bib.set("author", Some("Müller, Jörg and Doe, Jane"));
        bib.set("year", Some("2021"));
        assert_eq!(citekey(DEFAULT_KEY, &bib).unwrap(), "muller2021muller");
        let custom = "{{ authors | join('-') }}_{{ year }}_{{ words[:2] | join | upper }}";
        assert_eq!(citekey(custom, &bib).unwrap(), "Muller-Doe_2021_MULLEREFFECT");
        assert!(citekey("{{ year ", &bib).is_err());
        assert_eq!(normalize_keywords("- GANs, diffusion\n* gans; score matching"), "GANs\ndiffusion\nscore matching");
        assert_eq!(guess_title("x\n# **Attention Is All You Need** \n"), Some("Attention Is All You Need".into()));
    }

    #[test]
    fn library() {
        let tmp = std::env::temp_dir().join(format!("carrel-test-{}", std::process::id()));
        let mut lib = Library::open(tmp.join("lib")).unwrap();
        lib.index = tmp.join("index.sqlite");
        let dir = lib.root.join("doe2020graphs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("doe2020graphs.bib"), "@article{doe2020graphs, title = {Graphs}, author = {Doe, Jane}}")
            .unwrap();
        fs::write(dir.join("fulltext.md"), "We study spectral graph theory.").unwrap();

        lib.propose("doe2020graphs", Field::Keywords, "spectral, graphs").unwrap();
        assert_eq!(lib.drafts().len(), 1);
        assert_eq!(lib.search("spectr", 10).unwrap().len(), 1, "prefix match while typing");
        assert!(lib.search("quantum", 10).unwrap().is_empty());
        lib.approve("doe2020graphs", Field::Keywords).unwrap();
        assert_eq!(lib.get("doe2020graphs").unwrap().bib.keywords(), ["spectral", "graphs"]);
        for note in ["first", "second"] {
            lib.propose("doe2020graphs", Field::Notes, note).unwrap();
            lib.approve("doe2020graphs", Field::Notes).unwrap();
        }
        assert_eq!(lib.get("doe2020graphs").unwrap().get(Field::Notes).unwrap(), "first\n\nsecond\n");
        assert!(lib.drafts().is_empty() && !dir.join("drafts").exists());
        assert_eq!(lib.search("graphs second", 10).unwrap()[0].0.id, "doe2020graphs");

        lib.add_to_project("thesis", &["doe2020graphs".into()]).unwrap();
        assert_eq!(lib.rename("doe2020graphs", Some("doe2020spectral")).unwrap(), "doe2020spectral");
        assert_eq!(lib.project("thesis").unwrap(), ["doe2020spectral"], "projects follow renames");
        assert_eq!(lib.search("project:thesis spectral", 10).unwrap().len(), 1);
        assert_eq!(lib.search("author:doe", 10).unwrap().len(), 1);
        assert!(lib.search("title:doe", 10).unwrap().is_empty());
        let p = lib.get("doe2020spectral").unwrap();
        assert!(p.bibtex().starts_with("@article{doe2020spectral,") && p.bib.keywords().len() == 2);
        assert_eq!(lib.search("graphs", 10).unwrap()[0].0.id, "doe2020spectral", "index follows renames");
        fs::write(p.bib_path(), "@article{wrongkey, title = {Graphs}}").unwrap();
        assert!(lib.get("doe2020spectral").is_err() && lib.papers().1.len() == 1);
        assert!(lib.get("../escape").is_err() && lib.get(".tmp").is_err());
        fs::remove_dir_all(tmp).unwrap();
    }
}
