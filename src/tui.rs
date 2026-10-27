//! Terminal UI: browse and search the library, add papers (by PDF, DOI, arXiv id or title), attach PDFs,
//! review AI drafts.

use anyhow::Result;
use carrel::fetch::{self, Candidate, Source};
use carrel::{Field, Library, Paper};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Text},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(PartialEq)]
enum Mode {
    Browse,
    Search,
    Add,
    Results,
    Pick,
    Drafts,
    /// Choosing which project to show.
    Projects,
    /// Typing the project to put the selected paper in.
    Assign,
}

/// What the file picker picks for.
#[derive(Clone)]
enum Picking {
    NewPaper,
    /// The PDF of this paper.
    Pdf(String),
    /// A supplementary file of this paper.
    Supplement(String),
}

/// Background work: downloads, lookups and extraction take seconds.
enum Done {
    /// A paper was added or got its PDF: its id and what to tell the user.
    Paper(Result<(String, String)>),
    /// Papers found online, and what they are ("References of x").
    Found(Result<Vec<Candidate>>, String),
}

struct App {
    lib: Library,
    mode: Mode,
    papers: Vec<Paper>,
    list: ListState,
    query: String,
    input: String,
    drafts: Vec<(Paper, Field)>,
    draft_list: ListState,
    results: Vec<Candidate>,
    results_title: String,
    result_list: ListState,
    /// File picker: the folder shown, its files and subfolders, and what the pick is for.
    pick_dir: PathBuf,
    pick_entries: Vec<PathBuf>,
    pick_list: ListState,
    picking: Picking,
    /// Show only this project's papers.
    project: Option<String>,
    project_names: Vec<String>,
    project_list: ListState,
    scroll: u16,
    status: String,
    busy: Option<(&'static str, mpsc::Receiver<Done>)>,
}

pub fn run(lib: Library) -> Result<()> {
    let mut app = App {
        lib,
        mode: Mode::Browse,
        papers: vec![],
        list: ListState::default(),
        query: String::new(),
        input: String::new(),
        drafts: vec![],
        draft_list: ListState::default(),
        results: vec![],
        results_title: String::new(),
        result_list: ListState::default(),
        pick_dir: dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| ".".into()),
        pick_entries: vec![],
        pick_list: ListState::default(),
        picking: Picking::NewPaper,
        project: None,
        project_names: vec![],
        project_list: ListState::default(),
        scroll: 0,
        status: String::new(),
        busy: None,
    };
    app.reload();
    let mut term = ratatui::init();
    let result = app.run(&mut term);
    ratatui::restore();
    result
}

/// Leave the TUI while `f` runs (an editor, say), then come back.
fn suspend<T>(term: &mut DefaultTerminal, f: impl FnOnce() -> Result<T>) -> Result<T> {
    ratatui::restore();
    let result = f();
    *term = ratatui::init();
    result
}

fn clamp(state: &mut ListState, len: usize) {
    state.select(if len == 0 { None } else { Some(state.selected().unwrap_or(0).min(len - 1)) });
}

fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// "added x · note · note"
fn message(head: String, notes: Vec<String>) -> String {
    std::iter::once(head).chain(notes).collect::<Vec<_>>().join(" · ")
}

impl App {
    fn reload(&mut self) {
        let (all, broken) = self.lib.papers();
        if !broken.is_empty() {
            self.status = format!("broken: {}", broken.join("; "));
        }
        self.drafts = all.iter().flat_map(|p| p.drafts().into_iter().map(|f| (p.clone(), f))).collect();
        self.papers = all;
        let query = match &self.project {
            Some(name) => format!("project:{name} {}", self.query),
            None => self.query.clone(),
        };
        if !query.trim().is_empty() {
            match self.lib.search(&query, usize::MAX) {
                Ok(hits) => self.papers = hits.into_iter().map(|(p, _)| p).collect(),
                Err(e) => self.status = format!("search failed: {e:#}"),
            }
        }
        clamp(&mut self.list, self.papers.len());
        clamp(&mut self.draft_list, self.drafts.len());
    }

    fn selected(&self) -> Option<&Paper> {
        self.list.selected().and_then(|i| self.papers.get(i))
    }

