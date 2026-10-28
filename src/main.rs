mod mcp;
mod serve;
mod theme;
mod tui;

use anyhow::{Context, Result, bail};
use carrel::{Config, Field, Library, Paper};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A paper library for you and your AI. Without a command it runs the config's `default`, the TUI unless set.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// The terminal UI: browse, filter and manage the library, the citation graph, people and drafts
    Tui,
    /// Add papers by PDF file, DOI, arXiv id/URL or exact title
    Add {
        #[arg(required = true)]
        sources: Vec<String>,
    },
    /// Give a paper its PDF; without a file, download it from arXiv
    Attach { id: String, pdf: Option<PathBuf> },
    /// Add supplementary files (paths or URLs) to a paper; PDFs among them get their text extracted
    Addto {
        id: String,
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Fetch a paper's metadata again (preprints switch to their published version), keeping its key
    Update {
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        /// Which paper it is (DOI, arXiv id or title), for papers added without one
        source: Option<String>,
        /// Every paper with a DOI or arXiv id
        #[arg(long, conflicts_with_all = ["id", "source"])]
        all: bool,
    },
    /// Check the library for problems, each with the command that fixes it
    Doctor {
        /// Machine-readable output
        #[arg(long)]
        json: bool,
        /// Also list each paper without a PDF and each arXiv preprint
        #[arg(long)]
        all: bool,
    },
    /// Import every entry of a .bib file (a LaTeX project's, a Zotero export, ...), keeping its keys
    Import {
        bib: PathBuf,
        /// Also put the entries in this project
        #[arg(short, long)]
        project: Option<String>,
        /// Download the PDFs of entries with an arXiv id
        #[arg(long)]
        pdfs: bool,
    },
    /// Move an I, Librarian library over: its data folder (with database/main.db and pdfs/). Tags join the
    /// keywords, notes and highlights go into notes.md, projects stay projects, old citation keys stay usable
    #[command(name = "import-ilibrarian")]
    ImportIlibrarian { data: PathBuf },
    /// Papers a paper cites (looked up once, then kept), or with --cited-by the papers citing it
    Refs {
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        #[arg(long, conflicts_with = "refresh")]
        cited_by: bool,
        /// Look its references up again
        #[arg(long)]
        refresh: bool,
        /// Look up the references of every paper that has none yet
        #[arg(long, conflicts_with_all = ["id", "cited_by", "refresh"])]
        all: bool,
    },
    /// How papers connect. With an id: what it cites, what cites it, who shares authors with it.
    /// Without: the papers missing from the library that it cites most
    Graph {
        id: Option<String>,
        /// Only this project's papers
        #[arg(short, long, conflicts_with = "id")]
        project: Option<String>,
        /// The citations between papers, for Graphviz (`carrel graph --dot | dot -Tsvg > graph.svg`)
        #[arg(long, conflicts_with = "id")]
        dot: bool,
        /// Machine-readable output
        #[arg(long, conflicts_with = "dot")]
        json: bool,
        /// How many missing papers to show
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// The people behind the library's papers, with their spellings and co-authors. Spellings count as one
    /// person when they share co-authors or a first name
    #[command(args_conflicts_with_subcommands = true)]
    Authors {
        #[command(subcommand)]
        cmd: Option<AuthorsCmd>,
        /// Only people with a spelling containing this
        name: Option<String>,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// The library papers a paper cites, and those they cite, as a tree (or with --cited-by, who cites it)
    Tree {
        id: String,
        #[arg(long)]
        cited_by: bool,
        /// How many levels deep
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// List all papers, or a project's
    List {
        #[arg(short, long)]
        project: Option<String>,
        /// Machine-readable output: everything about each paper, with the paths of its files
        #[arg(long)]
        json: bool,
    },
    /// Search titles, authors, keywords, abstracts, summaries, notes and full text
    Search {
        #[arg(required = true)]
        query: Vec<String>,
    },
    /// Show a paper's details
    Show {
        id: String,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Edit a paper's .bib in $EDITOR
    Edit { id: String },
    /// Open a paper's PDF
    Open { id: String },
    /// Change a paper's id (citation key) everywhere; without a new id, the key template makes one
    Rename { id: String, new_id: Option<String> },
    /// Re-run text and figure extraction
    Extract {
        ids: Vec<String>,
        /// Every paper with a PDF
        #[arg(long)]
        all: bool,
    },
    /// List drafts waiting for review
    Drafts,
    /// Accept a draft into the library
    Approve { id: String, field: Field },
    /// Discard a draft
    Reject { id: String, field: Field },
    /// Print BibTeX for the given papers, a project, or the whole library
    Bib {
        ids: Vec<String>,
        #[arg(short, long)]
        project: Option<String>,
        /// Point each entry at its PDF (a `file` field), for JabRef and other reference managers
        #[arg(long)]
        files: bool,
    },
    /// Projects: named lists of papers, each with its own bibliography
    Project {
        #[command(subcommand)]
        cmd: Option<ProjectCmd>,
    },
    /// Share the library: upload what changed here, download what changed on the config's remote
    Sync {
        /// Only list what would happen
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Settle files changed on both sides by keeping this side's version
        #[arg(long)]
        keep: Option<carrel::remote::Keep>,
    },
    /// Show where carrel keeps its settings, library and index
    Config,
    /// Delete a paper and all its files
    Rm { id: String },
    /// Serve the library to AI assistants over MCP (stdio)
    Mcp,
    /// Run the library on a server: MCP over HTTP at /mcp, plus syncing, extraction and reference
    /// lookups every few minutes
    Serve {
        /// Address to listen on [default: the config's, else 127.0.0.1:8080]
        #[arg(long)]
        listen: Option<String>,
        /// Minutes between rounds of background work [default: the config's, else 5]
        #[arg(long)]
        every: Option<u64>,
    },
}

#[derive(Subcommand)]
enum AuthorsCmd {
    /// Confirm that these spellings are one person, e.g. `carrel authors same "Clark, Roger N." "Clark, R. N."`
    Same {
        #[arg(required = true, num_args = 2..)]
        spellings: Vec<String>,
    },
    /// Confirm that these spellings are different people
    Different {
        #[arg(required = true, num_args = 2..)]
        spellings: Vec<String>,
    },
    /// Accept an AI's proposal about people (`carrel drafts` lists them)
    Approve { draft: String },
    /// Discard an AI's proposal about people
    Reject { draft: String },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// List projects (the default)
    List,
    /// Show a project's papers
    Show { name: String },
    /// Add papers to a project, creating it if needed
    Add { name: String, ids: Vec<String> },
    /// Take papers out of a project; they stay in the library
    Remove {
        name: String,
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Delete a project; its papers stay in the library
    Delete { name: String },
    /// Put the papers that LaTeX (.tex) or Typst (.typ) files cite into a project
    Sync {
        name: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    // Rust ignores SIGPIPE, so `carrel list | head` would panic; die quietly like other Unix tools.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cmd = match Cli::parse().cmd {
        Some(cmd) => cmd,
        // `default = "list -p thesis"` in the config: words split on spaces, no quoting.
        None => match Config::load()?.default.filter(|d| !d.trim().is_empty()) {
            Some(default) => {
                let args = std::iter::once("carrel").chain(default.split_whitespace());
                let cli = Cli::try_parse_from(args).with_context(|| format!("the config's default {default:?}"))?;
                cli.cmd.unwrap_or(Cmd::Tui)
            }
            None => Cmd::Tui,
        },
    };
    let lib = Library::open_default()?;
    match cmd {
        Cmd::Tui => tui::run(lib)?,
        Cmd::Add { sources } => {
            let mut failed = 0;
            for source in &sources {
                match lib.add(source) {
                    Ok((id, messages)) => {
                        println!("added {id}");
                        messages.iter().for_each(|m| println!("  {m}"));
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("{source}: {e:#}");
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} of {} could not be added", sources.len());
            }
        }
        Cmd::Attach { id, pdf } => lib.attach(&id, pdf.as_deref())?.iter().for_each(|m| println!("{m}")),
        Cmd::Addto { id, files } => lib.add_supplementary(&id, &files)?.iter().for_each(|m| println!("{m}")),
        Cmd::Update { id: Some(id), source, .. } => {
            lib.update(&id, source.as_deref())?.iter().for_each(|m| println!("{m}"))
        }
        Cmd::Update { id: None, .. } => {
            let papers = lib.papers().0.into_iter().filter(|p| p.bib.doi().is_some() || p.bib.arxiv().is_some());
            for p in papers {
                match lib.update(&p.id, None) {
                    Ok(changes) => println!("{}: {}", p.id, changes.join("; ")),
                    Err(e) => println!("{}: {e:#}", p.id),
                }
            }
        }
        Cmd::Doctor { json, all } => doctor(&lib, json, all)?,
        Cmd::Sync { dry_run, keep } => sync(&lib, dry_run, keep)?,
        Cmd::ImportIlibrarian { data } => {
            let imported = lib.import_ilibrarian(&data)?;
            imported.messages.iter().for_each(|m| println!("{m}"));
            let ids: std::collections::BTreeSet<&String> = imported.papers.values().collect();
            let with_pdf = ids.iter().filter(|id| lib.get(id).is_ok_and(|p| p.pdf().is_some())).count();
            println!("{} papers, {with_pdf} with a PDF", ids.len());
            println!("next: `carrel extract --all` for their text, `carrel refs --all` for their references");
        }
        Cmd::Import { bib, project, pdfs } => {
            let (ids, messages) = lib.import_bib(&bib, pdfs)?;
            messages.iter().for_each(|m| println!("{m}"));
            if let Some(name) = project {
                lib.add_to_project(&name, &ids)?;
            }
            println!("{} entries imported or matched", ids.len());
        }
        Cmd::Refs { all: true, .. } => {
            let papers = lib.papers().0.into_iter().filter(|p| !p.dir.join(carrel::graph::FILE).exists());
            for p in papers {
                match lib.look_up_references(&p) {
                    Ok((n, Some(note))) => println!("{}: {note}; {n} references", p.id),
                    Ok((n, None)) => println!("{}: {n} references", p.id),
                    Err(e) => println!("{}: {e:#}", p.id),
                }
            }
        }
        Cmd::Refs { id: Some(id), cited_by: true, .. } => {
            for c in lib.related(&id, true)? {
                println!("{c}{}", in_library(&c.in_library));
            }
        }
        Cmd::Refs { id: Some(id), refresh, .. } => {
            for (r, found) in lib.references(&id, refresh)? {
                println!("{r}{}", in_library(&found));
            }
        }
        Cmd::Refs { id: None, .. } => unreachable!("clap requires an id without --all"),
        Cmd::Tree { id, cited_by, depth, json } => {
            let graph = lib.graph()?;
            let tree = graph.tree(&id, depth, cited_by)?;
            if json {
                let nodes: Vec<_> = tree
                    .iter()
                    .map(|(level, id, again)| serde_json::json!({"level": level, "id": id, "again": again}))
                    .collect();
                return print_json(&nodes);
            }
            println!("{id}  {}", graph.title(&id));
            for (level, id, again) in tree {
                let title = if again { "(see above)".to_string() } else { graph.title(&id) };
                println!("{}{id}  {title}", "  ".repeat(level));
            }
        }
        Cmd::Graph { id: Some(id), json, .. } => {
            let n = lib.graph()?.neighbors(&id)?;
            if json {
                return print_json(&n);
            }
            println!("cites:     {}", list(&n.cites));
            println!("cited by:  {}", list(&n.cited_by));
            for (other, authors) in &n.shared_authors {
                println!("authors:   {other} ({})", authors.join("; "));
            }
            match n.references_known {
                true => {
                    println!("{} of its references are not in the library (`carrel refs {id}`)", n.not_in_library.len())
                }
                false => println!("its references are unknown; look them up with `carrel refs {id}`"),
            }
        }
        Cmd::Graph { id: None, project, dot, json, limit } => {
            let within = project.map(|name| lib.project(&name)).transpose()?;
            let graph = lib.graph()?;
            if dot {
                print!("{}", graph.dot(within.as_deref()));
                return Ok(());
            }
            let suggestions: Vec<_> = graph.suggestions(within.as_deref()).into_iter().take(limit).collect();
            if json {
                return print_json(&suggestions);
            }
            for s in suggestions {
                println!("{:>3}× {}", s.cited_by.len(), s.reference.candidate(None));
                println!("      cited by {}", s.cited_by.join(", "));
            }
        }
        Cmd::Authors { cmd: Some(AuthorsCmd::Same { spellings }), .. } => {
            println!("{}", lib.same_person(&spellings)?);
        }
        Cmd::Authors { cmd: Some(AuthorsCmd::Different { spellings }), .. } => {
            println!("{}", lib.different_people(&spellings)?.join(", "));
        }
        Cmd::Authors { cmd: Some(AuthorsCmd::Approve { draft }), .. } => lib.decide_people(&draft, true)?,
        Cmd::Authors { cmd: Some(AuthorsCmd::Reject { draft }), .. } => lib.decide_people(&draft, false)?,
        Cmd::Authors { cmd: None, name, json } => {
            let graph = lib.graph()?;
            let people = graph.authors.people(name.as_deref());
            if json {
                return print_json(&people);
            }
            for p in people {
                let others: Vec<&str> = p.variants.iter().map(String::as_str).filter(|v| *v != p.name).collect();
                let also = if others.is_empty() { String::new() } else { format!("  (also {})", others.join("; ")) };
                let id = p.id.as_ref().map_or(String::new(), |id| format!("  [confirmed: {id}]"));
                println!("{}{also}{id}", p.name);
                println!("  papers:     {}", p.papers.join(", "));
                let with: Vec<String> = p.coauthors.iter().map(|(name, n)| format!("{name} ({n})")).collect();
                println!("  co-authors: {}", list(&with));
                if !p.maybe.is_empty() {
                    let maybe: Vec<String> = p.maybe.iter().map(|m| m.to_string()).collect();
                    println!("  maybe also: {}  [no co-author or first name in common]", maybe.join("; "));
                }
            }
        }
        Cmd::List { project, json } => {
            let papers = match project {
                Some(name) => project_papers(&lib, &name)?,
                None => {
                    let (papers, broken) = lib.papers();
                    broken.iter().for_each(|b| eprintln!("warning: {b}"));
                    papers
                }
            };
            match json {
                true => print_json(&papers.iter().map(|p| mcp::details(&lib, p)).collect::<Vec<_>>())?,
                false => papers.iter().for_each(print_line),
            }
        }
        Cmd::Search { query } => {
            for (p, snippet) in lib.search(&query.join(" "), 50)? {
                print_line(&p);
                println!("{:30}   {snippet}", "");
            }
        }
        Cmd::Show { id, json: true } => print_json(&mcp::details(&lib, &lib.get(&id)?))?,
        Cmd::Show { id, .. } => show(&lib, &lib.get(&id)?),
        Cmd::Edit { id } => {
            edit(&lib.bib_path(&id)?)?;
            lib.get(&id)?; // complain right away if the edit broke it
        }
        Cmd::Open { id } => open(&lib.get(&id)?.pdf().context("this paper has no PDF")?)?,
        Cmd::Rename { id, new_id } => println!("{}", lib.rename(&id, new_id.as_deref())?),
        Cmd::Extract { ids, all } => {
            let ids = match all {
                true => lib.papers().0.into_iter().filter(|p| p.pdf().is_some()).map(|p| p.id).collect(),
                false => ids,
            };
            for id in ids {
                print!("{id} ... ");
                std::io::stdout().flush()?;
                match lib.extract(&id) {
                    Ok(()) => println!("ok"),
                    Err(e) => println!("{e:#}"),
                }
            }
        }
        Cmd::Drafts => {
            for (p, field) in lib.drafts() {
                println!("{:<30} {}", p.id, field.name());
            }
            for d in lib.people_drafts() {
                let what = if d.same { "one person" } else { "different people" };
                println!("people {:<23} {what}: {}  ({})", d.id, d.spellings.join("; "), d.reason);
            }
        }
        Cmd::Approve { id, field } => lib.approve(&id, field)?,
        Cmd::Reject { id, field } => lib.reject(&id, field)?,
        Cmd::Bib { ids, project, files } => {
            let papers = match project {
                Some(name) => project_papers(&lib, &name)?,
                None if ids.is_empty() => lib.papers().0,
                None => ids.iter().map(|id| lib.get(id)).collect::<Result<_>>()?,
            };
            for p in papers {
                match p.pdf().filter(|_| files) {
                    // JabRef's format, `description:path:type`, with `:` and `;` in the path escaped.
                    Some(pdf) => {
                        let path =
                            pdf.display().to_string().replace('\\', "\\\\").replace(':', "\\:").replace(';', "\\;");
                        let mut bib = p.bib.clone();
                        bib.set("file", Some(&format!(":{path}:PDF")));
                        println!("{}", bib.to_bib(&p.id));
                    }
                    None => println!("{}", p.bibtex()),
                }
            }
        }
        Cmd::Project { cmd } => match cmd.unwrap_or(ProjectCmd::List) {
            ProjectCmd::List => {
                for name in lib.projects() {
                    let n = lib.project(&name)?.len();
                    println!("{name:<30} {n} paper{}", if n == 1 { "" } else { "s" });
                }
            }
            ProjectCmd::Show { name } => project_papers(&lib, &name)?.iter().for_each(print_line),
            ProjectCmd::Add { name, ids } => lib.add_to_project(&name, &ids)?,
            ProjectCmd::Remove { name, ids } => lib.remove_from_project(&name, &ids)?,
            ProjectCmd::Delete { name } => lib.delete_project(&name)?,
            ProjectCmd::Sync { name, files } => {
                let (added, missing, uncited) = lib.sync_project(&name, &files)?;
                println!("added to {name}: {}", list(&added));
                println!("cited but not in the library (or not citations): {}", list(&missing));
                println!("in {name} but cited nowhere: {}", list(&uncited));
            }
        },
        Cmd::Config => {
            let path = Config::path().map_or("unknown".into(), |p| p.display().to_string());
            println!("config:  {path}");
            println!("library: {}", lib.root.display());
            println!("key:     {}", lib.key);
            println!("index:   {}", lib.index.display());
            println!("theme:   {}", theme::Theme::configured()?.name);
            let tokens = [
                ("ads", carrel::token(&["ADS_API_TOKEN", "ADS_DEV_KEY"], |t| t.ads.as_ref())?),
                ("s2", carrel::token(&["S2_API_KEY"], |t| t.s2.as_ref())?),
                ("openalex", carrel::token(&["OPENALEX_API_KEY"], |t| t.openalex.as_ref())?),
            ];
            let found: Vec<_> = tokens.iter().filter(|(_, t)| t.is_some()).map(|(name, _)| *name).collect();
            println!("tokens:  {}", if found.is_empty() { "none".into() } else { found.join(", ") });
        }
        Cmd::Rm { id } => lib.remove(&id)?,
        Cmd::Serve { listen, every } => serve::run(lib, listen, every)?,
        Cmd::Mcp => tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(mcp::serve(lib))?,
    }
    Ok(())
}

fn sync(lib: &Library, dry_run: bool, keep: Option<carrel::remote::Keep>) -> Result<()> {
    use carrel::remote::Action;
    let config = Config::load()?.remote.context("no remote to sync with; add [remote] to the config")?;
    let remote = config.open()?;
    println!("syncing with {}", remote.name());
    let report = lib.sync(&*remote, keep, dry_run, |path, action| {
        let arrow = match action {
            Action::Upload => "↑",
            Action::Download => "↓",
            Action::DeleteRemote => "✗ remote",
            Action::DeleteLocal => "✗ here",
            Action::Conflict => "! conflict",
        };
        println!("{arrow:<10} {path}");
    })?;
    let verb = if dry_run { "would " } else { "" };
    println!("{verb}{}", report.summary());
    let conflicts = report.count(Action::Conflict);
    if conflicts > 0 {
        println!("{conflicts} changed on both sides and left alone; settle them with --keep local or --keep remote");
    }
    for (path, error) in &report.failed {
        eprintln!("failed: {path}: {error}");
    }
    if !report.failed.is_empty() {
        bail!("{} files failed; run carrel sync again", report.failed.len());
    }
    Ok(())
}

fn doctor(lib: &Library, json: bool, all: bool) -> Result<()> {
    let issues = lib.doctor();
    if json {
        print_json(&issues)?;
        return Ok(());
    }
    let (shown, hidden): (Vec<_>, Vec<_>) = issues.iter().partition(|i| all || i.level != "info");
    for i in &shown {
        let fix = i.fix.as_ref().map_or(String::new(), |fix| format!("  → {fix}"));
        println!("{:<8}{:<30}{}{fix}", i.level, i.id, i.message);
    }
    let count = |check| hidden.iter().filter(|i| i.check == check).count();
    if !hidden.is_empty() {
        println!("and {} without a PDF, {} arXiv preprints (--all lists them)", count("pdf"), count("preprint"));
    }
    if issues.is_empty() {
        println!("all good");
    }
    Ok(())
}

fn list(ids: &[String]) -> String {
    if ids.is_empty() { "none".into() } else { ids.join(", ") }
}

/// A project's papers, warning about ids it lists that aren't in the library.
fn project_papers(lib: &Library, name: &str) -> Result<Vec<Paper>> {
    let (papers, missing) = lib.project_papers(name)?;
    missing.iter().for_each(|id| eprintln!("warning: project {name} lists {id}, which is not in the library"));
    Ok(papers)
}

fn in_library(id: &Option<String>) -> String {
    id.as_ref().map_or(String::new(), |id| format!("  [in library: {id}]"))
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_line(p: &Paper) {
    let year = p.bib.year().map(|y| y.to_string()).unwrap_or_default();
    println!("{:<30} {year:>4}  {}", p.id, p.bib.title());
}

fn show(lib: &Library, p: &Paper) {
    let b = &p.bib;
    println!("{}\n{}", b.title(), b.authors().join("; "));
    let ids = [
        b.year().map(|y| y.to_string()),
        b.venue(),
        b.doi().map(|d| format!("doi:{d}")),
        b.arxiv().map(|a| format!("arXiv:{a}")),
    ];
    println!("{}", ids.into_iter().flatten().collect::<Vec<_>>().join(" · "));
    let keywords = b.keywords();
    if !keywords.is_empty() {
        println!("keywords: {}", keywords.join(", "));
    }
    let projects = lib.projects_of(&p.id);
    if !projects.is_empty() {
        println!("projects: {}", projects.join(", "));
    }
    let supplementary = p.supplementary();
    if !supplementary.is_empty() {
        println!("supplementary: {}", supplementary.join(", "));
    }
    println!("files: {}", p.dir.display());
    for field in p.drafts() {
        println!("draft waiting: {0} (carrel approve {1} {0})", field.name(), p.id);
    }
    if let Some(text) = p.get(Field::Summary).or(b.get("abstract")) {
        println!("\n{}", text.trim());
    }
}

/// Open a file in $VISUAL / $EDITOR (default vi).
pub fn edit(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    // Through sh, so EDITOR may carry arguments ("code --wait").
    let status = Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path).status()?;
    if !status.success() {
        bail!("{editor} exited with {status}");
    }
    Ok(())
}

/// Open a file with the desktop's default app.
pub fn open(path: &Path) -> Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(opener).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}
