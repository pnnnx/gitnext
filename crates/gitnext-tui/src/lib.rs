//! Minimal interactive UI: file tree · symbols · detail, with search,
//! staging, commit and semantic diff. Vim-style keys, one accent color.

use anyhow::Result;
use gitnext_ops::gitnext_core::StatusEntry;
use gitnext_ops::gitnext_index::{dir_of, Graph, Index, SymRef};
use gitnext_ops::gitnext_parse::{lang_for, Kind, Lang};
use gitnext_ops::{CommitOpts, DiffMode, Workspace};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

mod git;
mod views;
use git::{Action, CRow};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

pub fn run(ws: &Workspace) -> Result<()> {
    let mut app = App::new(ws)?;
    let mut terminal = ratatui::init();
    let res = app.event_loop(&mut terminal);
    ratatui::restore();
    res
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Files,
    Symbols,
    Detail,
}

enum Mode {
    Normal,
    Help,
    Search { query: String, results: Vec<Hit>, sel: usize },
    /// Changes screen: groups → files → symbols
    Changes { sel: usize },
    Commit { input: String, all: bool },
    Confirm { question: String, action: Action },
    Palette { query: String, sel: usize },
    /// side-by-side before/after; `back` = Changes row to return to
    Split { title: String, rows: Vec<gitnext_ops::changes::DiffRow>, scroll: u16, back: usize },
    Timeline { entries: Vec<gitnext_ops::gitnext_core::ReflogEntry>, sel: usize },
    /// Scrollable text page (semantic diff, overview, journal).
    Page { title: String, lines: Vec<Line<'static>>, scroll: u16 },
}

#[derive(Clone)]
enum Hit {
    File(String),
    Sym(usize),
}

struct Row {
    path: String,
    name: String,
    depth: usize,
    is_dir: bool,
}

struct App<'w> {
    ws: &'w Workspace,
    index: Index,
    g: Graph,
    last_refresh: Instant,
    /// Selected link (caller / callee / subtype / method) in the detail pane.
    link_sel: usize,
    /// Symbols visited via links, for `b` (back).
    history: Vec<usize>,
    detail_height: u16,
    detail_width: u16,
    /// qualified → 'r' / 'e': what the agent read / edited in its latest session
    agent_marks: HashMap<String, char>,
    status: Vec<StatusEntry>,
    changed: HashSet<String>,
    /// Fully staged: everything changed in the file is in the next commit.
    staged: HashSet<String>,
    branch: String,
    repo_name: String,
    expanded: HashSet<String>,
    rows: Vec<Row>,
    files_state: ListState,
    syms: Vec<usize>,
    syms_state: ListState,
    detail_scroll: u16,
    focus: Pane,
    mode: Mode,
    flash: Option<(String, bool)>,
    quit: bool,
    cv: Option<gitnext_ops::changes::ChangesView>,
    crows: Vec<CRow>,
}

impl<'w> App<'w> {
    fn new(ws: &'w Workspace) -> Result<Self> {
        let mut index = Index::open(&ws.root)?;
        index.refresh()?;
        let g = index.graph()?;
        let repo_name = ws.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut expanded = HashSet::new();
        expanded.insert(String::new());
        let mut app = App {
            ws,
            index,
            g,
            last_refresh: Instant::now(),
            link_sel: 0,
            history: vec![],
            detail_height: 20,
            detail_width: 60,
            agent_marks: gitnext_ops::journal::last_session_marks(&gitnext_ops::journal::load(&ws.root)),
            status: vec![],
            changed: HashSet::new(),
            staged: HashSet::new(),
            branch: String::new(),
            repo_name,
            expanded,
            rows: vec![],
            files_state: ListState::default().with_selected(Some(0)),
            syms: vec![],
            syms_state: ListState::default(),
            detail_scroll: 0,
            focus: Pane::Files,
            mode: Mode::Normal,
            flash: None,
            quit: false,
            cv: None,
            crows: vec![],
        };
        // Expand top-level directories that contain few entries (keeps first screen useful).
        let tops: Vec<String> = app.g.dirs().into_iter().filter(|d| !d.is_empty() && !d.contains('/')).collect();
        if tops.len() <= 3 {
            app.expanded.extend(tops);
        }
        app.reload_status();
        app.rebuild_rows();
        app.on_file_change();
        let welcomed = ws.root.join(gitnext_ops::gitnext_index::GITNEXT_DIR).join("tui-welcomed");
        if std::env::var_os("GITNEXT_NO_OVERVIEW").is_some() {
        } else if !welcomed.exists() {
            app.open_welcome(); // first run: how this works
            let _ = std::fs::create_dir_all(welcomed.parent().unwrap());
            let _ = std::fs::write(&welcomed, "");
        } else if !app.g.files.is_empty() {
            app.open_overview(); // first screen: what is this repo
        }
        Ok(app)
    }

    // ───────────────────────────── data ─────────────────────────────

    fn reload_status(&mut self) {
        self.agent_marks = gitnext_ops::journal::last_session_marks(&gitnext_ops::journal::load(&self.ws.root));
        if let Some(repo) = &self.ws.repo {
            self.branch = repo.branch();
            self.status = repo.status().unwrap_or_default();
            self.changed = self.status.iter().map(|e| e.path.clone()).collect();
            self.staged = self.status.iter().filter(|e| e.staged.is_some() && e.unstaged.is_none()).map(|e| e.path.clone()).collect();
        } else {
            self.branch = "(no git)".into();
        }
    }

    /// Re-index; returns the number of changed files. Keeps the selection.
    fn reload_graph(&mut self) -> usize {
        let changed = match self.index.refresh() {
            Ok(st) => st.files_reparsed + st.files_removed,
            Err(e) => {
                self.error(format!("index: {e:#}"));
                return 0;
            }
        };
        match self.index.graph() {
            Ok(g) => {
                let keep = self.selected_row().map(|r| r.path.clone());
                let keep_sym = self.syms_state.selected().and_then(|i| self.syms.get(i)).map(|&i| self.g.syms[i].sym.qualified.clone());
                let focus = self.focus;
                self.g = g;
                self.history.clear();
                self.rebuild_rows();
                if let Some(p) = keep {
                    self.select_path(&p);
                }
                self.on_file_change();
                if let Some(q) = keep_sym {
                    if let Some(pos) = self.syms.iter().position(|&i| self.g.syms[i].sym.qualified == q) {
                        self.syms_state.select(Some(pos));
                    }
                }
                self.focus = focus;
            }
            Err(e) => self.error(format!("index: {e:#}")),
        }
        changed
    }

