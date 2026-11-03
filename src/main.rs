mod auth;
mod history;
mod install;
mod mcp;
mod serve;
mod theme;
mod tui;
mod web;

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
    /// Add papers by PDF file, DOI, arXiv id/URL, ISBN, ADS bibcode/URL or exact title
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
        /// Which paper it is (DOI, arXiv id, ISBN, ADS bibcode or title), for papers added without one
        source: Option<String>,
        /// Every paper with a DOI, arXiv id or ADS bibcode, and every book
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
        /// Also look up every DOI, to find those that belong to another work
        #[arg(long)]
        online: bool,
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
        /// Look up the references (and authors' institutions) of every paper that has none yet
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
        /// Search by meaning, with the embeddings from `carrel embed` (needs [embeddings] in the config)
        #[arg(short, long)]
        semantic: bool,
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
    /// Compute the embeddings for semantic search, on the Ollama in the config's [embeddings]
    Embed {
        ids: Vec<String>,
        /// Every paper whose embeddings are missing or out of date
        #[arg(long, conflicts_with = "ids")]
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
        /// For JabRef: link each entry's PDF and supplementary files (`file`), and write projects as groups
        #[arg(long, alias = "files")]
        jabref: bool,
    },
    /// Projects: named lists of papers, each with its own bibliography
    Project {
        #[command(subcommand)]
        cmd: Option<ProjectCmd>,
    },
    /// Papers recommended by alert emails (Google Scholar), most alike to the library first, waiting to be
    /// added or dismissed
    #[command(args_conflicts_with_subcommands = true)]
    Recommended {
        #[command(subcommand)]
        cmd: Option<RecommendedCmd>,
        /// Also the dismissed ones
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Share the library: upload what changed here, download what changed on the config's remote
    Sync {
        /// Only list what would happen
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Settle files changed on both sides by keeping this side's version
        #[arg(long)]
        keep: Option<carrel::remote::Keep>,
        /// List every recommendation file too, not just how many
        #[arg(short, long)]
        verbose: bool,
    },
    /// Move new PDFs from Downloads (or the config's [consume] folders) into the library, asking for each.
    /// Without a terminal (from a timer) it doesn't ask: it tells through the notifiers, or with
    /// `auto = true` adds the PDFs it can identify as papers
    Consume {
        /// Folders to look in [default: the config's, else Downloads]
        folders: Vec<String>,
        /// Add the PDFs it can identify as papers without asking
        #[arg(short, long)]
        yes: bool,
        /// Also offer the PDFs you said no to before
        #[arg(long)]
        again: bool,
        /// Only PDFs changed in the last this many days [default: the config's, else 30]
        #[arg(long)]
        days: Option<u64>,
    },
    /// Send a message through the config's notifiers, to try them
    Notify {
        #[arg(default_value = "carrel can reach you this way.")]
        message: String,
    },
    /// Show where carrel keeps its settings, library and index
    Config,
    /// Delete a paper and all its files
    Rm { id: String },
    /// Serve the library to AI assistants over MCP (stdio)
    Mcp,
    /// Connect an AI agent to the library over MCP; without one, which agents have it
    Integrate {
        agent: Option<install::Agent>,
        /// Disconnect it instead
        #[arg(long, requires = "agent")]
        remove: bool,
    },
    /// Connect this machine to a carrel server and sync through it every 10 minutes. Without a token, from the
    /// server's trusted networks, it downloads and adds papers; with one from its web UI's Tokens page it also
    /// shares changes
    Connect {
        /// The server's address, such as https://carrel.example.org
        url: String,
        /// Read the token from this file; without it, the one kept from before
        /// (`~/.config/carrel/tokens/carrel.txt`), or else it asks
        #[arg(long)]
        token: Option<PathBuf>,
        /// Run the web UI on this machine too
        #[arg(long)]
        web: bool,
        /// Only write the config, set up no services
        #[arg(long)]
        no_service: bool,
    },
    /// Run `carrel web` or `carrel consume` in the background (systemd on Linux, launchd on macOS);
    /// without a command, what runs
    Service {
        #[command(subcommand)]
        cmd: Option<ServiceCmd>,
    },
    /// The library in your browser, on this machine: prints the link to open and opens it
    Web {
        /// Address to listen on, on loopback [default: 127.0.0.1:7700]
        #[arg(long)]
        listen: Option<String>,
        /// Don't open the browser
        #[arg(long)]
        no_open: bool,
        /// Only print the link with the token, for a `carrel web` running as a service, and exit
        #[arg(long, conflicts_with = "no_open")]
        get_url: bool,
    },
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
enum ServiceCmd {
    /// Start it now and at every login
    Install {
        service: install::Service,
        /// Where `carrel web` listens, on loopback
        #[arg(long, default_value = "127.0.0.1:7700")]
        listen: String,
    },
    /// Stop it and remove it
    Remove { service: install::Service },
}

#[derive(Subcommand)]
enum RecommendedCmd {
    /// Read alert emails: .eml files, or folders of them (`carrel consume` reads those in its folders)
    Import {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Add recommendations to the library, with their PDF when the alert links to one that's free
    Add {
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Hide recommendations for good
    Dismiss {
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Score them all again by how alike they are to the library (needs [embeddings])
    Score,
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
    // Setting up the machine needs no library.
    match cmd {
        Cmd::Connect { url, token, web, no_service } => return connect(&url, token, web, no_service),
        Cmd::Integrate { agent: Some(agent), remove } => {
            println!("{}", install::integrate(agent, remove)?);
            return Ok(());
        }
        Cmd::Integrate { agent: None, .. } => {
            for agent in install::AGENTS {
                let state = match install::integrated(agent)? {
                    Some(true) => "connected",
                    Some(false) => "not connected; `carrel integrate {}`",
                    None => "not installed",
                };
                println!("{:<9} {}", agent.name(), state.replace("{}", agent.name()));
            }
            return Ok(());
        }
        Cmd::Service { cmd: Some(ServiceCmd::Install { service, listen }) } => {
            install::install(service, &listen)?.iter().for_each(|f| println!("wrote {}", f.display()));
            println!("{}", install::status(service)?);
            if service == install::Service::Web {
                println!("`carrel web --get-url` prints the link to open");
            }
            if cfg!(target_os = "macos") && service == install::Service::Consume {
                println!(
                    "macOS asks before carrel may read Downloads; if it doesn't, allow it under Privacy & Security"
                );
            }
            return Ok(());
        }
        Cmd::Service { cmd: Some(ServiceCmd::Remove { service }) } => {
            let removed = install::remove(service)?;
            println!("{}", if removed { "removed" } else { "wasn't installed" });
            return Ok(());
        }
        Cmd::Service { cmd: None } => {
            for service in [install::Service::Web, install::Service::Consume, install::Service::Sync] {
                println!("{}", install::status(service)?);
            }
            return Ok(());
        }
        _ => {}
    }
    let lib = Library::open_default()?;
    match cmd {
        Cmd::Tui => tui::run(lib)?,
        Cmd::Add { sources } => {
            let mut failed = 0;
            for source in &sources {
                match spinning(&format!("adding {source}"), || lib.add(source)) {
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
        Cmd::Attach { id, pdf } => spinning(&format!("attaching to {id}"), || lib.attach(&id, pdf.as_deref()))?
            .iter()
            .for_each(|m| println!("{m}")),
        Cmd::Addto { id, files } => lib.add_supplementary(&id, &files)?.iter().for_each(|m| println!("{m}")),
        Cmd::Update { id: Some(id), source, .. } => {
            lib.update(&id, source.as_deref())?.iter().for_each(|m| println!("{m}"))
        }
        Cmd::Update { id: None, .. } => {
            let known = |p: &Paper| {
                p.bib.doi().is_some() || p.bib.arxiv().is_some() || p.bib.kind == "book" || p.bib.bibcode().is_some()
            };
            let papers = lib.papers().0.into_iter().filter(known);
            for p in papers {
                match lib.update(&p.id, None) {
                    Ok(changes) => println!("{}: {}", p.id, changes.join("; ")),
                    Err(e) => println!("{}: {e:#}", p.id),
                }
            }
        }
        Cmd::Doctor { json, all, online } => doctor(&lib, json, all, online)?,
        Cmd::Sync { dry_run, keep, verbose } => sync(&lib, dry_run, keep, verbose)?,
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
            // Only papers OpenAlex can find: by DOI or arXiv id.
            let missing = |p: &carrel::Paper| {
                carrel::affiliations::read(&p.dir).is_none() && (p.bib.doi().is_some() || p.bib.arxiv().is_some())
            };
            for p in lib.papers().0.into_iter().filter(|p| p.dir.join(carrel::graph::FILE).exists() && missing(p)) {
                match lib.look_up_affiliations(&p) {
                    Ok(n) => println!("{}: {n} institutions", p.id),
                    Err(e) => println!("{}: institutions: {e:#}", p.id),
                }
            }
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
        Cmd::Search { query, semantic } => {
            let hits = match semantic {
                true => lib.semantic_search(&ollama(&lib)?, &query.join(" "), 20)?,
                false => lib.search(&query.join(" "), 50)?,
            };
            for (p, snippet) in hits {
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
                match spinning(&format!("extracting {id}"), || lib.extract(&id)) {
                    Ok(()) => println!("{id}: ok"),
                    Err(e) => println!("{id}: {e:#}"),
                }
            }
        }
        Cmd::Embed { ids, all } => {
            let ollama = ollama(&lib)?;
            println!("embedding with {} on {}", ollama.model, ollama.url);
            let papers = match all {
                true => lib.papers().0,
                false => ids.iter().map(|id| lib.get(id)).collect::<Result<_>>()?,
            };
            let mut failed = 0;
            for p in papers {
                match spinning(&format!("embedding {}", p.id), || lib.embed(&ollama, &p)) {
                    Ok(true) => println!("{}: done", p.id),
                    Ok(false) if !all => println!("{}: up to date", p.id),
                    Ok(false) => {}
                    Err(e) => {
                        failed += 1;
                        println!("{}: {e:#}", p.id);
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} papers failed");
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
        Cmd::Bib { ids, project, jabref } => {
            let papers: Vec<Paper> = match project {
                Some(name) => project_papers(&lib, &name)?,
                None if ids.is_empty() => lib.papers().0,
                None => ids.iter().map(|id| lib.get(id)).collect::<Result<_>>()?,
            };
            match jabref {
                true => print!("{}", lib.jabref_bib(&papers)),
                false => papers.iter().for_each(|p| println!("{}", p.bibtex())),
            }
        }
        Cmd::Recommended { cmd, all, json } => match cmd {
            None => {
                let recs = lib.recommendations(all);
                if json {
                    return print_json(&recs);
                }
                if recs.is_empty() {
                    println!("no recommendations; `carrel recommended import <folder of .eml alerts>` reads some");
                }
                for r in recs {
                    print_recommendation(&r);
                }
            }
            Some(RecommendedCmd::Import { paths }) => {
                let imported = spinning("reading the alerts", || lib.import_alerts(&paths))?;
                println!("{} alerts, {} new recommendations", imported.alerts, imported.new.len());
                imported.new.iter().take(10).for_each(print_recommendation);
                if imported.new.len() > 10 {
                    println!("and {} more: `carrel recommended`", imported.new.len() - 10);
                }
            }
            Some(RecommendedCmd::Add { keys }) => {
                for key in keys {
                    match spinning("adding", || lib.add_recommended(&key)) {
                        Ok((id, messages)) => {
                            println!("added {id}");
                            messages.iter().for_each(|m| println!("  {m}"));
                        }
                        Err(e) => eprintln!("{key}: {e:#}"),
                    }
                }
            }
            Some(RecommendedCmd::Dismiss { keys }) => {
                for key in keys {
                    lib.dismiss(&key)?;
                }
            }
            Some(RecommendedCmd::Score) => {
                if lib.embeddings.is_none() {
                    bail!("scoring needs [embeddings] in the config (see `carrel embed --help`)");
                }
                let mut recs = lib.recommendations(true);
                spinning("comparing them with the library", || lib.score(&mut recs))?;
                for r in &recs {
                    lib.save_recommendation(r)?;
                }
                println!("scored {}", recs.len());
            }
        },
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
            let extractor = match lib.extractor() {
                Ok(Some(extractor)) => extractor.name(),
                Ok(None) => "none; install uv or docling to extract text from PDFs".into(),
                Err(e) => format!("{e:#}"),
            };
            println!("extractor: {extractor}");
            let tokens = [
                ("ads", carrel::token(&["ADS_API_TOKEN", "ADS_DEV_KEY"], |t| t.ads.as_ref())?),
                ("s2", carrel::token(&["S2_API_KEY"], |t| t.s2.as_ref())?),
                ("openalex", carrel::token(&["OPENALEX_API_KEY"], |t| t.openalex.as_ref())?),
                ("docling", carrel::token(&["DOCLING_SERVE_API_KEY"], |t| t.docling.as_ref())?),
            ];
            let found: Vec<_> = tokens.iter().filter(|(_, t)| t.is_some()).map(|(name, _)| *name).collect();
            println!("tokens:  {}", if found.is_empty() { "none".into() } else { found.join(", ") });
            let embeddings = match &lib.embeddings {
                None => "off; add [embeddings] for semantic search".into(),
                Some(config) => match carrel::embed::Ollama::connect(config) {
                    Ok(o) => format!("{} on {}", o.model, o.url),
                    Err(e) => format!("not working: {e:#}"),
                },
            };
            println!("embeddings: {embeddings}");
        }
        Cmd::Consume { folders, yes, again, days } => consume(&lib, folders, yes, again, days)?,
        Cmd::Notify { message } => {
            let config = Config::load()?.notify;
            let notifiers = carrel::notify::notifiers(&config);
            if notifiers.is_empty() {
                bail!(
                    "no notifiers; add [notify.email], [notify.telegram] or `desktop = true` under [notify] to the config"
                );
            }
            let failed = carrel::notify::send(&config, "carrel", &format!("{message}\n"));
            for n in &notifiers {
                let ok = !failed.iter().any(|f| f.starts_with(&n.name()));
                println!("{}: {}", n.name(), if ok { "sent" } else { "failed" });
            }
            if !failed.is_empty() {
                bail!("{}", failed.join("\n"));
            }
        }
        Cmd::Rm { id } => lib.remove(&id)?,
        Cmd::Serve { listen, every } => serve::run(lib, listen, every)?,
        Cmd::Web { listen, get_url: true, .. } => println!("{}", web::url(listen)?),
        Cmd::Web { listen, no_open, .. } => web::run(lib, listen, !no_open)?,
        Cmd::Mcp => tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(mcp::serve(lib))?,
        Cmd::Integrate { .. } | Cmd::Service { .. } | Cmd::Connect { .. } => {
            unreachable!("handled before the library opens")
        }
    }
    Ok(())
}

/// Run `work` with a spinner and the seconds so far on the terminal, so that a long PDF doesn't look
/// like a hang. Nothing is shown when stderr isn't a terminal.
fn spinning<T>(message: &str, work: impl FnOnce() -> T) -> T {
    use std::io::IsTerminal;
    use std::sync::atomic::{AtomicBool, Ordering};
    if !std::io::stderr().is_terminal() {
        return work();
    }
    let done = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            let start = std::time::Instant::now();
            for frame in ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"].iter().cycle() {
                if done.load(Ordering::Relaxed) {
                    break;
                }
                eprint!("\r{frame} {message} {}s", start.elapsed().as_secs());
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            eprint!("\r\x1b[2K"); // clear the line for what comes next
        });
        let result = work();
        done.store(true, Ordering::Relaxed);
        result
    })
}

fn consume(lib: &Library, folders: Vec<String>, yes: bool, again: bool, days: Option<u64>) -> Result<()> {
    use carrel::consume::{self, Mark, Seen};
    use std::io::IsTerminal;
    let config = Config::load()?;
    let mut settings = config.consume.unwrap_or_default();
    settings.days = days.or(settings.days);
    if !folders.is_empty() {
        settings.folders = folders;
    }
    let folders = settings.folders();
    if folders.is_empty() {
        bail!("no folder to look in; set [consume] folders in the config");
    }
    let interactive = std::io::stdin().is_terminal();
    if yes || !interactive {
        let Some((subject, body)) = consume::unattended(lib, &settings, yes || settings.auto, yes)? else {
            println!(
                "nothing new in {}",
                folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
            );
            return Ok(());
        };
        println!("{body}");
        if !interactive {
            for failed in carrel::notify::send(&config.notify, &subject, &body) {
                eprintln!("cannot notify: {failed}");
            }
        }
        return Ok(());
    }

    let (added, waiting) = spinning("reading alert emails", || consume::recommendations(lib, &settings));
    if !added.is_empty() {
        println!("added from the alerts:\n{}\n", added.join("\n"));
    }
    if !waiting.is_empty() {
        println!("{} new recommendations from alert emails: `carrel recommended`\n", waiting.len());
    }
    let mut seen = Seen::load();
    let found: Vec<_> = consume::find(&folders, settings.days())
        .into_iter()
        .filter(|f| again || seen.get(f) != Some(Mark::Skipped))
        .collect();
    if found.is_empty() {
        let where_ = folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ");
        println!("no new PDFs from the last {} days in {where_}", settings.days());
        return Ok(());
    }
    println!("{} PDFs from the last {} days. Added ones move out of their folder.", found.len(), settings.days());
    println!("Enter adds those naming a DOI or arXiv id [Y/n] and skips the rest [y/N]; q stops.\n");
    for f in found {
        let question = match &f.clue {
            Some(clue) => format!("{clue}: add? [Y/n/q] "),
            None => "no DOI or arXiv id in its name or metadata: add? [y/N/q] ".into(),
        };
        print!("{}\n  {question}", f.path.display());
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        match answer.trim().to_lowercase().as_str() {
            "q" => break,
            "y" | "yes" => {}
            "" if f.clue.is_some() => {}
            _ => {
                seen.set(&f, Mark::Skipped);
                continue;
            }
        }
        match spinning("extracting text and figures, then looking it up", || lib.consume(&f, false)) {
            Ok((id, messages)) => {
                println!("  added {id}");
                messages.iter().for_each(|m| println!("  {m}"));
            }
            Err(e) => {
                println!("  {e:#}");
                seen.set(&f, Mark::Told);
            }
        }
    }
    seen.save()
}

fn ollama(lib: &Library) -> Result<carrel::embed::Ollama> {
    let config =
        lib.embeddings.as_ref().context("semantic search needs [embeddings] in the config (where Ollama is)")?;
    carrel::embed::Ollama::connect(config)
}

fn sync(lib: &Library, dry_run: bool, keep: Option<carrel::remote::Keep>, verbose: bool) -> Result<()> {
    use carrel::remote::Action;
    let config = Config::load()?;
    let remote = config.remote.as_ref().context("no remote to sync with; add [remote] to the config")?.open()?;
    println!("syncing with {}", remote.name());
    let arrow = |action| match action {
        Action::Upload => "↑",
        Action::Download => "↓",
        Action::DeleteRemote => "✗ remote",
        Action::DeleteLocal => "✗ here",
        Action::Conflict => "! conflict",
    };
    // Recommendations come by the hundred: counted, unless asked for or in conflict.
    let mut recommended: Vec<(Action, usize)> = Vec::new();
    let report = lib.sync(&*remote, keep, dry_run, |path, action| {
        if !verbose && action != Action::Conflict && path.starts_with("recommended/") {
            match recommended.iter_mut().find(|(a, _)| *a == action) {
                Some((_, n)) => *n += 1,
                None => recommended.push((action, 1)),
            }
        } else {
            println!("{:<10} {path}", arrow(action));
        }
    })?;
    for (action, n) in recommended {
        println!("{:<10} {n} recommendation{} (--verbose lists them)", arrow(action), if n == 1 { "" } else { "s" });
    }
    let verb = if dry_run { "would " } else { "" };
    println!("{verb}{}", report.summary());
    let conflicts = report.count(Action::Conflict);
    if conflicts > 0 {
        println!("{conflicts} changed on both sides and left alone; settle them with --keep local or --keep remote");
    }
    if !report.settled.is_empty() && !dry_run {
        let n = report.settled.len();
        let (kept, yours) = match &report.conflicts {
            Some(dir) => ("the shared version", format!("; yours are in {}", dir.display())),
            None => ("your version", String::new()),
        };
        let body = format!(
            "{n} changed here and by someone else since the last sync; kept {kept}{yours}:\n{}\n",
            report.settled.join("\n")
        );
        print!("{body}");
        // From the sync service, which no one watches; only about what someone wrote, as text, figures and
        // the like are made again from the PDF.
        let written: Vec<&String> = report.settled.iter().filter(|p| !carrel::remote::generated(p)).collect();
        if !written.is_empty() && !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            let n = written.len();
            let list = written.iter().map(|p| p.as_str()).collect::<Vec<_>>().join("\n");
            let body =
                format!("{n} changed here and by someone else since the last sync; kept {kept}{yours}:\n{list}\n");
            for failed in carrel::notify::send(&config.notify, &format!("carrel: {n} sync conflicts settled"), &body) {
                eprintln!("cannot notify: {failed}");
            }
        }
    }
    if !report.unshared.is_empty() {
        println!(
            "{} changed here but not shared: {} takes changes only from members; make a token on its web UI \
             and put it in [remote] token",
            report.unshared.len(),
            remote.name()
        );
    }
    if !report.restored.is_empty() {
        println!(
            "{} deleted here, but {} keeps them (no token to delete with, or an admin said no); downloaded again",
            report.restored.len(),
            remote.name()
        );
    }
    if !report.asked.is_empty() {
        println!("{} deletions asked for at {}; an admin decides", report.asked.len(), remote.name());
    }
    for path in &report.skipped {
        eprintln!("warning: skipped {path:?} on the remote: not a file path inside the library");
    }
    for (path, error) in &report.failed {
        eprintln!("failed: {path}: {error}");
    }
    if !report.failed.is_empty() {
        bail!("{} files failed; run carrel sync again", report.failed.len());
    }
    Ok(())
}

fn doctor(lib: &Library, json: bool, all: bool, online: bool) -> Result<()> {
    let mut issues = lib.doctor();
    if online {
        issues.extend(spinning("looking up every DOI", || lib.check_dois()));
        carrel::doctor::sort(&mut issues);
    }
    if json {
        print_json(&issues)?;
        return Ok(());
    }
    let (shown, hidden): (Vec<_>, Vec<_>) = issues.iter().partition(|i| all || i.level != "info");
    for i in &shown {
        let fix = i.fix.as_ref().map_or(String::new(), |fix| format!("  → {fix}"));
        println!("{:<8}{:<29} {}{fix}", i.level, i.id, i.message);
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

/// Score, title, then byline and key: `carrel recommended add <key>`.
fn print_recommendation(r: &carrel::recommended::Recommendation) {
    let score = r.score.map_or("    ".into(), |s| format!("{s:.2}"));
    let times = if r.dates.len() > 1 { format!(", recommended {}×", r.dates.len()) } else { String::new() };
    let pdf = if r.pdf { ", PDF" } else { "" };
    let venue = [r.venue.clone(), r.year.map(|y| y.to_string())].into_iter().flatten().collect::<Vec<_>>();
    let mut byline = r.authors.first().cloned().unwrap_or_default();
    if r.authors.len() > 1 {
        byline.push_str(" et al.");
    }
    if !venue.is_empty() {
        byline.push_str(&format!(" · {}", venue.join(", ")));
    }
    println!("{score}  {}\n      {byline}{pdf}{times}  [{}]", r.title, r.key);
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
/// `carrel connect`: `[remote]` pointing at a server, a token kept beside the config, and the sync service.
fn connect(url: &str, token_file: Option<PathBuf>, web: bool, no_service: bool) -> Result<()> {
    use std::fs;
    use std::io::IsTerminal;
    let url = url.trim_end_matches('/').to_string();
    let me = |token: Option<&str>| -> Result<serde_json::Value> {
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let mut request = agent.get(format!("{url}/api/me")).header("X-Carrel-Client", carrel::remote::identity(None));
        if let Some(token) = token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request.call().with_context(|| format!("cannot reach {url}"))?;
        response.body_mut().read_json().with_context(|| format!("{url} doesn't answer like a carrel server"))
    };
    let anonymous = me(None)?;
    if anonymous["server"] != true {
        bail!("{url} is a carrel web for one machine, not a server");
    }
    let config = Config::path().context("no folder for the config")?;
    let dir = config.parent().context("no folder for the config")?.to_path_buf();
    // Where it keeps the token, so connecting again needs no new one.
    let kept = dir.join("tokens").join("carrel.txt");
    let saved = fs::read_to_string(&kept).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let saved = saved.filter(|t| me(Some(t)).is_ok_and(|who| matches!(who["role"].as_str(), Some("member" | "admin"))));
    let token = match (token_file, saved) {
        (Some(file), _) => Some(fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?),
        (None, Some(saved)) => Some(saved),
        (None, None) if std::io::stdin().is_terminal() => {
            let tokens = format!("{url}/#/tokens");
            println!("A token lets this machine share changes too: make one for it at {tokens} (opening it now).");
            let _ = open(Path::new(&tokens));
            Some(read_secret("Paste it here, or press Enter to connect without one: ")?)
        }
        (None, None) => None,
    };
    let token = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let quoted = |s: &str| toml::Value::from(s).to_string();
    let table = match &token {
        Some(token) => {
            let who = me(Some(token))?;
            if !matches!(who["role"].as_str(), Some("member" | "admin")) {
                bail!("{url} doesn't know that token; make one on its Tokens page");
            }
            println!("connected as {} ({})", who["user"].as_str().unwrap_or("?"), who["role"].as_str().unwrap_or("?"));
            // Beside the other tokens of `[tokens]`.
            let file = kept.clone();
            fs::create_dir_all(file.parent().unwrap())?;
            use std::os::unix::fs::OpenOptionsExt;
            let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&file)?;
            out.write_all(token.as_bytes())?;
            format!(
                "[remote]\ntype = \"carrel\"\nurl = {}\ntoken = {{ file = {} }}\n",
                quoted(&url),
                quoted(&file.to_string_lossy())
            )
        }
        None => {
            if anonymous["role"] != "anonymous" {
                bail!("{url} lets this machine in only with a token; make one on its Tokens page");
            }
            println!(
                "connected without a token, as {}: you can download and add papers; changes you make stay here",
                carrel::remote::identity(None)
            );
            format!("[remote]\ntype = \"carrel\"\nurl = {}\n", quoted(&url))
        }
    };
    // `[remote]` replaced, its `name` kept; the rest of the config as it was, and the old one beside it.
    let old = fs::read_to_string(&config).unwrap_or_default();
    let (mut kept, mut name, mut inside, mut had) = (String::new(), None, false, false);
    for line in old.lines() {
        if line.trim_start().starts_with('[') {
            inside = line.trim() == "[remote]" || line.trim().starts_with("[remote.");
            had |= inside;
        }
        if inside && line.trim_start().starts_with("name") {
            name = Some(line.to_string());
        }
        if !inside {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let table = match name {
        Some(name) => format!("{table}{name}\n"),
        None => table,
    };
    let sep = if kept.trim().is_empty() || kept.ends_with("\n\n") { "" } else { "\n" };
    let new = format!("{}{sep}{table}", kept.trim_start_matches('\n'));
    if had {
        fs::write(config.with_extension("toml.bak"), &old)?;
        println!("replaced [remote] in {}; the old config is in config.toml.bak", config.display());
    }
    fs::create_dir_all(&dir)?;
    fs::write(&config, &new)?;
    if let Err(e) = Config::load() {
        fs::write(&config, &old)?;
        return Err(e.context("the new config doesn't load; left it as it was"));
    }
    if no_service {
        println!("`carrel sync` fetches the library");
        return Ok(());
    }
    let services = [install::Service::Sync].into_iter().chain(web.then_some(install::Service::Web));
    for service in services {
        match install::install(service, "127.0.0.1:7700") {
            Ok(_) => println!("{}", install::status(service)?),
            Err(e) => println!("cannot set up the background service: {e:#}"),
        }
    }
    println!("the library arrives with the first sync, within a minute; `carrel sync` fetches it now");
    Ok(())
}

/// A line from the terminal, not shown as it is typed.
fn read_secret(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    // SAFETY: termios is plain data, filled by tcgetattr before use; fd 0 is stdin, a terminal here.
    let mut was: libc::termios = unsafe { std::mem::zeroed() };
    let hidden = unsafe { libc::tcgetattr(0, &mut was) } == 0;
    if hidden {
        let mut quiet = was;
        quiet.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) };
    }
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if hidden {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &was) };
        println!();
    }
    read?;
    Ok(line.trim().to_string())
}

pub fn open(path: &Path) -> Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(opener).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}
