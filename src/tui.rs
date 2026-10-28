//! Terminal UI, styled like Tapedeck: a header with the logo, the tabs and the library's size; three panes
//! that narrow from left to right; a Selected bar; the keys. Tabs: Library (shelves › papers › the paper),
//! Graph (what it cites, what cites it, what the library lacks), People (people › papers › maybe the same
//! person) and Drafts (AI proposals next to what they would change).

use crate::theme::Theme;
use anyhow::Result;
use carrel::authors::{self, Person, Proposal};
use carrel::fetch::{self, Candidate, Source};
use carrel::graph::{Graph, Suggestion};
use carrel::{Field, Library, Paper};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const TABS: [&str; 4] = ["Library", "Graph", "People", "Drafts"];
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Library,
    Graph,
    People,
    Drafts,
}

#[derive(PartialEq)]
enum Mode {
    Normal,
    /// Typing the filter of the papers (Library) or people (People).
    Filter,
    /// Typing what to add: a PDF path, DOI, arXiv id or title.
    Add,
    /// Typing the project to put the selected paper in.
    Assign,
    Confirm,
    Info,
    Results,
    Pick,
}

/// Something that needs a `y` first.
enum Ask {
    Delete(String),
    OutOfProject(String, String),
    Reject(Draft),
    People { same: bool, spellings: Vec<String> },
}

/// What the file picker picks for.
#[derive(Clone)]
enum Picking {
    NewPaper,
    Pdf(String),
    Supplement(String),
}

/// Background work: downloads, lookups and extraction take seconds.
enum Done {
    /// A paper was added or changed: its id and what to tell the user.
    Paper(Result<(String, String)>),
    /// Papers found online, and what they are ("References of x").
    Found(Result<Vec<Candidate>>, String),
}

/// A left-hand shelf of the Library: everything, a project or a keyword.
#[derive(Clone, PartialEq)]
enum Shelf {
    All,
    Project(String),
    Keyword(String),
}

#[derive(Clone, Copy)]
enum Status {
    Ready,
    NoText,
    NoPdf,
    Draft,
}

#[derive(Clone)]
enum Draft {
    Paper(Paper, Field),
    People(Proposal),
}

/// What the Graph tab shows for one paper.
struct GraphView {
    id: String,
    cites: Vec<(usize, String, bool)>,
    cited_by: Vec<(usize, String, bool)>,
    missing: Vec<Suggestion>,
}