    fn rebuild_rows(&mut self) {
        let dirs: BTreeSet<String> = self.g.dirs().into_iter().collect();
        let mut rows = Vec::new();
        fn walk(app: &App, dirs: &BTreeSet<String>, dir: &str, depth: usize, rows: &mut Vec<Row>) {
            let subdirs: Vec<&String> = dirs.iter().filter(|d| !d.is_empty() && dir_of(d) == dir).collect();
            for d in subdirs {
                let name = d.rsplit('/').next().unwrap_or(d).to_string();
                rows.push(Row { path: d.clone(), name, depth, is_dir: true });
                if app.expanded.contains(d.as_str()) {
                    walk(app, dirs, d, depth + 1, rows);
                }
            }
            for f in app.g.files.iter().filter(|f| dir_of(&f.path) == dir) {
                let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                rows.push(Row { path: f.path.clone(), name, depth, is_dir: false });
            }
        }
        walk(self, &dirs, "", 0, &mut rows);
        self.rows = rows;
        let sel = self.files_state.selected().unwrap_or(0).min(self.rows.len().saturating_sub(1));
        self.files_state.select(Some(sel));
    }

    fn selected_row(&self) -> Option<&Row> {
        self.files_state.selected().and_then(|i| self.rows.get(i))
    }

    fn select_path(&mut self, path: &str) {
        // expand ancestors
        let mut d = dir_of(path);
        while !d.is_empty() {
            self.expanded.insert(d.to_string());
            d = dir_of(d);
        }
        self.rebuild_rows();
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.files_state.select(Some(i));
        }
    }

    fn on_file_change(&mut self) {
        self.detail_scroll = 0;
        self.link_sel = 0;
        self.syms = match self.selected_row() {
            Some(r) if !r.is_dir => self
                .g
                .in_file(&r.path)
                .into_iter()
                .filter(|&i| !(self.g.syms[i].sym.kind == Kind::Module && self.g.syms[i].sym.start_line == 1))
                .collect(),
            _ => vec![],
        };
        self.syms_state.select(if self.syms.is_empty() { None } else { Some(0) });
    }

    fn dir_changed(&self, dir: &str) -> bool {
        let prefix = format!("{dir}/");
        self.changed.iter().any(|p| p.starts_with(&prefix))
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), false));
    }
    fn error(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), true));
    }

    // ───────────────────────────── events ─────────────────────────────

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(400))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        self.on_key(k);
                    }
                }
            } else {
                self.tick();
            }
        }
        Ok(())
    }

    /// Live update: re-index when files change on disk (e.g. edited in an editor).
    fn tick(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_millis(1200) || !matches!(self.mode, Mode::Normal) {
            return;
        }
        self.last_refresh = Instant::now();
        let n = self.reload_graph();
        self.reload_status();
        if n > 0 {
            self.flash(format!("↻ updated · {n} file{} changed", if n == 1 { "" } else { "s" }));
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('p') && !matches!(self.mode, Mode::Palette { .. } | Mode::Commit { .. }) {
            self.flash = None;
            self.mode = Mode::Palette { query: String::new(), sel: 0 };
            return;
        }
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        self.mode = match mode {
            Mode::Normal => {
                self.flash = None;
                self.key_normal(k);
                return;
            }
            Mode::Help => Mode::Normal,
            Mode::Search { query, results, sel } => self.key_search(k, query, results, sel),
            Mode::Changes { sel } => self.key_changes(k, sel),
            Mode::Commit { input, all } => self.key_commit(k, input, all),
            Mode::Confirm { question, action } => self.key_confirm(k, question, action),
            Mode::Palette { query, sel } => self.key_palette(k, query, sel),
            Mode::Timeline { entries, sel } => self.key_timeline(k, entries, sel),
            Mode::Split { title, rows, scroll, back } => match k.code {
                KeyCode::Down | KeyCode::Char('j') => Mode::Split { title, rows, scroll: scroll.saturating_add(1), back },
                KeyCode::Up | KeyCode::Char('k') => Mode::Split { title, rows, scroll: scroll.saturating_sub(1), back },
                KeyCode::PageDown | KeyCode::Char(' ') => Mode::Split { title, rows, scroll: scroll.saturating_add(15), back },
                KeyCode::PageUp => Mode::Split { title, rows, scroll: scroll.saturating_sub(15), back },
                _ if self.cv.is_some() => Mode::Changes { sel: back.min(self.crows.len().saturating_sub(1)) },
                _ => Mode::Normal,
            },
            Mode::Page { title, lines, scroll } => match k.code {
                KeyCode::Down | KeyCode::Char('j') => Mode::Page { title, lines, scroll: scroll.saturating_add(1) },
                KeyCode::Up | KeyCode::Char('k') => Mode::Page { title, lines, scroll: scroll.saturating_sub(1) },
                KeyCode::PageDown | KeyCode::Char(' ') => Mode::Page { title, lines, scroll: scroll.saturating_add(15) },
                KeyCode::PageUp => Mode::Page { title, lines, scroll: scroll.saturating_sub(15) },
                _ => Mode::Normal, // any other key closes the page
            },
        };
    }

    fn key_normal(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('/') => self.mode = Mode::Search { query: String::new(), results: vec![], sel: 0 },
            KeyCode::Char('s') => self.open_changes(),
            KeyCode::Char('U') => self.ask_undo(),
            KeyCode::Char('H') => self.open_timeline(),
            KeyCode::Char('B') => self.open_branches(),
            KeyCode::Char(':') => self.mode = Mode::Palette { query: String::new(), sel: 0 },
            KeyCode::Char('c') => self.open_commit(),
            KeyCode::Char('d') => self.open_diff(),
            KeyCode::Char('b') => self.back(),
            KeyCode::Char('o') => self.open_overview(),
            KeyCode::Char('a') => self.open_journal(),
            KeyCode::Backspace if self.focus == Pane::Detail && !self.history.is_empty() => self.back(),
            KeyCode::Char('r') => {
                self.reload_graph();
                self.reload_status();
                self.flash(format!("reindexed · {}", self.g.counts().line()));
            }
            KeyCode::Char('m') => match self.ws.write_map(&self.g) {
                Ok(o) => self.flash(format!("map: {} written, {} unchanged", o.written.len(), o.unchanged)),
                Err(e) => self.error(format!("map: {e:#}")),
            },
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Pane::Files => Pane::Symbols,
                    Pane::Symbols => Pane::Detail,
                    Pane::Detail => Pane::Files,
                }
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Pane::Files => Pane::Detail,
                    Pane::Symbols => Pane::Files,
                    Pane::Detail => Pane::Symbols,
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::PageDown => self.move_sel(15),
            KeyCode::PageUp => self.move_sel(-15),
            KeyCode::Char('g') | KeyCode::Home => self.move_sel(-100_000),
            KeyCode::Char('G') | KeyCode::End => self.move_sel(100_000),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => self.go_in(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.go_out(),
            _ => {}
        }
    }

    fn move_sel(&mut self, delta: i32) {
        let step = |cur: usize, len: usize| -> usize {
            if len == 0 {
                return 0;
            }
            (cur as i64 + delta as i64).clamp(0, len as i64 - 1) as usize
        };
        match self.focus {
            Pane::Files => {
                let cur = self.files_state.selected().unwrap_or(0);
                let n = step(cur, self.rows.len());
                if n != cur {
                    self.files_state.select(Some(n));
                    self.on_file_change();
                }
            }
            Pane::Symbols => {
                let cur = self.syms_state.selected().unwrap_or(0);
                self.syms_state.select(Some(step(cur, self.syms.len())));
                self.detail_scroll = 0;
                self.link_sel = 0;
            }
            Pane::Detail => {
                // j/k walk links when there are any; PgUp/PgDn always scroll
                let links = self.current_symbol().map(|i| self.symbol_detail(i).1).unwrap_or_default();
                if !links.is_empty() && delta.abs() == 1 {
                    self.link_sel = step(self.link_sel, links.len());
                    let lines = self.current_symbol().map(|i| self.symbol_detail(i).0).unwrap_or_default();
                    let (_, rows) = wrap_hanging(lines, self.detail_width as usize);
                    let line = rows.get(links[self.link_sel].0).copied().unwrap_or(links[self.link_sel].0) as u16;
                    let h = self.detail_height.saturating_sub(3);
                    if line < self.detail_scroll || line >= self.detail_scroll + h {
                        self.detail_scroll = line.saturating_sub(h / 3);
                    }
                } else {
                    self.detail_scroll = (self.detail_scroll as i32 + delta).clamp(0, 10_000) as u16;
                }
            }
        }
    }

    fn go_in(&mut self) {
        match self.focus {
            Pane::Files => {
                let Some(r) = self.selected_row() else { return };
                if r.is_dir {
                    let p = r.path.clone();
                    if !self.expanded.remove(&p) {
                        self.expanded.insert(p);
                    }
                    self.rebuild_rows();
                } else if !self.syms.is_empty() {
                    self.focus = Pane::Symbols;
                }
            }
            Pane::Symbols => {
                self.focus = Pane::Detail;
                self.link_sel = 0;
            }
            Pane::Detail => {
                // follow the selected link
                let Some(cur) = self.current_symbol() else { return };
                let links = self.symbol_detail(cur).1;
                if let Some(&(_, target)) = links.get(self.link_sel) {
                    self.history.push(cur);
                    self.goto_symbol(target);
                }
            }
        }
    }

    fn goto_symbol(&mut self, i: usize) {
        let file = self.g.syms[i].file.clone();
        self.select_path(&file);
        self.on_file_change();
        if let Some(pos) = self.syms.iter().position(|&s| s == i) {
            self.syms_state.select(Some(pos));
        }
        self.focus = Pane::Detail;
        self.detail_scroll = 0;
        self.link_sel = 0;
    }

    fn back(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.goto_symbol(prev);
        } else {
            self.flash("history is empty");
        }
    }

    fn ref_index(&self, r: &SymRef) -> Option<usize> {
        self.g
            .find(&r.qualified)
            .into_iter()
            .chain(self.g.in_file(&r.file))
            .find(|&i| self.g.syms[i].file == r.file && self.g.syms[i].sym.start_line == r.line)
    }

    fn go_out(&mut self) {
        match self.focus {
            Pane::Files => {
                let Some(r) = self.selected_row() else { return };
                if r.is_dir && self.expanded.contains(&r.path) {
                    let p = r.path.clone();
                    self.expanded.remove(&p);
                    self.rebuild_rows();
                } else {
                    let parent = dir_of(&r.path).to_string();
                    if !parent.is_empty() {
                        if let Some(i) = self.rows.iter().position(|x| x.path == parent) {
                            self.files_state.select(Some(i));
                            self.on_file_change();
                        }
                    }
                }
            }
            Pane::Symbols => self.focus = Pane::Files,
            Pane::Detail => self.focus = Pane::Symbols,
        }
    }

    fn key_search(&mut self, k: KeyEvent, mut query: String, mut results: Vec<Hit>, mut sel: usize) -> Mode {
        match k.code {
            KeyCode::Esc => return Mode::Normal,
            KeyCode::Enter => {
                if let Some(h) = results.get(sel).cloned() {
                    self.jump(h);
                }
                return Mode::Normal;
            }
            KeyCode::Down | KeyCode::Tab => sel = (sel + 1).min(results.len().saturating_sub(1)),
            KeyCode::Up | KeyCode::BackTab => sel = sel.saturating_sub(1),
            KeyCode::Backspace => {
                query.pop();
                results = self.search(&query);
                sel = 0;
            }
            KeyCode::Char(c) => {
                query.push(c);
                results = self.search(&query);
                sel = 0;
            }
            _ => {}
        }
        Mode::Search { query, results, sel }
    }

    fn search(&self, q: &str) -> Vec<Hit> {
        if q.is_empty() {
            return vec![];
        }
        let ql = q.to_lowercase();
        let mut hits: Vec<Hit> = self.g.search(q, 30).into_iter().map(Hit::Sym).collect();
        let files = self
            .g
            .files
            .iter()
            .filter(|f| f.path.to_lowercase().contains(&ql))
            .take(10)
            .map(|f| Hit::File(f.path.clone()));
        hits.extend(files);
        hits
    }

    fn jump(&mut self, h: Hit) {
        match h {
            Hit::File(p) => {
                self.select_path(&p);
                self.on_file_change();
                self.focus = Pane::Files;
            }
            Hit::Sym(i) => {
                let file = self.g.syms[i].file.clone();
                self.select_path(&file);
                self.on_file_change();
                if let Some(pos) = self.syms.iter().position(|&s| s == i) {
                    self.syms_state.select(Some(pos));
                }
                self.focus = Pane::Symbols;
            }
        }
    }

    fn open_commit(&mut self) {
        if self.ws.repo.is_none() {
            return self.error("not a git repository");
        }
        self.reload_status();
        if self.status.is_empty() {
            return self.flash("nothing to commit, working tree clean");
        }
        let any_staged = self.status.iter().any(|e| e.staged.is_some());
        self.mode = Mode::Commit { input: self.commit_draft(), all: !any_staged };
    }

    fn key_commit(&mut self, k: KeyEvent, mut input: String, mut all: bool) -> Mode {
        match k.code {
            KeyCode::Esc => return Mode::Normal,
            KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => all = !all,
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => input.clear(),
            KeyCode::Enter => {
                if input.trim().is_empty() {
                    self.error("write a few words about what this commit does");
                    return Mode::Commit { input, all };
                }
                let opts = CommitOpts { all, ..CommitOpts::default() };
                match gitnext_ops::commit(self.ws, &input, &opts) {
                    Ok(r) => {
                        let n: usize = r.changes.iter().map(|c| c.changes.len()).sum();
                        self.flash(format!("✓ {} {} · {n} symbol changes", &r.id[..7], input.trim()));
                        self.reload_graph();
                        self.reload_status();
                    }
                    Err(e) => self.error(gitnext_ops::changes::humanize(&e)),
                }
                return Mode::Normal;
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => input.push(c),
            _ => {}
        }
        Mode::Commit { input, all }
    }

    fn open_overview(&mut self) {
        let text = gitnext_ops::insight::overview(self.ws.repo.as_ref(), &self.g, &self.ws.root);
        self.mode = Mode::Page { title: " overview · any key to close ".into(), lines: style_page(&text), scroll: 0 };
    }

    fn open_journal(&mut self) {
        let text = gitnext_ops::journal::render(&gitnext_ops::journal::load(&self.ws.root), 1);
        self.mode = Mode::Page { title: " agent journal · last session ".into(), lines: style_page(&text), scroll: 0 };
    }

    fn open_diff(&mut self) {
        let Some(repo) = &self.ws.repo else { return self.error("not a git repository") };
        match gitnext_ops::semantic_diff(repo, DiffMode::Worktree) {
            Ok(d) => {
                let mut lines = Vec::new();
                if d.is_empty() {
                    lines.push(Line::from("no changes".fg(DIM)));
                }
                for fc in d {
                    let mut l = vec![Span::styled(format!("{:<9}", fc.status), Style::new().bold()), Span::raw(fc.path.clone())];
                    if !fc.semantic {
                        l.push(" (no parser)".fg(DIM));
                    }
                    lines.push(Line::from(l));
                    for c in &fc.changes {
                        let s = c.line();
                        let color = match s.chars().next() {
                            Some('+') => Color::Green,
                            Some('-') => Color::Red,
                            Some('~') => Color::Yellow,
                            _ => ACCENT,
                        };
                        lines.push(Line::from(Span::styled(format!("   {s}"), Style::new().fg(color))));
                    }
                }
                self.mode = Mode::Page { title: " semantic diff · HEAD → working tree ".into(), lines, scroll: 0 };
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    // ───────────────────────────── drawing ─────────────────────────────

    fn draw(&mut self, f: &mut Frame) {
        let [top, main, bottom] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        self.draw_top(f, top);
        let [a, b, c] = Layout::horizontal([
            Constraint::Percentage(26),
            Constraint::Percentage(28),
            Constraint::Percentage(46),
        ])
        .areas(main);
        self.detail_height = c.height;
        self.detail_width = c.width.saturating_sub(2);
        self.draw_files(f, a);
        self.draw_symbols(f, b);
        self.draw_detail(f, c);
        self.draw_bottom(f, bottom);

        match &self.mode {
            Mode::Normal => {}
            Mode::Help => draw_help(f),
            Mode::Search { .. } => self.draw_search(f),
            Mode::Changes { sel } => self.draw_changes(f, *sel),
            Mode::Commit { input, all } => self.draw_commit(f, input, *all),
            Mode::Confirm { question, .. } => self.draw_confirm(f, question),
            Mode::Palette { query, sel } => self.draw_palette(f, query, *sel),
            Mode::Split { title, rows, scroll, .. } => self.draw_split(f, title, rows, *scroll),
            Mode::Timeline { entries, sel } => self.draw_timeline(f, entries, *sel),
            Mode::Page { title, lines, scroll } => {
                let area = screen_area(f.area());
                f.render_widget(Clear, area);
                let (lines, _) = wrap_hanging(lines.clone(), area.width.saturating_sub(2) as usize);
                let p = Paragraph::new(Text::from(lines)).block(pane_block(title, true)).scroll((*scroll, 0));
                f.render_widget(p, area);
            }
        }
    }

    fn draw_top(&self, f: &mut Frame, area: Rect) {
        let n = self.status.len();
        let mut spans = vec![
            " gitnext ".bold().fg(ACCENT),
            Span::raw("· "),
            Span::raw(self.repo_name.clone()).bold(),
            " · ".fg(DIM),
            Span::raw(self.branch.clone()),
        ];
        if n > 0 {
            spans.push(" · ".fg(DIM));
            spans.push(Span::styled(format!("{n} changed"), Style::new().fg(Color::Yellow)));
        }
        let syms = self.g.syms.len();
        let c = self.g.counts();
        let _ = syms;
        let right = format!("{} code files · {} functions and types   ? help ", c.code_files, c.symbols);
        let w = area.width as usize;
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let pad = w.saturating_sub(left_len + right.chars().count());
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(right.fg(DIM));
        f.render_widget(Line::from(spans), area);
    }

    fn draw_bottom(&self, f: &mut Frame, area: Rect) {
        let line = match &self.flash {
            Some((m, true)) => Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::Red))),
            Some((m, false)) => Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::Green))),
            None => {
                let keys: &[(&str, &str)] = match self.mode {
                    Mode::Changes { .. } => &[("space", "stage file / function"), ("enter", "before/after"), ("c", "commit"), ("U", "undo commit"), ("esc", "close")],
                    Mode::Commit { .. } => &[("enter", "commit"), ("ctrl-u", "clear"), ("ctrl-a", "toggle stage all"), ("esc", "cancel")],
                    Mode::Split { .. } => &[("jk", "scroll"), ("esc", "back")],
                    Mode::Page { .. } => &[("jk", "scroll"), ("pgup/pgdn", "page"), ("any other key", "close")],
                    Mode::Timeline { .. } => &[("jk", "move"), ("enter", "go back to this point"), ("esc", "close")],
                    Mode::Palette { .. } => &[("type", "filter"), ("enter", "run"), ("esc", "close")],
                    Mode::Confirm { .. } => &[("y", "yes"), ("n", "no")],
                    Mode::Search { .. } => &[("↑↓", "select"), ("enter", "jump"), ("esc", "cancel")],
                    _ if self.focus == Pane::Detail => &[("jk", "links"), ("enter", "jump"), ("b", "back"), ("/", "search"), ("ctrl-p", "all")],
                    _ if self.focus == Pane::Symbols => &[("enter", "details"), ("/", "search"), ("s", "changes"), ("c", "commit"), ("ctrl-p", "all")],
                    _ => &[("enter", "open"), ("/", "search"), ("s", "changes"), ("o", "overview"), ("ctrl-p", "all")],
                };
                let mut spans = vec![Span::raw(" ")];
                for (k, v) in keys {
                    spans.push(Span::styled(*k, Style::new().fg(ACCENT)));
                    spans.push(Span::styled(format!(" {v}   "), Style::new().fg(DIM)));
                }
                // the tree marks, where they are visible
                if matches!(self.mode, Mode::Normal) && self.focus == Pane::Files && !self.changed.is_empty() {
                    spans.push(Span::styled("+", Style::new().fg(Color::Green)));
                    spans.push(Span::styled(" staged  ", Style::new().fg(DIM)));
                    spans.push(Span::styled("•", Style::new().fg(Color::Yellow)));
                    spans.push(Span::styled(" changed", Style::new().fg(DIM)));
                }
                Line::from(spans)
            }
        };
        f.render_widget(line, area);
    }

    fn draw_files(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|r| {
                let indent = "  ".repeat(r.depth);
                let (icon, changed) = if r.is_dir {
                    (if self.expanded.contains(&r.path) { "▾ " } else { "▸ " }, self.dir_changed(&r.path))
                } else {
                    ("  ", self.changed.contains(&r.path))
                };
                let mut spans = vec![Span::raw(indent), Span::styled(icon, Style::new().fg(DIM))];
                let name_style = if r.is_dir { Style::new().bold() } else { Style::new() };
                spans.push(Span::styled(r.name.clone(), name_style));
                if changed {
                    // + staged (goes into the next commit) · • changed, not staged
                    if !r.is_dir && self.staged.contains(&r.path) {
                        spans.push(Span::styled(" +", Style::new().fg(Color::Green)));
                    } else {
                        spans.push(Span::styled(" •", Style::new().fg(Color::Yellow)));
                    }
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(pane_block(" files ", self.focus == Pane::Files))
            .highlight_style(hl(self.focus == Pane::Files));
        f.render_stateful_widget(list, area, &mut self.files_state);
    }

    fn draw_symbols(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .syms
            .iter()
            .map(|&i| {
                let s = &self.g.syms[i].sym;
                let nested = s.kind == Kind::Method || s.parent.as_deref().is_some_and(|p| p.contains("::") && self.g.syms.iter().any(|x| x.sym.qualified == p && x.sym.kind != Kind::Module));
                // one kind column for every row; members are indented under their type by name
                let indent = if nested { "  " } else { "" };
                let style = if s.is_public() { Style::new() } else { Style::new().fg(DIM) };
                let style = if s.kind.is_type() { style.bold() } else { style };
                let mut spans = vec![
                    Span::styled(format!("{:<7}", views::kind_word(s.kind)), Style::new().fg(DIM)),
                    Span::raw(indent),
                    Span::styled(s.name.clone(), style),
                ];
                match self.agent_marks.get(&s.qualified) {
                    Some('e') => spans.push(Span::styled("  edited by agent", Style::new().fg(Color::Magenta))),
                    Some(_) => spans.push(Span::styled("  read by agent", Style::new().fg(Color::Magenta))),
                    None => {}
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let title = if self.syms.is_empty() { " symbols ".to_string() } else { format!(" symbols · {} ", self.syms.len()) };
        let list = List::new(items)
            .block(pane_block(&title, self.focus == Pane::Symbols))
            .highlight_style(hl(self.focus == Pane::Symbols));
        f.render_stateful_widget(list, area, &mut self.syms_state);
    }

    fn draw_detail(&self, f: &mut Frame, area: Rect) {
        if let Some(i) = self.current_symbol() {
            let (mut lines, links) = self.symbol_detail(i);
            if self.focus == Pane::Detail {
                if let Some(&(li, _)) = links.get(self.link_sel) {
                    if let Some(l) = lines.get_mut(li) {
                        *l = l.clone().style(hl(true));
                    }
                }
            }
            let title = if self.history.is_empty() { " detail ".to_string() } else { format!(" detail · b back ({}) ", self.history.len()) };
            // wrapped here, not by the Paragraph: continuation lines keep the indent
            let (lines, _) = wrap_hanging(lines, area.width.saturating_sub(2) as usize);
            let p = Paragraph::new(Text::from(lines)).block(pane_block(&title, self.focus == Pane::Detail)).scroll((self.detail_scroll, 0));
            f.render_widget(p, area);
            return;
        }
        let (title, text) = if let Some(r) = self.selected_row() {
            if r.is_dir {
                (" map ".to_string(), self.dir_detail(&r.path))
            } else {
                (" file ".to_string(), self.file_detail(&r.path))
            }
        } else {
            (" detail ".to_string(), Text::from("empty repository — add some files".fg(DIM)))
        };
        let (lines, _) = wrap_hanging(text.lines, area.width.saturating_sub(2) as usize);
        let p = Paragraph::new(Text::from(lines)).block(pane_block(&title, self.focus == Pane::Detail)).scroll((self.detail_scroll, 0));
        f.render_widget(p, area);
    }

    fn current_symbol(&self) -> Option<usize> {
        if self.focus == Pane::Files {
            return None;
        }
        self.syms_state.selected().and_then(|i| self.syms.get(i)).copied()
    }

    /// Detail lines for a symbol plus navigable links `(line index, target symbol)`.
    fn symbol_detail(&self, i: usize) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
        self.symbol_view(i)
    }

    fn file_detail(&self, path: &str) -> Text<'static> {
        let mut l: Vec<Line> = vec![];
        let Some(f) = self.g.files.iter().find(|f| f.path == path) else { return Text::raw("") };
        l.push(Line::from(Span::styled(path.to_string(), Style::new().bold())));
        l.push(Line::from(format!("{} · {} lines", f.file_type, f.lines).fg(DIM)));
        if let Some(e) = self.status.iter().find(|e| e.path == path) {
            let s = format!(
                "git: {}{}",
                e.staged.map(|c| format!("staged {c} ")).unwrap_or_default(),
                e.unstaged.map(|c| if c == '?' { "untracked".to_string() } else { format!("modified {c}") }).unwrap_or_default()
            );
            l.push(Line::from(Span::styled(s, Style::new().fg(Color::Yellow))));
        }
        if let Some(d) = &f.module_doc {
            l.push(Line::raw(""));
            for x in d.lines() {
                l.push(Line::from(vec!["│ ".fg(DIM), Span::raw(x.to_string())]));
            }
        }
        if !self.syms.is_empty() {
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for &i in &self.syms {
                *counts.entry(self.g.syms[i].sym.kind.as_str()).or_default() += 1;
            }
            let mut c: Vec<String> = counts.into_iter().map(|(k, n)| format!("{n} {k}")).collect();
            c.sort();
            l.push(Line::raw(""));
            l.push(Line::from(c.join(", ").fg(DIM)));
            l.push(Line::raw(""));
            for &i in self.syms.iter().filter(|&&i| !self.g.syms[i].sym.test).take(60) {
                let sym = &self.g.syms[i].sym;
                let indent = if sym.kind == Kind::Method { "    " } else { "  " };
                let mut spans = vec![Span::raw(indent), Span::styled(format!("{:<7}", views::kind_word(sym.kind)), Style::new().fg(DIM))];
                spans.extend(views::call_spans(sym, if sym.is_public() { Style::new().bold() } else { Style::new() }));
                l.push(Line::from(spans));
            }
            l.push(Line::raw(""));
            l.push(Line::from("→ or l opens the list of symbols".fg(DIM)));
        } else if !f.parsed {
            // Preview non-code files (README, Cargo.toml…)
            if let Ok(t) = std::fs::read_to_string(self.ws.root.join(path)) {
                l.push(Line::raw(""));
                for x in t.lines().take(200) {
                    l.push(Line::raw(x.to_string()));
                }
            }
        }
        Text::from(l)
    }

    fn dir_detail(&self, dir: &str) -> Text<'static> {
        self.dir_view(dir)
    }

    fn draw_search(&self, f: &mut Frame) {
        let Mode::Search { query, results, sel } = &self.mode else { return };
        let area = popup(f.area(), 60, 60);
        f.render_widget(Clear, area);
        let [input, list] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(area);
        f.render_widget(
            Paragraph::new(Line::from(vec!["/ ".fg(ACCENT), Span::raw(query.clone()), "▏".fg(ACCENT)]))
                .block(pane_block(" search symbols & files ", true)),
            input,
        );
        let items: Vec<ListItem> = results
            .iter()
            .map(|h| match h {
                Hit::Sym(i) => {
                    let r = &self.g.syms[*i];
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{} ", r.sym.kind.icon()), Style::new().fg(ACCENT)),
                        Span::raw(r.sym.name.clone()),
                        Span::styled(format!("  {}", short_q(&r.sym.qualified)), Style::new().fg(DIM)),
                    ]))
                }
                Hit::File(p) => ListItem::new(Line::from(vec!["▤ ".fg(ACCENT), Span::raw(p.clone())])),
            })
            .collect();
        let mut st = ListState::default().with_selected(if results.is_empty() { None } else { Some(*sel) });
        f.render_stateful_widget(
            List::new(items).block(pane_block("", true)).highlight_style(hl(true)),
            list,
            &mut st,
        );
    }

    fn draw_commit(&self, f: &mut Frame, input: &str, all: bool) {
        let area = popup(f.area(), 60, 30);
        let area = Rect { height: area.height.clamp(8, 9), ..area };
        f.render_widget(Clear, area);
        let staged = self.status.iter().filter(|e| e.staged.is_some()).count();
        let lines = vec![
            Line::from(vec!["› ".fg(ACCENT), Span::raw(input.to_string()), "▏".fg(ACCENT)]),
            Line::raw(""),
            Line::from(vec![
                Span::raw(if all { "[x] " } else { "[ ] " }),
                Span::raw("stage all changes"),
                format!("   ({staged} staged, {} changed)", self.status.len()).fg(DIM),
            ]),
            Line::from("suggested from your changes — edit it or press enter · the full list of changed symbols is added below it".fg(DIM)),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane_block(" commit message ", true)), area);
    }
}