    fn report(&mut self, result: Result<()>, success: String) {
        self.status = match result {
            Ok(()) => success,
            Err(e) => format!("{e:#}"),
        };
    }

    fn run(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        loop {
            term.draw(|f| self.draw(f))?;
            if let Some((_, rx)) = &self.busy
                && let Ok(done) = rx.try_recv()
            {
                self.busy = None;
                self.finished(done);
            }
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            let Event::Key(key) = event::read()? else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match self.mode {
                Mode::Browse if key.code == KeyCode::Char('q') => return Ok(()),
                Mode::Browse => self.on_browse(key.code, term),
                Mode::Search | Mode::Add | Mode::Assign => self.on_input(key.code),
                Mode::Projects => self.on_projects(key.code),
                Mode::Results => self.on_results(key.code),
                Mode::Pick => self.on_pick(key.code),
                Mode::Drafts => self.on_drafts(key.code, term),
            }
        }
    }

    fn spawn(&mut self, label: &'static str, job: impl FnOnce(Library) -> Done + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let lib = self.lib.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job(lib));
        });
        self.busy = Some((label, rx));
    }

    /// Add a paper in the background; it gets selected once it is in.
    fn add(&mut self, add: impl FnOnce(&Library) -> Result<(String, Vec<String>)> + Send + 'static) {
        self.spawn("adding a paper…", move |lib| {
            Done::Paper(add(&lib).map(|(id, notes)| {
                let text = message(format!("added {id}"), notes);
                (id, text)
            }))
        });
    }

    fn finished(&mut self, done: Done) {
        self.reload();
        match done {
            Done::Paper(Ok((id, text))) => {
                if !self.papers.iter().any(|p| p.id == id) {
                    self.query.clear(); // the new paper may not match the current search
                    self.reload();
                }
                self.list.select(self.papers.iter().position(|p| p.id == id));
                self.scroll = 0;
                self.status = text;
            }
            Done::Found(Ok(found), title) if !found.is_empty() => {
                self.results = found;
                self.results_title = title;
                self.result_list.select(Some(0));
                self.mode = Mode::Results;
            }
            Done::Found(Ok(_), title) => self.status = format!("{title}: nothing found"),
            Done::Paper(Err(e)) | Done::Found(Err(e), _) => self.status = format!("{e:#}"),
        }
    }

    fn on_browse(&mut self, code: KeyCode, term: &mut DefaultTerminal) {
        self.status.clear();
        let selected = self.selected().cloned();
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.select_next();
                self.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.select_previous();
                self.scroll = 0;
            }
            KeyCode::PageDown | KeyCode::Char('J') => self.scroll = self.scroll.saturating_add(5),
            KeyCode::PageUp | KeyCode::Char('K') => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Esc => {
                self.query.clear();
                self.reload();
            }
            KeyCode::Char('a' | 'i' | 'p' | 's' | 'u' | 'r' | 'R') if self.busy.is_some() => {
                self.status = "one moment, still busy".into();
            }
            KeyCode::Char('s') => {
                if let Some(p) = selected {
                    self.pick(Picking::Supplement(p.id));
                }
            }
            KeyCode::Char('u') => {
                if let Some(p) = selected {
                    self.spawn("updating the metadata…", move |lib| {
                        Done::Paper(lib.update(&p.id, None).map(|changes| {
                            let text = message(p.id.clone(), changes);
                            (p.id, text)
                        }))
                    });
                }
            }
            KeyCode::Char(key @ ('r' | 'R')) => {
                if let Some(p) = selected {
                    let cited_by = key == 'R';
                    let title = format!("{} {}", if cited_by { "Papers citing" } else { "References of" }, p.id);
                    self.spawn("looking up citations…", move |lib| Done::Found(lib.related(&p.id, cited_by), title));
                }
            }
            KeyCode::Char('a') => {
                self.input.clear();
                self.mode = Mode::Add;
            }
            KeyCode::Char('i') => self.pick(Picking::NewPaper),
            KeyCode::Char('p') => match selected {
                Some(p) if p.pdf().is_some() => self.status = format!("{} already has a PDF", p.id),
                Some(p) => self.pick(Picking::Pdf(p.id)),
                None => {}
            },
            KeyCode::Char('P') => {
                self.project_names = self.lib.projects();
                let current = self.project.as_ref().and_then(|p| self.project_names.iter().position(|n| n == p));
                self.project_list.select(Some(current.map_or(0, |i| i + 1)));
                self.mode = Mode::Projects;
            }
            KeyCode::Char('+') if selected.is_some() => {
                self.input.clear();
                self.mode = Mode::Assign;
            }
            KeyCode::Char('-') => match (&self.project, selected) {
                (Some(name), Some(p)) => {
                    let result = self.lib.remove_from_project(name, std::slice::from_ref(&p.id));
                    self.report(result, format!("took {} out of {name}", p.id));
                    self.reload();
                }
                _ => self.status = "- takes a paper out of the project shown (P picks one)".into(),
            },
            KeyCode::Char('d') if self.drafts.is_empty() => self.status = "no drafts to review".into(),
            KeyCode::Char('d') => {
                self.mode = Mode::Drafts;
                self.scroll = 0;
            }
            KeyCode::Char('o') => match selected.as_ref().and_then(Paper::pdf) {
                Some(pdf) => {
                    let result = crate::open(&pdf);
                    self.report(result, "opening the PDF".into());
                }
                None => self.status = "this paper has no PDF; p attaches one".into(),
            },
            KeyCode::Char('e') => {
                if let Some(p) = selected {
                    self.edit(term, &p.id, p.bib_path());
                }
            }
            KeyCode::Char('n') => {
                if let Some(p) = selected {
                    self.edit(term, &p.id, p.dir.join(Field::Notes.file()));
                }
            }
            _ => {}
        }
    }

    fn edit(&mut self, term: &mut DefaultTerminal, id: &str, path: PathBuf) {
        let result = suspend(term, || crate::edit(&path)).and_then(|()| self.lib.get(id).map(|_| ()));
        self.report(result, format!("saved {}", path.display()));
        self.reload();
    }

    fn on_input(&mut self, code: KeyCode) {
        let search = self.mode == Mode::Search;
        let buffer = if search { &mut self.query } else { &mut self.input };
        match code {
            KeyCode::Char(c) => buffer.push(c),
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Esc => {
                buffer.clear();
                self.mode = Mode::Browse;
            }
            KeyCode::Enter => {
                match self.mode {
                    Mode::Add => self.submit(),
                    Mode::Assign => self.assign(),
                    _ => {}
                }
                self.mode = Mode::Browse;
            }
            _ => return,
        }
        if search {
            self.list.select(Some(0));
            self.reload();
        }
    }

    fn assign(&mut self) {
        let name = self.input.trim().to_string();
        let Some(id) = self.selected().map(|p| p.id.clone()) else { return };
        if !name.is_empty() {
            let result = self.lib.add_to_project(&name, std::slice::from_ref(&id));
            self.report(result, format!("put {id} in {name}"));
        }
    }

    fn on_projects(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Browse,
            KeyCode::Down | KeyCode::Char('j') => self.project_list.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.project_list.select_previous(),
            KeyCode::Enter => {
                // The first entry is the whole library.
                let chosen = self.project_list.selected().unwrap_or(0);
                self.project = chosen.checked_sub(1).and_then(|i| self.project_names.get(i)).cloned();
                self.mode = Mode::Browse;
                self.list.select(Some(0));
                self.reload();
            }
            _ => {}
        }
    }

    /// Files and identifiers are added right away; anything else is looked up online first.
    fn submit(&mut self) {
        let source = self.input.trim().to_string();
        if source.is_empty() {
            return;
        }
        match fetch::classify(&source) {
            Ok(Source::Title(query)) => self.spawn("searching online…", move |lib| {
                let found = fetch::search(&query).map(|mut found| {
                    lib.mark_known(&mut found);
                    found
                });
                Done::Found(found, format!("Found online for \"{query}\""))
            }),
            Ok(_) => self.add(move |lib| lib.add(&source)),
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn on_results(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Browse,
            KeyCode::Down | KeyCode::Char('j') => self.result_list.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.result_list.select_previous(),
            KeyCode::Enter => match self.result_list.selected().and_then(|i| self.results.get(i)).cloned() {
                Some(c) if c.in_library.is_some() => {
                    self.status = format!("already in the library as {}", c.in_library.unwrap_or_default());
                }
                Some(c) => {
                    self.mode = Mode::Browse;
                    self.add(move |lib| lib.add_ids(&c.ids));
                }
                None => {}
            },
            _ => {}
        }
    }

    fn pick(&mut self, picking: Picking) {
        self.picking = picking;
        self.open_dir(self.pick_dir.clone());
        self.mode = Mode::Pick;
    }

    /// A folder's PDFs (any files, for supplementary material), newest first since a fresh download is
    /// usually the one wanted, then its subfolders.
    fn open_dir(&mut self, dir: PathBuf) {
        let entries = fs::read_dir(&dir).into_iter().flatten().flatten();
        let visible = entries.filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path());
        let any = matches!(self.picking, Picking::Supplement(_));
        let wanted = |p: &PathBuf| any || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        let (mut folders, mut files): (Vec<PathBuf>, Vec<PathBuf>) =
            visible.filter(|p| p.is_dir() || wanted(p)).partition(|p| p.is_dir());
        files.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).ok()));
        folders.sort();
        self.pick_entries = files.into_iter().chain(folders).collect();
        self.pick_list.select((!self.pick_entries.is_empty()).then_some(0));
        self.pick_dir = dir;
    }

    fn on_pick(&mut self, code: KeyCode) {
        let chosen = self.pick_list.selected().and_then(|i| self.pick_entries.get(i)).cloned();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Browse,
            KeyCode::Down | KeyCode::Char('j') => self.pick_list.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.pick_list.select_previous(),
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                if let Some(parent) = self.pick_dir.parent().map(PathBuf::from) {
                    self.open_dir(parent);
                }
            }
            KeyCode::Char('~') => {
                if let Some(home) = dirs::home_dir() {
                    self.open_dir(home);
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match chosen {
                Some(dir) if dir.is_dir() => self.open_dir(dir),
                Some(file) => {
                    self.mode = Mode::Browse;
                    match self.picking.clone() {
                        Picking::NewPaper => self.add(move |lib| lib.add(&file.to_string_lossy())),
                        Picking::Pdf(id) => self.spawn("attaching the PDF…", move |lib| {
                            Done::Paper(lib.attach(&id, Some(&file)).map(|notes| {
                                let text = message(format!("attached the PDF to {id}"), notes);
                                (id, text)
                            }))
                        }),
                        Picking::Supplement(id) => self.spawn("adding the file…", move |lib| {
                            let source = file.to_string_lossy().into_owned();
                            Done::Paper(lib.add_supplementary(&id, &[source]).map(|notes| {
                                let text = message(id.clone(), notes);
                                (id, text)
                            }))
                        }),
                    }
                }
                None => {}
            },
            _ => {}
        }
    }

    fn on_drafts(&mut self, code: KeyCode, term: &mut DefaultTerminal) {
        let current = self.draft_list.selected().and_then(|i| self.drafts.get(i)).map(|(p, f)| (p.clone(), *f));
        match (code, current) {
            (KeyCode::Esc | KeyCode::Char('q'), _) => self.mode = Mode::Browse,
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                self.draft_list.select_next();
                self.scroll = 0;
            }
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                self.draft_list.select_previous();
                self.scroll = 0;
            }
            (KeyCode::PageDown | KeyCode::Char('J'), _) => self.scroll = self.scroll.saturating_add(5),
            (KeyCode::PageUp | KeyCode::Char('K'), _) => self.scroll = self.scroll.saturating_sub(5),
            (KeyCode::Char('a'), Some((p, field))) => {
                let result = self.lib.approve(&p.id, field);
                self.report(result, format!("approved the {} for {}", field.name(), p.id));
            }
            (KeyCode::Char('r'), Some((p, field))) => {
                let result = self.lib.reject(&p.id, field);
                self.report(result, format!("rejected the {} for {}", field.name(), p.id));
            }
            (KeyCode::Char('e'), Some((p, field))) => {
                let path = p.dir.join("drafts").join(field.file());
                let result = suspend(term, || crate::edit(&path));
                self.report(result, "draft edited; approve it with a".into());
            }
            _ => return,
        }
        self.reload();
        if self.drafts.is_empty() {
            self.mode = Mode::Browse;
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let [top, body, bottom] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());

        let mut header = vec![" carrel ".bold().reversed(), format!("  {}", count(self.papers.len(), "paper")).into()];
        if let Some(name) = &self.project {
            header.push(format!(" in project {name}").bold());
        }
        if !self.query.is_empty() && self.mode != Mode::Search {
            header.push(format!(" matching \"{}\" (esc clears)", self.query).into());
        }
        if !self.drafts.is_empty() {
            header.push(format!(" · {} to review", count(self.drafts.len(), "draft")).yellow());
        }
        if let Some((label, _)) = &self.busy {
            header.push(format!(" · {label}").cyan());
        }
        f.render_widget(Line::from(header), top);

        let help = match self.mode {
            Mode::Browse => {
                " / search · a add · i import PDF · p attach PDF · o open · s add supplementary · u update metadata · e edit .bib · n notes · r/R refs/cited by · P projects · +/- project · d drafts · q quit"
            }
            Mode::Projects => " enter show · j/k move · esc back",
            Mode::Results => " enter add · j/k move · esc back",
            Mode::Pick => " enter open/choose · backspace up · ~ home · esc cancel",
            Mode::Drafts => " a approve · r reject · e edit · j/k move · J/K scroll · esc back",
            Mode::Search | Mode::Add | Mode::Assign => "",
        };
        let footer: Line = match self.mode {
            Mode::Search => format!(" search: {}▏", self.query).into(),
            Mode::Add => format!(" add a PDF path, DOI, arXiv id or title: {}▏", self.input).into(),
            Mode::Assign => {
                let known = self.lib.projects().join(", ");
                format!(" put it in project ({known}): {}▏", self.input).into()
            }
            _ if !self.status.is_empty() => format!(" {}", self.status).into(),
            _ => help.dim().into(),
        };
        f.render_widget(footer, bottom);

        match self.mode {
            Mode::Drafts => self.draw_drafts(f, body),
            Mode::Results => self.draw_results(f, body),
            Mode::Pick => self.draw_pick(f, body),
            Mode::Projects => {
                let all = std::iter::once("All papers".to_string());
                let names = self.project_names.iter().map(|name| {
                    let n = self.lib.project(name).map_or(0, |ids| ids.len());
                    format!("{name} ({})", count(n, "paper"))
                });
                let list = List::new(all.chain(names).map(ListItem::new))
                    .block(Block::bordered().title(" Projects "))
                    .highlight_style(Style::new().reversed());
                f.render_stateful_widget(list, body, &mut self.project_list);
            }
            _ => self.draw_papers(f, body),
        }
    }

    fn draw_papers(&mut self, f: &mut Frame, area: Rect) {
        let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
        let items: Vec<ListItem> = self
            .papers
            .iter()
            .map(|p| {
                let has_draft = self.drafts.iter().any(|(d, _)| d.id == p.id);
                let year = p.bib.year().map(|y| y.to_string()).unwrap_or_else(|| "    ".into());
                ListItem::new(Line::from(vec![
                    if has_draft { "✎ ".yellow() } else { "  ".into() },
                    format!("{year} ").dim(),
                    format!("{:<16.16} ", p.short_authors()).cyan(),
                    p.bib.title().into(),
                ]))
            })
            .collect();
        let list = List::new(items).block(Block::bordered().title(" Papers ")).highlight_style(Style::new().reversed());
        f.render_stateful_widget(list, left, &mut self.list);

        let detail = match self.selected() {
            Some(p) => details(p, &self.lib.projects_of(&p.id)),
            None => {
                Text::from(" No papers yet. Press a to add one by PDF, DOI, arXiv id or title, or i to import a PDF.")
                    .dim()
            }
        };
        let detail = Paragraph::new(detail).wrap(Wrap { trim: false }).scroll((self.scroll, 0));
        f.render_widget(detail.block(Block::bordered()), right);
    }

    fn draw_results(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .results
            .iter()
            .map(|c| match c.in_library {
                Some(_) => ListItem::new(format!("✓ {c}")).dim(),
                None => ListItem::new(format!("  {c}")),
            })
            .collect();
        let block = Block::bordered().title(format!(" {} · ✓ in the library ", self.results_title));
        let list = List::new(items).block(block).highlight_style(Style::new().reversed());
        f.render_stateful_widget(list, area, &mut self.result_list);
    }

    fn draw_pick(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .pick_entries
            .iter()
            .map(|p| {
                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                if p.is_dir() { ListItem::new(format!("{name}/").blue()) } else { ListItem::new(name) }
            })
            .collect();
        let purpose = match &self.picking {
            Picking::NewPaper => "a PDF to add".into(),
            Picking::Pdf(id) => format!("the PDF for {id}"),
            Picking::Supplement(id) => format!("a supplementary file for {id}"),
        };
        let block = Block::bordered().title(format!(" Pick {purpose} · {} ", self.pick_dir.display()));
        let list = List::new(items).block(block).highlight_style(Style::new().reversed());
        f.render_stateful_widget(list, area, &mut self.pick_list);
    }

    fn draw_drafts(&mut self, f: &mut Frame, area: Rect) {
        let [left, right] = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(area);
        let items: Vec<ListItem> =
            self.drafts.iter().map(|(p, field)| ListItem::new(format!("{} · {}", p.id, field.name()))).collect();
        let list = List::new(items).block(Block::bordered().title(" Drafts ")).highlight_style(Style::new().reversed());
        f.render_stateful_widget(list, left, &mut self.draft_list);

        let Some((p, field)) = self.draft_list.selected().and_then(|i| self.drafts.get(i)) else { return };
        let mut lines = vec![Line::from(p.bib.title().bold()), Line::default()];
        lines.push(format!("Proposed {}", field.name()).yellow().bold().into());
        lines.extend(text_lines(&p.draft(*field).unwrap_or_default()));
        lines.push(Line::default());
        let current = p.get(*field).unwrap_or_default();
        let heading = match field {
            Field::Notes => "Current notes (the draft is appended)",
            Field::References => "Current references (the draft adds those not in it)",
            _ => "Current version (the draft replaces it)",
        };
        lines.push(heading.bold().into());
        if current.trim().is_empty() {
            lines.push("(none)".dim().into());
        }
        lines.extend(text_lines(&current));
        let view = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll, 0));
        f.render_widget(view.block(Block::bordered()), right);
    }
}