struct App {
    lib: Library,
    theme: Theme,
    tab: Tab,
    mode: Mode,
    ask: Option<(String, Ask)>,
    info: Vec<(&'static str, String)>,
    /// Which pane of the tab has the focus, left to right.
    focus: usize,

    all: Vec<Paper>,
    titles: HashMap<String, String>,
    shelves: Vec<(Shelf, usize)>,
    shelf_list: ListState,
    papers: Vec<(Paper, Status)>,
    paper_table: TableState,
    query: String,
    scroll: u16,

    /// The whole graph, built once after each change to the library; `graph` is the part for one paper.
    full: Option<Graph>,
    graph: Option<GraphView>,
    people: Vec<Person>,
    people_query: String,
    people_loaded: bool,
    graph_lists: [ListState; 3],
    people_list: ListState,
    person_papers: ListState,
    maybe_list: ListState,

    drafts: Vec<Draft>,
    draft_list: ListState,

    input: String,
    results: Vec<Candidate>,
    results_title: String,
    result_list: ListState,
    pick_dir: PathBuf,
    pick_entries: Vec<PathBuf>,
    pick_list: ListState,
    picking: Picking,

    status: String,
    busy: Option<(&'static str, mpsc::Receiver<Done>)>,
    ticks: usize,
}

pub fn run(lib: Library) -> Result<()> {
    let theme = Theme::configured()?;
    let mut app = App {
        lib,
        theme,
        tab: Tab::Library,
        mode: Mode::Normal,
        ask: None,
        info: vec![],
        focus: 1,
        all: vec![],
        titles: HashMap::new(),
        shelves: vec![],
        shelf_list: ListState::default().with_selected(Some(0)),
        papers: vec![],
        paper_table: TableState::default().with_selected(Some(0)),
        query: String::new(),
        scroll: 0,
        full: None,
        graph: None,
        people: vec![],
        people_query: String::new(),
        people_loaded: false,
        graph_lists: Default::default(),
        people_list: ListState::default().with_selected(Some(0)),
        person_papers: ListState::default().with_selected(Some(0)),
        maybe_list: ListState::default().with_selected(Some(0)),
        drafts: vec![],
        draft_list: ListState::default().with_selected(Some(0)),
        input: String::new(),
        results: vec![],
        results_title: String::new(),
        result_list: ListState::default(),
        pick_dir: dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| ".".into()),
        pick_entries: vec![],
        pick_list: ListState::default(),
        picking: Picking::NewPaper,
        status: String::new(),
        busy: None,
        ticks: 0,
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

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// "added x · note · note"
fn message(head: String, notes: Vec<String>) -> String {
    std::iter::once(head).chain(notes).collect::<Vec<_>>().join(" · ")
}

/// Move a list selection by `by` rows, staying inside it.
fn step(state: &mut ListState, len: usize, by: isize) {
    if len > 0 {
        let at = state.selected().unwrap_or(0) as isize + by;
        state.select(Some(at.clamp(0, len as isize - 1) as usize));
    }
}

fn status_of(p: &Paper) -> Status {
    if !p.drafts().is_empty() {
        Status::Draft
    } else if p.pdf().is_none() {
        Status::NoPdf
    } else if !p.dir.join("fulltext.md").exists() {
        Status::NoText
    } else {
        Status::Ready
    }
}

impl App {
    /// Read the library again and narrow it to the shelf and the filter, keeping the selected paper.
    fn reload(&mut self) {
        let keep = self.selected().map(|p| p.id.clone());
        let (all, broken) = self.lib.papers();
        if !broken.is_empty() {
            self.status = format!("broken: {}", broken.join("; "));
        }
        self.titles = all.iter().map(|p| (p.id.clone(), p.bib.title())).collect();

        let mut shelves = vec![(Shelf::All, all.len())];
        for name in self.lib.projects() {
            let n = self.lib.project(&name).map_or(0, |ids| ids.len());
            shelves.push((Shelf::Project(name), n));
        }
        let mut keywords: Vec<(String, usize)> = vec![];
        for k in all.iter().flat_map(|p| p.bib.keywords()) {
            match keywords.iter_mut().find(|(seen, _)| seen.eq_ignore_ascii_case(&k)) {
                Some((_, n)) => *n += 1,
                None => keywords.push((k, 1)),
            }
        }
        // Keywords on one paper only would bury the tags you use.
        keywords.retain(|(_, n)| *n > 1);
        keywords.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        shelves.extend(keywords.into_iter().map(|(k, n)| (Shelf::Keyword(k), n)));
        let shelf = self.shelf_list.selected().and_then(|i| self.shelves.get(i)).map(|(s, _)| s.clone());
        self.shelves = shelves;
        self.shelf_list.select(Some(shelf.and_then(|s| self.shelves.iter().position(|(x, _)| *x == s)).unwrap_or(0)));

        let mut papers: Vec<Paper> = match self.shelf() {
            Shelf::All => all.clone(),
            Shelf::Project(name) => {
                let ids = self.lib.project(&name).unwrap_or_default();
                ids.iter().filter_map(|id| all.iter().find(|p| &p.id == id).cloned()).collect()
            }
            Shelf::Keyword(k) => {
                all.iter().filter(|p| p.bib.keywords().iter().any(|x| x.eq_ignore_ascii_case(&k))).cloned().collect()
            }
        };
        if !self.query.trim().is_empty() {
            match self.lib.search(&self.query, usize::MAX) {
                Ok(hits) => {
                    let shown: Vec<String> = papers.iter().map(|p| p.id.clone()).collect();
                    papers = hits.into_iter().map(|(p, _)| p).filter(|p| shown.contains(&p.id)).collect();
                }
                Err(e) => self.status = format!("filter: {e:#}"),
            }
        }
        self.papers = papers.into_iter().map(|p| (status_of(&p), p)).map(|(s, p)| (p, s)).collect();
        self.all = all;
        let at = keep.and_then(|id| self.papers.iter().position(|(p, _)| p.id == id));
        let len = self.papers.len();
        self.paper_table.select(if len == 0 {
            None
        } else {
            Some(at.unwrap_or(self.paper_table.selected().unwrap_or(0)).min(len - 1))
        });

        let mut drafts: Vec<Draft> =
            self.all.iter().flat_map(|p| p.drafts().into_iter().map(|f| Draft::Paper(p.clone(), f))).collect();
        drafts.extend(self.lib.people_drafts().into_iter().map(Draft::People));
        self.drafts = drafts;
        clamp(&mut self.draft_list, self.drafts.len());
        self.full = None;
        self.graph = None;
        self.people_loaded = false;
    }

    fn shelf(&self) -> Shelf {
        self.shelf_list.selected().and_then(|i| self.shelves.get(i)).map_or(Shelf::All, |(s, _)| s.clone())
    }

    fn selected(&self) -> Option<&Paper> {
        self.paper_table.selected().and_then(|i| self.papers.get(i)).map(|(p, _)| p)
    }

    /// Show this paper in the Library, widening the shelf and the filter when it's hidden.
    fn select_paper(&mut self, id: &str) {
        if !self.papers.iter().any(|(p, _)| p.id == id) {
            self.query.clear();
            self.shelf_list.select(Some(0));
            self.reload();
        }
        self.paper_table.select(self.papers.iter().position(|(p, _)| p.id == id));
        self.scroll = 0;
    }

    /// The citation graph and the people: built when a tab needs them, once after every change.
    fn load_graph(&mut self) {
        if !matches!(self.tab, Tab::Graph | Tab::People) {
            return;
        }
        if self.full.is_none() {
            match self.lib.graph() {
                Ok(graph) => self.full = Some(graph),
                Err(e) => {
                    self.status = format!("graph: {e:#}");
                    return;
                }
            }
        }
        let Some(graph) = &self.full else { return };
        let id = self.selected().map(|p| p.id.clone());
        if let Some(id) = id.filter(|id| self.graph.as_ref().is_none_or(|g| &g.id != id)) {
            let within = match self.shelf() {
                Shelf::Project(name) => self.lib.project(&name).ok(),
                _ => None,
            };
            let mut missing = graph.suggestions(within.as_deref());
            missing.truncate(100);
            self.graph = Some(GraphView {
                cites: graph.tree(&id, 3, false).unwrap_or_default(),
                cited_by: graph.tree(&id, 3, true).unwrap_or_default(),
                missing,
                id,
            });
            let lens = self.graph_lens();
            for (list, len) in self.graph_lists.iter_mut().zip(lens) {
                clamp(list, len);
            }
        }
        if !self.people_loaded {
            let query = Some(self.people_query.trim()).filter(|q| !q.is_empty());
            self.people = graph.authors.people(query).into_iter().cloned().collect();
            clamp(&mut self.people_list, self.people.len());
            self.people_loaded = true;
        }
    }

    fn graph_lens(&self) -> [usize; 3] {
        self.graph.as_ref().map_or([0; 3], |g| [g.cites.len(), g.cited_by.len(), g.missing.len()])
    }

    fn person(&self) -> Option<&Person> {
        self.people_list.selected().and_then(|i| self.people.get(i))
    }

    fn draft(&self) -> Option<&Draft> {
        self.draft_list.selected().and_then(|i| self.drafts.get(i))
    }

    fn report(&mut self, result: Result<()>, success: String) {
        self.status = match result {
            Ok(()) => success,
            Err(e) => format!("{e:#}"),
        };
    }

    fn run(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        loop {
            self.load_graph();
            term.draw(|f| self.draw(f))?;
            if let Some((_, rx)) = &self.busy
                && let Ok(done) = rx.try_recv()
            {
                self.busy = None;
                self.finished(done);
            }
            self.ticks = self.ticks.wrapping_add(1);
            if !event::poll(Duration::from_millis(120))? {
                continue;
            }
            let Event::Key(key) = event::read()? else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match self.mode {
                Mode::Normal if key.code == KeyCode::Char('q') => return Ok(()),
                Mode::Normal => self.on_key(key, term),
                Mode::Filter | Mode::Add | Mode::Assign => self.on_input(key.code),
                Mode::Confirm => self.on_confirm(key.code),
                Mode::Info => self.mode = Mode::Normal,
                Mode::Results => self.on_results(key.code),
                Mode::Pick => self.on_pick(key.code),
            }
        }
    }

    fn spawn(&mut self, label: &'static str, job: impl FnOnce(Library) -> Done + Send + 'static) {
        if self.busy.is_some() {
            self.status = "one moment, still busy".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let lib = self.lib.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job(lib));
        });
        self.busy = Some((label, rx));
    }

    /// Add a paper in the background; it gets selected once it is in.
    fn add(&mut self, add: impl FnOnce(&Library) -> Result<(String, Vec<String>)> + Send + 'static) {
        self.spawn("adding a paper", move |lib| {
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
                self.select_paper(&id);
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

    fn panes(&self) -> usize {
        match self.tab {
            Tab::Drafts => 1,
            _ => 3,
        }
    }

    fn switch(&mut self, tab: Tab) {
        self.tab = tab;
        self.focus = if tab == Tab::Library { 1 } else { 0 };
        self.scroll = 0;
    }

    fn on_key(&mut self, key: KeyEvent, term: &mut DefaultTerminal) {
        self.status.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let tabs = [Tab::Library, Tab::Graph, Tab::People, Tab::Drafts];
        let now = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        match key.code {
            KeyCode::Tab | KeyCode::Char(']') => return self.switch(tabs[(now + 1) % tabs.len()]),
            KeyCode::BackTab | KeyCode::Char('[') => return self.switch(tabs[(now + tabs.len() - 1) % tabs.len()]),
            KeyCode::Char(c @ '1'..='4') => return self.switch(tabs[c as usize - '1' as usize]),
            KeyCode::Left | KeyCode::Char('h') => return self.focus = self.focus.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => return self.focus = (self.focus + 1).min(self.panes() - 1),
            KeyCode::Down | KeyCode::Char('j') => return self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => return self.move_by(-1),
            KeyCode::Char('d') if ctrl => return self.move_by(10),
            KeyCode::Char('u') if ctrl => return self.move_by(-10),
            KeyCode::Char('g') => return self.move_by(-100_000),
            KeyCode::Char('G') => return self.move_by(100_000),
            KeyCode::PageDown | KeyCode::Char('J') => return self.scroll = self.scroll.saturating_add(5),
            KeyCode::PageUp | KeyCode::Char('K') => return self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Char('/') if matches!(self.tab, Tab::Library | Tab::People) => return self.mode = Mode::Filter,
            KeyCode::Esc => {
                match self.tab {
                    Tab::People => self.people_query.clear(),
                    _ => self.query.clear(),
                }
                return self.reload();
            }
            _ => {}
        }
        match self.tab {
            Tab::Library => self.on_library(key.code, term),
            Tab::Graph => self.on_graph(key.code),
            Tab::People => self.on_people(key.code),
            Tab::Drafts => self.on_drafts(key.code, term),
        }
    }

    fn move_by(&mut self, by: isize) {
        let lens = self.graph_lens();
        let person_papers = self.person().map_or(0, |p| p.papers.len());
        let maybe = self.person().map_or(0, |p| p.maybe.len());
        match (self.tab, self.focus) {
            (Tab::Library, 0) => {
                step(&mut self.shelf_list, self.shelves.len(), by);
                self.paper_table.select(Some(0));
                self.reload();
            }
            (Tab::Library, _) => {
                let len = self.papers.len();
                if len > 0 {
                    let at = self.paper_table.selected().unwrap_or(0) as isize + by;
                    self.paper_table.select(Some(at.clamp(0, len as isize - 1) as usize));
                }
            }
            (Tab::Graph, i) => step(&mut self.graph_lists[i.min(2)], lens[i.min(2)], by),
            (Tab::People, 0) => {
                step(&mut self.people_list, self.people.len(), by);
                self.person_papers.select(Some(0));
                self.maybe_list.select(Some(0));
            }
            (Tab::People, 1) => step(&mut self.person_papers, person_papers, by),
            (Tab::People, _) => step(&mut self.maybe_list, maybe, by),
            (Tab::Drafts, _) => step(&mut self.draft_list, self.drafts.len(), by),
        }
        self.scroll = 0;
    }

    fn on_library(&mut self, code: KeyCode, term: &mut DefaultTerminal) {
        let selected = self.selected().cloned();
        match code {
            KeyCode::Char('a') => {
                self.input.clear();
                self.mode = Mode::Add;
            }
            KeyCode::Char('A') => self.pick(Picking::NewPaper),
            KeyCode::Char('i') => {
                if let Some(p) = &selected {
                    self.info = info(p, &self.lib.projects_of(&p.id));
                    self.mode = Mode::Info;
                }
            }
            KeyCode::Char('s') => {
                if let Some(p) = selected {
                    self.pick(Picking::Supplement(p.id));
                }
            }
            KeyCode::Char('p') => match selected {
                Some(p) if p.pdf().is_some() => self.status = format!("{} already has a PDF", p.id),
                Some(p) => self.pick(Picking::Pdf(p.id)),
                None => {}
            },
            KeyCode::Char('u') => {
                if let Some(p) = selected {
                    self.spawn("updating the metadata", move |lib| {
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
                    self.spawn("looking up citations", move |lib| Done::Found(lib.related(&p.id, cited_by), title));
                }
            }
            KeyCode::Char('+') if selected.is_some() => {
                self.input.clear();
                self.mode = Mode::Assign;
            }
            KeyCode::Char('-') => match (self.shelf(), selected) {
                (Shelf::Project(name), Some(p)) => {
                    self.confirm(format!("Take {} out of {name}?", p.id), Ask::OutOfProject(name, p.id));
                }
                _ => self.status = "- takes a paper out of the project shown; pick a project on the left".into(),
            },
            KeyCode::Char('D') => {
                if let Some(p) = selected {
                    self.confirm(format!("Delete {} and all its files?", p.id), Ask::Delete(p.id));
                }
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

    fn on_graph(&mut self, code: KeyCode) {
        let Some(g) = &self.graph else { return };
        let node = |list: &ListState, nodes: &[(usize, String, bool)]| {
            list.selected().and_then(|i| nodes.get(i)).map(|n| n.1.clone())
        };
        match (code, self.focus) {
            (KeyCode::Enter, 0 | 1) => {
                let nodes = if self.focus == 0 { &g.cites } else { &g.cited_by };
                if let Some(id) = node(&self.graph_lists[self.focus], nodes) {
                    self.select_paper(&id);
                    self.graph_lists = Default::default();
                    self.status = format!("now showing {id}");
                }
            }
            (KeyCode::Char('i'), 0 | 1) => {
                let nodes = if self.focus == 0 { &g.cites } else { &g.cited_by };
                if let Some(p) = node(&self.graph_lists[self.focus], nodes).and_then(|id| self.lib.get(&id).ok()) {
                    self.info = info(&p, &self.lib.projects_of(&p.id));
                    self.mode = Mode::Info;
                }
            }
            (KeyCode::Char('a') | KeyCode::Enter, 2) => {
                let Some(s) = self.graph_lists[2].selected().and_then(|i| g.missing.get(i)).cloned() else { return };
                let ids = s.reference.candidate(None).ids;
                if ids.is_empty() {
                    self.input = s.reference.title.clone();
                    self.mode = Mode::Add;
                    self.status = "no DOI or arXiv id: look it up by its title".into();
                } else {
                    self.add(move |lib| lib.add_ids(&ids));
                }
            }
            (KeyCode::Char('i'), 2) => {
                if let Some(s) = self.graph_lists[2].selected().and_then(|i| g.missing.get(i)) {
                    let r = &s.reference;
                    self.info = vec![
                        ("title", r.title.clone()),
                        ("authors", r.authors.join("; ")),
                        ("year", r.year.map(|y| y.to_string()).unwrap_or_default()),
                        ("venue", r.venue.clone()),
                        ("doi", r.doi.clone().unwrap_or_default()),
                        ("arXiv", r.arxiv.clone().unwrap_or_default()),
                        ("source", r.source.clone()),
                        ("cited by", s.cited_by.join(", ")),
                    ];
                    self.info.retain(|(_, v)| !v.is_empty());
                    self.mode = Mode::Info;
                }
            }
            _ => {}
        }
    }

    fn on_people(&mut self, code: KeyCode) {
        let Some(person) = self.person().cloned() else { return };
        match (code, self.focus) {
            (KeyCode::Enter, 1) => {
                if let Some(id) = self.person_papers.selected().and_then(|i| person.papers.get(i)).cloned() {
                    self.select_paper(&id);
                    self.switch(Tab::Library);
                }
            }
            (KeyCode::Char(key @ ('m' | 'x')), _) => {
                let Some(maybe) = self.maybe_list.selected().and_then(|i| person.maybe.get(i)) else {
                    self.status = "nobody else's name fits theirs".into();
                    return;
                };
                let same = key == 'm';
                let spellings: Vec<String> = person.variants.iter().chain(&maybe.spellings).cloned().collect();
                let question = match same {
                    true => format!("Confirm that {} and {} are one person?", person.name, maybe),
                    false => format!("Confirm that {} and {} are different people?", person.name, maybe),
                };
                // Different people: one spelling each is enough to set them apart.
                let spellings =
                    if same { spellings } else { vec![person.variants[0].clone(), maybe.spellings[0].clone()] };
                self.confirm(question, Ask::People { same, spellings });
            }
            _ => {}
        }
    }

    fn on_drafts(&mut self, code: KeyCode, term: &mut DefaultTerminal) {
        let Some(draft) = self.draft().cloned() else { return };
        match code {
            KeyCode::Char('a') => {
                let result = match &draft {
                    Draft::Paper(p, field) => self.lib.approve(&p.id, *field),
                    Draft::People(d) => self.lib.decide_people(&d.id, true),
                };
                self.report(result, format!("approved {}", describe(&draft)));
                self.reload();
            }
            KeyCode::Char('r') => self.confirm(format!("Reject {}?", describe(&draft)), Ask::Reject(draft)),
            KeyCode::Char('e') => {
                let path = match &draft {
                    Draft::Paper(p, field) => p.dir.join("drafts").join(field.file()),
                    Draft::People(d) => self.lib.root.join(authors::DIR).join("drafts").join(format!("{}.json", d.id)),
                };
                let result = suspend(term, || crate::edit(&path));
                self.report(result, "draft edited; a approves it".into());
                self.reload();
            }
            _ => {}
        }
    }

    fn confirm(&mut self, question: String, ask: Ask) {
        self.ask = Some((question, ask));
        self.mode = Mode::Confirm;
    }

    fn on_confirm(&mut self, code: KeyCode) {
        self.mode = Mode::Normal;
        let Some((_, ask)) = self.ask.take() else { return };
        if code != KeyCode::Char('y') {
            self.status = "nothing changed".into();
            return;
        }
        let (result, done) = match ask {
            Ask::Delete(id) => (self.lib.remove(&id), format!("deleted {id}")),
            Ask::OutOfProject(name, id) => {
                (self.lib.remove_from_project(&name, std::slice::from_ref(&id)), format!("took {id} out of {name}"))
            }
            Ask::Reject(draft) => {
                let result = match &draft {
                    Draft::Paper(p, field) => self.lib.reject(&p.id, *field),
                    Draft::People(d) => self.lib.decide_people(&d.id, false),
                };
                (result, format!("rejected {}", describe(&draft)))
            }
            Ask::People { same: true, spellings } => {
                (self.lib.same_person(&spellings).map(|_| ()), "confirmed: one person".into())
            }
            Ask::People { same: false, spellings } => {
                (self.lib.different_people(&spellings).map(|_| ()), "confirmed: different people".into())
            }
        };
        self.report(result, done);
        self.reload();
    }

    fn edit(&mut self, term: &mut DefaultTerminal, id: &str, path: PathBuf) {
        let result = suspend(term, || crate::edit(&path)).and_then(|()| self.lib.get(id).map(|_| ()));
        self.report(result, format!("saved {}", path.display()));
        self.reload();
    }

    fn on_input(&mut self, code: KeyCode) {
        let filtering = self.mode == Mode::Filter;
        let buffer = match (filtering, self.tab) {
            (true, Tab::People) => &mut self.people_query,
            (true, _) => &mut self.query,
            (false, _) => &mut self.input,
        };
        match code {
            KeyCode::Char(c) => buffer.push(c),
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Esc => {
                buffer.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                match self.mode {
                    Mode::Add => self.submit(),
                    Mode::Assign => self.assign(),
                    _ => {}
                }
                if self.mode != Mode::Results {
                    self.mode = Mode::Normal;
                }
            }
            _ => return,
        }
        if filtering {
            match self.tab {
                Tab::People => {
                    self.people_loaded = false;
                    self.people_list.select(Some(0));
                }
                _ => {
                    self.paper_table.select(Some(0));
                    self.reload();
                }
            }
        }
    }

    fn assign(&mut self) {
        let name = self.input.trim().to_string();
        let Some(id) = self.selected().map(|p| p.id.clone()) else { return };
        if !name.is_empty() {
            let result = self.lib.add_to_project(&name, std::slice::from_ref(&id));
            self.report(result, format!("put {id} in {name}"));
            self.reload();
        }
    }

    /// Files and identifiers are added right away; anything else is looked up online first.
    fn submit(&mut self) {
        let source = self.input.trim().to_string();
        if source.is_empty() {
            return;
        }
        match fetch::classify(&source) {
            Ok(Source::Title(query)) => self.spawn("searching online", move |lib| {
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
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => step(&mut self.result_list, self.results.len(), 1),
            KeyCode::Up | KeyCode::Char('k') => step(&mut self.result_list, self.results.len(), -1),
            KeyCode::Enter | KeyCode::Char('a') => {
                match self.result_list.selected().and_then(|i| self.results.get(i)).cloned() {
                    Some(Candidate { in_library: Some(id), .. }) => {
                        self.status = format!("already in the library as {id}")
                    }
                    Some(c) => {
                        self.mode = Mode::Normal;
                        self.add(move |lib| lib.add_ids(&c.ids));
                    }
                    None => {}
                }
            }
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
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => step(&mut self.pick_list, self.pick_entries.len(), 1),
            KeyCode::Up | KeyCode::Char('k') => step(&mut self.pick_list, self.pick_entries.len(), -1),
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
                    self.mode = Mode::Normal;
                    match self.picking.clone() {
                        Picking::NewPaper => self.add(move |lib| lib.add(&file.to_string_lossy())),
                        Picking::Pdf(id) => self.spawn("attaching the PDF", move |lib| {
                            Done::Paper(lib.attach(&id, Some(&file)).map(|notes| {
                                let text = message(format!("attached the PDF to {id}"), notes);
                                (id, text)
                            }))
                        }),
                        Picking::Supplement(id) => self.spawn("adding the file", move |lib| {
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

    // ---- drawing ----

    fn draw(&mut self, f: &mut Frame) {
        let th = self.theme.clone();
        f.render_widget(Block::new().style(Style::new().fg(th.text)), f.area());
        let [header, body, selected, help] =
            Layout::vertical([Constraint::Length(4), Constraint::Min(5), Constraint::Length(3), Constraint::Length(1)])
                .areas(f.area());
        self.draw_header(f, header);
        match self.tab {
            Tab::Library => self.draw_library(f, body),
            Tab::Graph => self.draw_graph(f, body),
            Tab::People => self.draw_people(f, body),
            Tab::Drafts => self.draw_drafts(f, body),
        }
        self.draw_selected(f, selected);
        f.render_widget(Paragraph::new(self.hints()), help);
        match self.mode {
            Mode::Info => self.draw_info(f),
            Mode::Confirm => self.draw_confirm(f),
            Mode::Add | Mode::Assign => self.draw_prompt(f),
            Mode::Results => self.draw_results(f),
            Mode::Pick => self.draw_pick(f),
            _ => {}
        }
    }

    /// A shelf of books in the theme's colours, the tabs and the library's size.
    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let th = &self.theme;
        let [logo, right] = Layout::horizontal([Constraint::Length(13), Constraint::Min(10)]).areas(area);
        let frame = Style::new().fg(th.border);
        let mut books = vec![Span::styled("│ ", frame)];
        for (i, spine) in ["▆", "█", "▇", "▅", "█", "▆", "▇", "█"].into_iter().enumerate() {
            books.push(Span::styled(spine, Style::new().fg(th.stripes[i % th.stripes.len()])));
        }
        books.push(Span::styled(" │", frame));
        let shelf = vec![
            Line::from(Span::styled("╭──────────╮", frame)),
            Line::from(books),
            Line::from(vec![
                Span::styled("│ ", frame),
                Span::styled("▔▔▔▔▔▔▔▔", Style::new().fg(th.muted)),
                Span::styled(" │", frame),
            ]),
            Line::from(Span::styled("╰──────────╯", frame)),
        ];
        f.render_widget(Paragraph::new(shelf), logo);

        let [_, tabs, counts, line] = Layout::vertical([Constraint::Length(1); 4]).areas(right);
        let titles: Vec<Line> = TABS
            .iter()
            .map(|t| match *t {
                "Drafts" if !self.drafts.is_empty() => Line::from(vec![
                    Span::raw("Drafts "),
                    Span::styled(self.drafts.len().to_string(), Style::new().fg(th.warning)),
                ]),
                t => Line::from(t),
            })
            .collect();
        let at = [Tab::Library, Tab::Graph, Tab::People, Tab::Drafts].iter().position(|t| *t == self.tab).unwrap_or(0);
        f.render_widget(
            Tabs::new(titles)
                .select(at)
                .style(Style::new().fg(th.muted))
                .highlight_style(Style::new().fg(th.accent).bold())
                .divider(Span::styled("│", Style::new().fg(th.border))),
            tabs,
        );
        let pdfs = self.all.iter().filter(|p| p.pdf().is_some()).count();
        let projects = self.shelves.iter().filter(|(s, _)| matches!(s, Shelf::Project(_))).count();
        let summary = format!(
            "{} · {} · {} · {}",
            plural(self.all.len(), "paper"),
            plural(pdfs, "PDF"),
            plural(projects, "project"),
            plural(self.drafts.len(), "draft")
        );
        f.render_widget(Paragraph::new(Span::styled(format!(" {summary}"), Style::new().fg(th.muted))), counts);

        let query = if self.tab == Tab::People { &self.people_query } else { &self.query };
        let mut parts = vec![];
        if self.mode == Mode::Filter {
            parts.push(format!("filter: {query}▏"));
        } else if !query.is_empty() && matches!(self.tab, Tab::Library | Tab::People) {
            parts.push(format!("filter: {query}  (esc clears)"));
        }
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {}", parts.join(" · ")), Style::new().fg(th.warning))),
            line,
        );
    }

    /// Rounded, padded pane; the focused one gets the accent border.
    fn pane(&self, title: String, focused: bool) -> Block<'static> {
        let th = &self.theme;
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if focused { th.accent } else { th.border }))
            .title(Span::styled(
                format!(" {title} "),
                Style::new().fg(if focused { th.accent } else { th.title }).bold(),
            ))
            .padding(Padding::horizontal(1))
    }

    fn highlight(&self) -> Style {
        Style::new().bg(self.theme.selection_bg).fg(self.theme.selection_fg).bold()
    }

    fn badge(&self, status: Status, selected: bool) -> Cell<'static> {
        let th = &self.theme;
        let (label, colour) = match status {
            Status::Ready => ("ready", th.ready),
            Status::NoText => ("no text", th.failed),
            Status::NoPdf => ("no pdf", th.wanted),
            Status::Draft => ("draft", th.warning),
        };
        let colour = if selected { th.accent } else { colour };
        Cell::from(Span::styled(format!(" {label:^7} "), Style::new().bg(colour).fg(th.selection_bg).bold()))
    }

    fn draw_library(&mut self, f: &mut Frame, area: Rect) {
        let th = self.theme.clone();
        let [left, middle, right] =
            Layout::horizontal([Constraint::Percentage(22), Constraint::Percentage(43), Constraint::Percentage(35)])
                .areas(area);

        let items: Vec<ListItem> = self
            .shelves
            .iter()
            .map(|(shelf, n)| {
                let (mark, name, colour) = match shelf {
                    Shelf::All => ("", "All papers".to_string(), th.text),
                    Shelf::Project(name) => ("▪ ", name.clone(), th.text),
                    Shelf::Keyword(k) => ("# ", k.clone(), th.text),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::new().fg(th.title)),
                    Span::styled(name, Style::new().fg(colour)),
                    Span::styled(format!(" {n}"), Style::new().fg(th.muted)),
                ]))
            })
            .collect();
        let title = format!("Shelves ({})", self.shelves.len().saturating_sub(1));
        let list = List::new(items).block(self.pane(title, self.focus == 0)).highlight_style(self.highlight());
        f.render_stateful_widget(list, left, &mut self.shelf_list);

        let chosen = self.paper_table.selected();
        let rows: Vec<Row> = self
            .papers
            .iter()
            .enumerate()
            .map(|(i, (p, status))| {
                let year = p.bib.year().map(|y| y.to_string()).unwrap_or_default();
                let row = Row::new(vec![
                    self.badge(*status, chosen == Some(i)),
                    Cell::from(Span::styled(p.id.clone(), Style::new().fg(th.title))),
                    Cell::from(Span::styled(year, Style::new().fg(th.muted))),
                    Cell::from(p.bib.title()),
                ]);
                if chosen == Some(i) { row.style(self.highlight()) } else { row }
            })
            .collect();
        let header = Row::new(
            ["STATUS", "KEY", "YEAR", "TITLE"].map(|c| Cell::from(Span::styled(c, Style::new().fg(th.title).bold()))),
        );
        let shelf = match self.shelf() {
            Shelf::All => "Papers".to_string(),
            Shelf::Project(name) => name,
            Shelf::Keyword(k) => format!("# {k}"),
        };
        let table = Table::new(
            rows,
            [Constraint::Length(9), Constraint::Length(24), Constraint::Length(4), Constraint::Fill(1)],
        )
        .header(header)
        .block(self.pane(format!("{shelf} ({})", self.papers.len()), self.focus == 1));
        f.render_stateful_widget(table, middle, &mut self.paper_table);

        let text = match self.selected() {
            Some(p) => details(p, &th),
            None => Text::styled(
                "No papers here. a adds one by DOI, arXiv id, title or PDF path; A picks a PDF.",
                Style::new().fg(th.muted),
            ),
        };
        let view = Paragraph::new(text).wrap(Wrap { trim: false }).scroll((self.scroll, 0));
        f.render_widget(view.block(self.pane("Paper".into(), self.focus == 2)), right);
    }

    fn draw_graph(&mut self, f: &mut Frame, area: Rect) {
        let th = self.theme.clone();
        let areas: [Rect; 3] =
            Layout::horizontal([Constraint::Percentage(33), Constraint::Percentage(33), Constraint::Percentage(34)])
                .areas(area);
        let Some(g) = &self.graph else {
            let text = Paragraph::new("Select a paper in the Library first.").fg(th.muted);
            f.render_widget(text.block(self.pane("Graph".into(), true)), area);
            return;
        };
        let tree = |nodes: &[(usize, String, bool)]| -> Vec<ListItem<'static>> {
            nodes
                .iter()
                .map(|(level, id, again)| {
                    let indent = "  ".repeat(level.saturating_sub(1));
                    let mut spans = vec![Span::raw(indent), Span::styled(id.clone(), Style::new().fg(th.title))];
                    spans.push(match again {
                        true => Span::styled("  (see above)", Style::new().fg(th.muted)),
                        false => Span::raw(format!("  {}", self.titles.get(id).cloned().unwrap_or_default())),
                    });
                    ListItem::new(Line::from(spans))
                })
                .collect()
        };
        let empty = |what: &str| vec![ListItem::new(Span::styled(what.to_string(), Style::new().fg(th.muted)))];
        let cites = if g.cites.is_empty() {
            empty("none in the library, or references unknown (r looks them up)")
        } else {
            tree(&g.cites)
        };
        let cited_by = if g.cited_by.is_empty() { empty("no library paper cites it") } else { tree(&g.cited_by) };
        let missing: Vec<ListItem> = g
            .missing
            .iter()
            .map(|s| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:>2}× ", s.cited_by.len()), Style::new().fg(th.warning)),
                    Span::raw(s.reference.candidate(None).to_string()),
                ]))
            })
            .collect();
        let titles = [
            format!("Cites ({})", g.cites.iter().filter(|n| n.0 == 1).count()),
            format!("Cited by ({})", g.cited_by.iter().filter(|n| n.0 == 1).count()),
            "Cited most, not in the library".to_string(),
        ];
        let highlight = self.highlight();
        for (i, (items, title)) in [cites, cited_by, missing].into_iter().zip(titles).enumerate() {
            let list = List::new(items).block(self.pane(title, self.focus == i)).highlight_style(highlight);
            f.render_stateful_widget(list, areas[i], &mut self.graph_lists[i]);
        }
    }

    fn draw_people(&mut self, f: &mut Frame, area: Rect) {
        let th = self.theme.clone();
        let [left, middle, right] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(35), Constraint::Percentage(35)])
                .areas(area);
        let items: Vec<ListItem> = self
            .people
            .iter()
            .map(|p| {
                let mark = if p.id.is_some() { "✓ " } else { "" };
                let maybe = if p.maybe.is_empty() { "" } else { " ?" };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::new().fg(th.ready)),
                    Span::raw(p.name.clone()),
                    Span::styled(format!(" {}", p.papers.len()), Style::new().fg(th.muted)),
                    Span::styled(maybe, Style::new().fg(th.warning).bold()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(self.pane(format!("People ({})", self.people.len()), self.focus == 0))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, left, &mut self.people_list);

        let Some(person) = self.person().cloned() else { return };
        let papers: Vec<ListItem> = person
            .papers
            .iter()
            .map(|id| {
                ListItem::new(Line::from(vec![
                    Span::styled(id.clone(), Style::new().fg(th.title)),
                    Span::raw(format!("  {}", self.titles.get(id).cloned().unwrap_or_default())),
                ]))
            })
            .collect();
        let list = List::new(papers)
            .block(self.pane("Their papers".into(), self.focus == 1))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, middle, &mut self.person_papers);