#[derive(Default)]
struct HlState {
    /// inside `/* ... */`
    block_comment: bool,
    /// inside a Python triple-quoted string (the quote char)
    triple: Option<char>,
}

const KW_RUST: &[&str] = &["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "for", "in", "if", "else", "match", "return", "use", "mod", "self", "Self", "where", "as", "const", "static", "ref", "move", "async", "await", "loop", "while", "break", "continue", "type", "dyn", "crate", "super", "unsafe", "true", "false"];
const KW_PY: &[&str] = &["def", "class", "return", "if", "elif", "else", "for", "while", "in", "import", "from", "as", "with", "try", "except", "finally", "raise", "pass", "yield", "lambda", "not", "and", "or", "is", "None", "True", "False", "self", "async", "await", "global", "nonlocal", "break", "continue", "assert", "del"];
const KW_JS: &[&str] = &[
    "abstract", "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "enum",
    "export", "extends", "false", "finally", "for", "from", "function", "get", "if", "implements", "import", "in", "instanceof",
    "interface", "let", "new", "null", "of", "private", "protected", "public", "readonly", "return", "set", "static", "super", "switch",
    "this", "throw", "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield",
];
const KW_GO: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if", "import",
    "interface", "map", "nil", "package", "range", "return", "select", "struct", "switch", "type", "var", "true", "false",
];
const KW_JAVA: &[&str] = &[
    "abstract", "boolean", "break", "case", "catch", "class", "continue", "default", "do", "double", "else", "enum", "extends", "false",
    "final", "finally", "float", "for", "if", "implements", "import", "instanceof", "int", "interface", "long", "new", "null", "package",
    "private", "protected", "public", "record", "return", "static", "super", "switch", "this", "throw", "throws", "true", "try", "var",
    "void", "while",
];
const KW_C: &[&str] = &["int", "char", "void", "return", "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "struct", "union", "enum", "typedef", "static", "const", "unsigned", "signed", "long", "short", "float", "double", "sizeof", "class", "public", "private", "protected", "virtual", "template", "typename", "namespace", "using", "new", "delete", "this", "auto", "bool", "true", "false", "nullptr", "override", "inline", "extern", "goto", "volatile", "explicit", "friend", "operator", "constexpr", "noexcept"];

/// Tiny line highlighter: comments, strings, numbers, keywords, types, calls.
fn highlight(line: &str, lang: Option<Lang>, st: &mut HlState) -> Vec<Span<'static>> {
    let comment = Style::new().fg(DIM);
    let string = Style::new().fg(Color::Green);
    let number = Style::new().fg(Color::Yellow);
    let keyword = Style::new().fg(Color::Magenta);
    let ty = Style::new().fg(ACCENT);
    let call = Style::new().fg(Color::Blue);
    let py = lang == Some(Lang::Python);
    let kws: &[&str] = match lang {
        Some(Lang::Rust) => KW_RUST,
        Some(Lang::Python) => KW_PY,
        Some(Lang::C | Lang::Cpp) => KW_C,
        Some(Lang::TypeScript | Lang::Tsx | Lang::JavaScript) => KW_JS,
        Some(Lang::Go) => KW_GO,
        Some(Lang::Java) => KW_JAVA,
        None => &[],
    };
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<Span<'static>>, s: String, style: Style| out.push(Span::styled(s, style));
    while i < chars.len() {
        if st.block_comment {
            let start = i;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            if i < chars.len() {
                i += 2;
                st.block_comment = false;
            }
            push(&mut out, chars[start..i.min(chars.len())].iter().collect(), comment);
            continue;
        }
        if let Some(q) = st.triple {
            let start = i;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == q && chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q) {
                    i += 3;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if closed {
                st.triple = None;
            }
            push(&mut out, chars[start..i].iter().collect(), string);
            continue;
        }
        let c = chars[i];
        let rest_starts = |p: &str| chars[i..].iter().take(p.len()).collect::<String>() == p;
        if (!py && rest_starts("//")) || (py && c == '#') {
            push(&mut out, chars[i..].iter().collect(), comment);
            break;
        }
        if !py && rest_starts("/*") {
            st.block_comment = true;
            continue;
        }
        if py && (rest_starts("\"\"\"") || rest_starts("'''")) {
            st.triple = Some(c);
            push(&mut out, chars[i..i + 3].iter().collect(), string);
            i += 3;
            continue;
        }
        if c == '"' || (py && c == '\'') || (lang.is_some_and(Lang::is_c_family) && c == '\'') {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            push(&mut out, chars[start..i].iter().collect(), string);
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            push(&mut out, chars[start..i].iter().collect(), number);
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            let style = if kws.contains(&w.as_str()) {
                keyword
            } else if w.chars().next().is_some_and(char::is_uppercase) {
                ty
            } else if chars.get(i) == Some(&'(') || (chars.get(i) == Some(&'!') && lang == Some(Lang::Rust)) {
                call
            } else {
                Style::new()
            };
            push(&mut out, w, style);
            continue;
        }
        let start = i;
        while i < chars.len() && !(chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '"' || chars[i] == '\'' || chars[i] == '/' || chars[i] == '#') {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        push(&mut out, chars[start..i].iter().collect(), Style::new());
    }
    out
}

/// Light styling for plain-text pages: first line bold, section headers accent, `≈` dim.
fn style_page(text: &str) -> Vec<Line<'static>> {
    let text = text.replace('`', "");
    text.lines()
        .enumerate()
        .map(|(n, l)| {
            if n == 0 {
                Line::from(Span::styled(l.to_string(), Style::new().bold()))
            } else if l.ends_with(':') && !l.starts_with(' ') {
                Line::from(Span::styled(l.to_string(), Style::new().fg(ACCENT).bold()))
            } else if let Some((a, b)) = l.split_once(" — ") {
                Line::from(vec![Span::raw(a.to_string()), Span::styled(format!(" — {b}"), Style::new().fg(DIM))])
            } else if let Some((label, rest)) = l.split_once(": ").filter(|(a, _)| !a.starts_with(' ') && a.len() < 16) {
                Line::from(vec![Span::styled(format!("{label}: "), Style::new().fg(DIM)), Span::raw(rest.to_string())])
            } else {
                Line::raw(l.to_string())
            }
        })
        .collect()
}