fn text_lines(text: &str) -> Vec<Line<'static>> {
    text.lines().map(|l| Line::from(l.to_string())).collect()
}

fn details(p: &Paper, projects: &[String]) -> Text<'static> {
    let b = &p.bib;
    let all = b.authors();
    let mut authors = all.iter().take(10).cloned().collect::<Vec<_>>().join("; ");
    if all.len() > 10 {
        authors += &format!(" and {} more", all.len() - 10);
    }
    let mut lines = vec![Line::from(b.title().bold()), Line::from(authors.cyan())];
    let facts = [
        b.year().map(|y| y.to_string()),
        b.venue(),
        b.doi().map(|d| format!("doi:{d}")),
        b.arxiv().map(|a| format!("arXiv:{a}")),
    ];
    lines.push(facts.into_iter().flatten().collect::<Vec<_>>().join(" · ").dim().into());
    let pdf = if p.pdf().is_some() { "PDF" } else { "no PDF (p attaches one)" };
    lines.push(format!("{} · {pdf}", p.id).dim().into());
    let keywords = b.keywords();
    if !keywords.is_empty() {
        lines.push(Line::from(vec!["keywords: ".dim(), keywords.join(", ").into()]));
    }
    if !projects.is_empty() {
        lines.push(Line::from(vec!["projects: ".dim(), projects.join(", ").into()]));
    }
    let supplementary = p.supplementary();
    if !supplementary.is_empty() {
        lines.push(Line::from(vec!["supplementary: ".dim(), supplementary.join(", ").into()]));
    }
    for field in p.drafts() {
        lines.push(format!("✎ {} draft waiting for review (press d)", field.name()).yellow().into());
    }
    for (heading, text) in
        [("Summary", p.get(Field::Summary)), ("Abstract", b.get("abstract")), ("Notes", p.get(Field::Notes))]
    {
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            lines.push(Line::default());
            lines.push(heading.bold().into());
            lines.extend(text_lines(&text));
        }
    }
    if p.get(Field::Summary).is_none() {
        lines.push(Line::default());
        lines.push("No summary yet. Ask your AI to read the paper and propose one.".dim().into());
    }
    Text::from(lines)
}