        let [about, maybe] = Layout::vertical([Constraint::Min(6), Constraint::Length(8)]).areas(right);
        let label = |l: &str| Span::styled(format!("{l:>10}  "), Style::new().fg(th.title).bold());
        let mut lines = vec![Line::from(vec![label("name"), Span::raw(person.name.clone()).bold()])];
        if let Some(id) = &person.id {
            lines.push(Line::from(vec![
                label("confirmed"),
                Span::styled(format!("people/{id}.json"), Style::new().fg(th.ready)),
            ]));
        }
        lines.push(Line::from(vec![label("spellings"), Span::raw(person.variants.join("; "))]));
        if person.coauthors.is_empty() {
            lines.push(Line::from(vec![label("with"), Span::styled("nobody", Style::new().fg(th.muted))]));
        }
        for (i, (name, n)) in person.coauthors.iter().enumerate() {
            let head = if i == 0 { label("with") } else { label("") };
            lines.push(Line::from(vec![
                head,
                Span::raw(name.clone()),
                Span::styled(format!(" {n}"), Style::new().fg(th.muted)),
            ]));
        }
        let view = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll, 0));
        f.render_widget(view.block(self.pane("Person".into(), false)), about);

        let items: Vec<ListItem> = person.maybe.iter().map(|m| ListItem::new(m.to_string())).collect();
        let items = if items.is_empty() {
            vec![ListItem::new(Span::styled("nobody else's name fits", Style::new().fg(th.muted)))]
        } else {
            items
        };
        let list = List::new(items)
            .block(self.pane("Maybe the same person".into(), self.focus == 2))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, maybe, &mut self.maybe_list);
    }

    fn draw_drafts(&mut self, f: &mut Frame, area: Rect) {
        let th = self.theme.clone();
        let [left, right] = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(area);
        let items: Vec<ListItem> = self
            .drafts
            .iter()
            .map(|d| match d {
                Draft::Paper(p, field) => ListItem::new(Line::from(vec![
                    Span::styled(p.id.clone(), Style::new().fg(th.title)),
                    Span::styled(format!("  {}", field.name()), Style::new().fg(th.warning)),
                ])),
                Draft::People(d) => ListItem::new(Line::from(vec![
                    Span::styled(format!("people {}", d.id), Style::new().fg(th.title)),
                    Span::styled(
                        format!("  {}", if d.same { "one person" } else { "different" }),
                        Style::new().fg(th.warning),
                    ),
                ])),
            })
            .collect();
        let list = List::new(items)
            .block(self.pane(format!("Drafts ({})", self.drafts.len()), true))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, left, &mut self.draft_list);

        let heading = |s: &str| Line::from(Span::styled(s.to_string(), Style::new().fg(th.title).bold()));
        let mut lines = vec![];
        match self.draft() {
            None => lines.push(Line::from(Span::styled(
                "Nothing to review. Drafts appear when an AI proposes something.",
                Style::new().fg(th.muted),
            ))),
            Some(Draft::Paper(p, field)) => {
                lines.push(Line::from(p.bib.title().bold()));
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    format!("Proposed {}", field.name()),
                    Style::new().fg(th.warning).bold(),
                )));
                lines.extend(text_lines(&p.draft(*field).unwrap_or_default()));
                lines.push(Line::default());
                lines.push(heading(match field {
                    Field::Notes => "Current notes (the draft is added below them)",
                    Field::References => "Current references (the draft adds those not in it)",
                    Field::Keywords => "Current keywords (the draft adds those not there yet)",
                    Field::Summary => "Current summary (the draft replaces it)",
                }));
                let current = match field {
                    Field::Keywords => Some(p.bib.keywords().join("\n")),
                    _ => p.get(*field),
                };
                match current.filter(|c| !c.trim().is_empty()) {
                    Some(text) => lines.extend(text_lines(&text)),
                    None => lines.push(Line::from(Span::styled("(none)", Style::new().fg(th.muted)))),
                }
            }
            Some(Draft::People(d)) => {
                let what =
                    if d.same { "These spellings are one person:" } else { "These spellings are different people:" };
                lines.push(Line::from(Span::styled(what, Style::new().fg(th.warning).bold())));
                lines.extend(d.spellings.iter().map(|s| Line::from(format!("  {s}"))));
                lines.push(Line::default());
                lines.push(heading("Why"));
                lines.extend(text_lines(&d.reason));
            }
        }
        let view = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll, 0));
        f.render_widget(view.block(self.pane("Proposal".into(), false)), right);
    }

    fn draw_selected(&self, f: &mut Frame, area: Rect) {
        let th = &self.theme;
        let dim = |s: String| Span::styled(s, Style::new().fg(th.muted));
        let sep = || dim("  ·  ".into());
        let line = match self.tab {
            Tab::People => match self.person() {
                Some(p) => {
                    Line::from(vec![Span::raw(p.name.clone()).bold(), sep(), dim(plural(p.papers.len(), "paper"))])
                }
                None => Line::default(),
            },
            Tab::Drafts => match self.draft() {
                Some(d) => Line::from(Span::raw(describe(d)).bold()),
                None => Line::default(),
            },
            _ => match self.selected() {
                Some(p) => {
                    let mut spans = vec![
                        Span::styled(p.id.clone(), Style::new().fg(th.title)),
                        Span::raw("  "),
                        Span::raw(p.bib.title()).bold(),
                    ];
                    if let Some(pdf) = p.pdf() {
                        spans.extend([sep(), dim(pdf.display().to_string())]);
                    }
                    if let Some(doi) = p.bib.doi() {
                        spans.extend([sep(), dim(format!("doi:{doi}"))]);
                    }
                    Line::from(spans)
                }
                None => Line::default(),
            },
        };
        let title = match &self.busy {
            Some((label, _)) => format!("Selected · {label} {}", SPINNER[self.ticks % SPINNER.len()]),
            None => "Selected".to_string(),
        };
        f.render_widget(Paragraph::new(line).block(self.pane(title, self.busy.is_some())), area);
    }

    fn hints(&self) -> Line<'static> {
        let keys: &[(&str, &str)] = match (&self.mode, self.tab) {
            (Mode::Info, _) => &[("any key", "close")],
            (Mode::Confirm, _) => &[("y", "yes"), ("any other key", "no")],
            (Mode::Filter | Mode::Add | Mode::Assign, _) => &[("Enter", "done"), ("Esc", "cancel")],
            (Mode::Results, _) => &[("hjkl", "move"), ("Enter", "add"), ("Esc", "close")],
            (Mode::Pick, _) => {
                &[("jk", "move"), ("Enter/l", "open or choose"), ("h", "up"), ("~", "home"), ("Esc", "cancel")]
            }
            (_, Tab::Library) => &[
                ("Tab/[]", "tabs"),
                ("hjkl", "move"),
                ("/", "filter"),
                ("a", "add"),
                ("A", "add PDF"),
                ("i", "info"),
                ("o", "open"),
                ("e", "edit"),
                ("n", "notes"),
                ("p", "attach PDF"),
                ("s", "supplement"),
                ("u", "update"),
                ("r/R", "refs/cited by"),
                ("+/-", "project"),
                ("D", "delete"),
                ("q", "quit"),
            ],
            (_, Tab::Graph) => &[
                ("Tab/[]", "tabs"),
                ("hjkl", "move"),
                ("Enter", "follow / add"),
                ("a", "add"),
                ("i", "info"),
                ("q", "quit"),
            ],
            (_, Tab::People) => &[
                ("Tab/[]", "tabs"),
                ("hjkl", "move"),
                ("/", "filter"),
                ("Enter", "open paper"),
                ("m", "same person"),
                ("x", "different people"),
                ("q", "quit"),
            ],
            (_, Tab::Drafts) => &[
                ("Tab/[]", "tabs"),
                ("jk", "move"),
                ("a", "approve"),
                ("r", "reject"),
                ("e", "edit"),
                ("J/K", "scroll"),
                ("q", "quit"),
            ],
        };
        let th = &self.theme;
        let mut spans = vec![];
        for (i, (key, what)) in keys.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::new().fg(th.muted)));
            }
            spans.push(Span::styled(key.to_string(), Style::new().fg(th.accent).bold()));
            spans.push(Span::styled(format!(" {what}"), Style::new().fg(th.muted)));
        }
        Line::from(spans)
    }

    /// A box in the middle of the screen, `width` columns (at most the screen) and `height` rows.
    fn popup(f: &mut Frame, width: u16, height: u16) -> Rect {
        let area = f.area();
        let (w, h) = (width.min(area.width.saturating_sub(4)), height.min(area.height.saturating_sub(2)));
        let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        f.render_widget(Clear, rect);
        rect
    }

    fn draw_info(&self, f: &mut Frame) {
        let th = &self.theme;
        let width = self.info.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        let lines: Vec<Line> = self
            .info
            .iter()
            .map(|(k, v)| {
                Line::from(vec![
                    Span::styled(format!("{k:>width$}  "), Style::new().fg(th.title).bold()),
                    Span::raw(v.clone()),
                ])
            })
            .collect();
        let rect = Self::popup(f, f.area().width * 4 / 5, self.info.len() as u16 + 4);
        let view = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(view.block(self.pane("Paper".into(), true)), rect);
    }

    fn draw_confirm(&self, f: &mut Frame) {
        let th = &self.theme;
        let question = self.ask.as_ref().map(|(q, _)| q.clone()).unwrap_or_default();
        let rect = Self::popup(f, (question.chars().count() as u16 + 14).max(30), 3);
        let line = Line::from(vec![Span::raw(question), Span::styled("  [y/N]", Style::new().fg(th.warning).bold())]);
        f.render_widget(Paragraph::new(line).block(self.pane("Sure?".into(), true)), rect);
    }

    fn draw_prompt(&self, f: &mut Frame) {
        let title = match self.mode {
            Mode::Assign => {
                let known = self.lib.projects();
                format!(
                    "Put it in a project{}",
                    if known.is_empty() { String::new() } else { format!(" ({})", known.join(", ")) }
                )
            }
            _ => "Add: a PDF path, DOI, arXiv id or title".to_string(),
        };
        let rect = Self::popup(f, f.area().width * 3 / 5, 3);
        let line =
            Line::from(vec![Span::raw(self.input.clone()), Span::styled("▏", Style::new().fg(self.theme.accent))]);
        f.render_widget(Paragraph::new(line).block(self.pane(title, true)), rect);
    }

    fn draw_results(&mut self, f: &mut Frame) {
        let th = self.theme.clone();
        let items: Vec<ListItem> = self
            .results
            .iter()
            .map(|c| match &c.in_library {
                Some(_) => ListItem::new(Line::from(vec![
                    Span::styled("✓ ", Style::new().fg(th.ready)),
                    Span::styled(c.to_string(), Style::new().fg(th.muted)),
                ])),
                None => ListItem::new(format!("  {c}")),
            })
            .collect();
        let rect = Self::popup(f, f.area().width * 9 / 10, f.area().height * 4 / 5);
        let list = List::new(items)
            .block(self.pane(format!("{} · ✓ in the library", self.results_title), true))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, rect, &mut self.result_list);
    }

    fn draw_pick(&mut self, f: &mut Frame) {
        let th = self.theme.clone();
        let items: Vec<ListItem> = self
            .pick_entries
            .iter()
            .map(|p| {
                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                match p.is_dir() {
                    true => ListItem::new(Span::styled(format!("{name}/"), Style::new().fg(th.accent))),
                    false => ListItem::new(name),
                }
            })
            .collect();
        let purpose = match &self.picking {
            Picking::NewPaper => "a PDF to add".into(),
            Picking::Pdf(id) => format!("the PDF for {id}"),
            Picking::Supplement(id) => format!("a supplementary file for {id}"),
        };
        let rect = Self::popup(f, f.area().width * 4 / 5, f.area().height * 4 / 5);
        let list = List::new(items)
            .block(self.pane(format!("Pick {purpose} · {}", self.pick_dir.display()), true))
            .highlight_style(self.highlight());
        f.render_stateful_widget(list, rect, &mut self.pick_list);
    }
}