fn short_q(q: &str) -> String {
    let parts: Vec<&str> = q.split("::").collect();
    if parts.len() <= 3 { q.to_string() } else { format!("…::{}", parts[parts.len() - 3..].join("::")) }
}

fn draw_help(f: &mut Frame) {
    let area = popup(f.area(), 60, 86);
    f.render_widget(Clear, area);
    let rows = [
        ("tab / shift-tab", "switch pane"),
        ("j k ↑ ↓", "move"),
        ("l → enter", "open dir / go into pane"),
        ("h ← ", "collapse / go back"),
        ("g G", "top / bottom"),
        ("/", "search symbols and files"),
        ("ctrl-p  :", "command palette (everything)"),
        ("s", "changes: stage files or single functions"),
        ("enter (changes)", "before / after side by side"),
        ("c", "commit (message suggested from changes)"),
        ("U", "undo last commit (changes stay staged)"),
        ("H", "time machine: earlier points of the branch"),
        ("B", "branches: ahead / behind, what they change"),
        ("d", "semantic diff"),
        ("enter (detail)", "jump to caller / callee / subclass"),
        ("o", "overview of the repository"),
        ("a", "agent journal (what the agent did)"),
        ("b", "back to previous symbol"),
        ("r", "reindex"),
        ("m", "regenerate .gitnext/map"),
        ("q", "quit"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| Line::from(vec![Span::styled(format!("  {k:<18}"), Style::new().fg(ACCENT)), Span::raw(*v)]))
        .collect();
    f.render_widget(Paragraph::new(lines).block(pane_block(" keys ", true)), area);
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let color = if focused { ACCENT } else { DIM };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .title(Span::styled(title.to_string(), Style::new().fg(if focused { ACCENT } else { Color::Gray })))
}

fn hl(focused: bool) -> Style {
    if focused {
        Style::new().bg(ACCENT).fg(Color::Black).add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::REVERSED)
    }
}

/// Wrap lines to `width` columns. A continuation keeps the line's indent (its
/// leading spaces, a line-number gutter and the code's own indentation) and
/// starts with `↪`, so wrapped signatures and code stay readable. Returns the
/// lines and, for each input line, the index of its first output row.
fn wrap_hanging(lines: Vec<Line<'static>>, width: usize) -> (Vec<Line<'static>>, Vec<usize>) {
    use unicode_width::UnicodeWidthChar;
    let mut out = Vec::with_capacity(lines.len());
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        rows.push(out.len());
        let total: usize = line.spans.iter().map(|s| s.width()).sum();
        if width < 12 || total <= width {
            out.push(line);
            continue;
        }
        let line_style = line.style;
        // (char, style) cells
        let cells: Vec<(char, Style)> = line.spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style))).collect();
        // hanging indent: leading spaces, an optional line-number gutter, then code indentation
        let text: String = cells.iter().map(|c| c.0).collect();
        let mut indent = text.len() - text.trim_start().len();
        let rest = &text[indent..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && rest[digits..].starts_with(' ') {
            indent += digits;
            indent += text[indent..].len() - text[indent..].trim_start().len();
        }
        let indent = indent.min(width / 2);
        let mut start = 0;
        let mut first = true;
        while start < cells.len() {
            let avail = if first { width } else { width.saturating_sub(indent + 2) };
            // take cells up to `avail` columns
            let (mut end, mut w) = (start, 0);
            while end < cells.len() {
                let cw = cells[end].0.width().unwrap_or(0);
                if w + cw > avail {
                    break;
                }
                w += cw;
                end += 1;
            }
            // break after the last space in the chunk when the line continues
            if end < cells.len() {
                if let Some(sp) = (start + 1..end).rev().find(|&k| cells[k].0 == ' ') {
                    if sp > start + avail / 3 {
                        end = sp + 1;
                    }
                }
            }
            if end == start {
                end = start + 1;
            }
            let mut spans: Vec<Span<'static>> = Vec::new();
            if !first {
                spans.push(Span::raw(" ".repeat(indent)));
                spans.push(Span::styled("↪ ", Style::new().fg(DIM)));
            }
            let mut buf = String::new();
            let mut cur = cells[start].1;
            let skip_lead = !first;
            for (k, &(c, st)) in cells[start..end].iter().enumerate() {
                if skip_lead && k == 0 && c == ' ' {
                    continue;
                }
                if st != cur && !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), cur));
                }
                cur = st;
                buf.push(c);
            }
            if !buf.is_empty() {
                spans.push(Span::styled(buf, cur));
            }
            out.push(Line::from(spans).style(line_style));
            start = end;
            first = false;
        }
    }
    (out, rows)
}