fn describe(draft: &Draft) -> String {
    match draft {
        Draft::Paper(p, field) => format!("the {} for {}", field.name(), p.id),
        Draft::People(d) => format!("people draft {} ({})", d.id, d.spellings.join("; ")),
    }
}

fn text_lines(text: &str) -> Vec<Line<'static>> {
    text.lines().map(|l| Line::from(l.to_string())).collect()
}

/// The right-hand pane of the Library.
fn details(p: &Paper, th: &Theme) -> Text<'static> {
    let b = &p.bib;
    let muted = Style::new().fg(th.muted);
    let all = b.authors();
    let mut authors = all.iter().take(10).cloned().collect::<Vec<_>>().join("; ");
    if all.len() > 10 {
        authors += &format!(" and {} more", all.len() - 10);
    }
    let mut lines = vec![Line::from(b.title().bold()), Line::from(Span::styled(authors, Style::new().fg(th.title)))];
    let facts = [b.year().map(|y| y.to_string()), b.venue()];
    lines.push(Line::from(Span::styled(facts.into_iter().flatten().collect::<Vec<_>>().join(" · "), muted)));
    let keywords = b.keywords();
    if !keywords.is_empty() {
        let tags: Vec<Span> = keywords
            .into_iter()
            .flat_map(|k| [Span::styled(format!("#{k}"), Style::new().fg(th.accent)), Span::raw(" ")])
            .collect();
        lines.push(Line::from(tags));
    }
    for field in p.drafts() {
        lines.push(Line::from(Span::styled(
            format!("✎ {} draft waiting (Drafts tab)", field.name()),
            Style::new().fg(th.warning),
        )));
    }
    for (heading, text) in
        [("Summary", p.get(Field::Summary)), ("Abstract", b.get("abstract")), ("Notes", p.get(Field::Notes))]
    {
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(heading, Style::new().fg(th.title).bold())));
            lines.extend(text_lines(&text));
        }
    }
    if p.get(Field::Summary).is_none() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("No summary yet. Ask your AI to read the paper and propose one.", muted)));
    }
    Text::from(lines)
}

/// Everything about a paper for the `i` popup.
fn info(p: &Paper, projects: &[String]) -> Vec<(&'static str, String)> {
    let b = &p.bib;
    let rows = [
        ("key", Some(p.id.clone())),
        ("title", Some(b.title())),
        ("authors", Some(b.authors().join("; "))),
        ("year", b.year().map(|y| y.to_string())),
        ("venue", b.venue()),
        ("doi", b.doi()),
        ("arXiv", b.arxiv()),
        ("also cited as", b.get("ids")),
        ("keywords", Some(b.keywords().join(", "))),
        ("projects", Some(projects.join(", "))),
        ("folder", Some(p.dir.display().to_string())),
        ("pdf", p.pdf().map(|f| f.display().to_string())),
        ("full text", Some(p.dir.join("fulltext.md")).filter(|f| f.exists()).map(|f| f.display().to_string())),
        ("figures", Some(p.figures().len()).filter(|n| *n > 0).map(|n| n.to_string())),
        ("supplementary", Some(p.supplementary().join(", "))),
    ];
    rows.into_iter().filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| (k, v))).collect()
}