/// Everything between the top bar and the key hints: for screens that replace the panes.
fn screen_area(area: Rect) -> Rect {
    Rect { x: area.x, y: area.y + 1, width: area.width, height: area.height.saturating_sub(2) }
}

fn popup(area: Rect, pw: u16, ph: u16) -> Rect {
    let w = area.width * pw / 100;
    let h = area.height * ph / 100;
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w.max(20), height: h.max(5) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Renders the UI against this very repository and checks the panes appear.
    #[test]
    fn renders_three_panes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let ws = Workspace::open(&root).unwrap();
        let mut app = App::new(&ws).unwrap();
        // the first-run welcome page covers the panes: close it like a user would
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = format!("{:?}", term.backend().buffer());
        assert!(buf.contains("files"));
        assert!(buf.contains("symbols"));
        assert!(buf.contains("gitnext"));
    }
}

#[cfg(test)]
mod snapshot {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;

    fn dump(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn wrapped_lines_keep_their_indent() {
        let lines = vec![
            Line::from("  takes    path  impl AsRef<Path> and some more words that do not fit"),
            Line::from("short"),
            Line::from("  19      pub fn open(path: impl AsRef<Path>) -> Result<Self> {"),
        ];
        let (out, rows) = wrap_hanging(lines, 30);
        let text: Vec<String> = out.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert!(text.len() > 3, "{text:?}");
        assert!(text.iter().all(|t| unicode_width::UnicodeWidthStr::width(t.as_str()) <= 30), "{text:?}");
        // continuation of the first line: its 2-space indent, then the mark
        assert!(text[1].starts_with("  ↪ "), "{text:?}");
        // the code line continues under its code, past the line-number gutter
        let code = rows[2];
        assert!(text[code + 1].starts_with(&format!("{}↪ ", " ".repeat(10))), "{text:?}");
        assert_eq!(text[rows[1]], "short");
    }

    #[test]
    #[ignore]
    fn print_screens() {
        let root = std::env::var("ROOT").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let ws = Workspace::open(&root).unwrap();
        let mut app = App::new(&ws).unwrap();
        let size = |k: &str, d: u16| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let mut term = Terminal::new(TestBackend::new(size("COLS", 130), size("ROWS", 34))).unwrap();
        let keys = std::env::var("KEYS").unwrap_or_default();
        for c in keys.chars() {
            let code = match c {
                'T' => KeyCode::Tab,
                'E' => KeyCode::Enter,
                'X' => KeyCode::Esc,
                'B' => KeyCode::Backspace,
                c => KeyCode::Char(c),
            };
            app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        term.draw(|f| app.draw(f)).unwrap();
        println!("{}", dump(&term));
        if let Ok(out) = std::env::var("SVG") {
            std::fs::write(out, svg(&term)).unwrap();
        }
    }

    /// The screen as an SVG "screenshot" (for the README): one text run per
    /// same-styled stretch, background rects, a terminal-like frame.
    fn svg(term: &Terminal<TestBackend>) -> String {
        use ratatui::style::{Color, Modifier};
        let buf = term.backend().buffer();
        let (cw, ch) = (8.4_f64, 18.0_f64);
        let (pad, bar) = (14.0, 30.0);
        let w = buf.area.width as f64 * cw + 2.0 * pad;
        let h = buf.area.height as f64 * ch + 2.0 * pad + bar;
        let bg = "#16191f";
        let fg_default = "#d7dae0";
        let col = |c: Color, fallback: &str| -> String {
            match c {
                Color::Reset => fallback.into(),
                Color::Black => "#16191f".into(),
                Color::Red => "#ef6b73".into(),
                Color::Green => "#8bd17c".into(),
                Color::Yellow => "#e8c36a".into(),
                Color::Blue => "#6aa8f0".into(),
                Color::Magenta => "#c792ea".into(),
                Color::Cyan => "#5fd3d3".into(),
                Color::Gray => "#a9b1bd".into(),
                Color::DarkGray => "#6b7380".into(),
                Color::White => "#f2f4f7".into(),
                Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                _ => fallback.into(),
            }
        };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"ui-monospace, SFMono-Regular, Menlo, Consolas, monospace\" font-size=\"13.5\">\n\
             <rect width=\"{w}\" height=\"{h}\" rx=\"10\" fill=\"{bg}\"/>\n\
             <circle cx=\"22\" cy=\"15\" r=\"5.5\" fill=\"#ff5f57\"/><circle cx=\"40\" cy=\"15\" r=\"5.5\" fill=\"#febc2e\"/><circle cx=\"58\" cy=\"15\" r=\"5.5\" fill=\"#28c840\"/>\n"
        );
        for y in 0..buf.area.height {
            let ty = pad + bar + y as f64 * ch;
            let mut x = 0;
            while x < buf.area.width {
                let c0 = &buf[(x, y)];
                let (f0, b0, m0) = (c0.fg, c0.bg, c0.modifier);
                let mut text = String::new();
                let start = x;
                while x < buf.area.width {
                    let c = &buf[(x, y)];
                    if c.fg != f0 || c.bg != b0 || c.modifier != m0 {
                        break;
                    }
                    text.push_str(c.symbol());
                    x += 1;
                }
                let n = (x - start) as f64;
                let px = pad + start as f64 * cw;
                let reversed = m0.contains(Modifier::REVERSED);
                let (mut fgc, mut bgc) = (col(f0, fg_default), if b0 == Color::Reset { String::new() } else { col(b0, bg) });
                if reversed {
                    (fgc, bgc) = (if b0 == Color::Reset { bg.into() } else { col(b0, bg) }, col(f0, fg_default));
                }
                if !bgc.is_empty() {
                    out.push_str(&format!("<rect x=\"{px:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{ch}\" fill=\"{bgc}\"/>", ty, n * cw));
                }
                if text.trim().is_empty() {
                    continue;
                }
                let weight = if m0.contains(Modifier::BOLD) { " font-weight=\"700\"" } else { "" };
                let italic = if m0.contains(Modifier::ITALIC) { " font-style=\"italic\"" } else { "" };
                out.push_str(&format!(
                    "<text x=\"{px:.1}\" y=\"{:.1}\" fill=\"{fgc}\"{weight}{italic} textLength=\"{:.1}\" lengthAdjust=\"spacingAndGlyphs\" xml:space=\"preserve\">{}</text>\n",
                    ty + ch * 0.74,
                    n * cw,
                    esc(&text)
                ));
            }
        }
        out.push_str("</svg>\n");
        out
    }
}
