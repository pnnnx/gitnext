//! The human side of git in the TUI: changes in words and functions, staging a
//! single function, commit with a drafted message, side-by-side diffs, branch
//! cards, undo / time machine, and the command palette.

use super::*;
use gitnext_ops::changes::{self as ch, ChangesView, DiffRow, GroupKind, RowKind};
use gitnext_ops::gitnext_core::ReflogEntry;
use gitnext_ops::gitnext_index::semdiff::SymChange;

/// One line of the Changes screen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CRow {
    Group(usize),
    File(usize, usize),
    Sym(usize, usize, usize),
}

#[derive(Clone)]
pub(crate) enum Action {
    UndoCommit,
    RestoreTo { id: String, label: String },
}

/// Command palette entries: (label, key hint, what it does).
const COMMANDS: &[(&str, &str, Cmd)] = &[
    ("Changes: what changed, stage, commit", "s", Cmd::Changes),
    ("Commit", "c", Cmd::Commit),
    ("Semantic diff of everything", "d", Cmd::Diff),
    ("Branches", "B", Cmd::Branches),
    ("Undo last commit (keeps the changes)", "U", Cmd::Undo),
    ("Time machine: where HEAD has been", "H", Cmd::Timeline),
    ("Overview of this repository", "o", Cmd::Overview),
    ("Agent journal: what the agent did", "a", Cmd::Journal),
    ("Search symbols and files", "/", Cmd::Search),
    ("Reindex", "r", Cmd::Reindex),
    ("Regenerate the code map (.gitnext/map)", "m", Cmd::Map),
    ("Welcome / how this works", "", Cmd::Welcome),
    ("Keys", "?", Cmd::Help),
    ("Quit", "q", Cmd::Quit),
];

#[derive(Clone, Copy)]
pub(crate) enum Cmd {
    Changes,
    Commit,
    Diff,
    Branches,
    Undo,
    Timeline,
    Overview,
    Journal,
    Search,
    Reindex,
    Map,
    Welcome,
    Help,
    Quit,
}

fn fuzzy(hay: &str, q: &str) -> bool {
    let hay = hay.to_lowercase();
    let mut it = hay.chars();
    q.to_lowercase().chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|h| h == c))
}

pub(crate) fn palette_items(q: &str) -> Vec<usize> {
    (0..COMMANDS.len()).filter(|&i| q.is_empty() || fuzzy(COMMANDS[i].0, q)).collect()
}

impl App<'_> {
    pub(crate) fn run_cmd(&mut self, c: Cmd) {
        self.mode = Mode::Normal;
        match c {
            Cmd::Changes => self.open_changes(),
            Cmd::Commit => self.open_commit(),
            Cmd::Diff => self.open_diff(),
            Cmd::Branches => self.open_branches(),
            Cmd::Undo => self.ask_undo(),
            Cmd::Timeline => self.open_timeline(),
            Cmd::Overview => self.open_overview(),
            Cmd::Journal => self.open_journal(),
            Cmd::Search => self.mode = Mode::Search { query: String::new(), results: vec![], sel: 0 },
            Cmd::Reindex => {
                self.reload_graph();
                self.reload_status();
                self.flash(format!("reindexed · {}", self.g.counts().line()));
            }
            Cmd::Map => match self.ws.write_map(&self.g) {
                Ok(o) => self.flash(format!("map: {} written, {} unchanged", o.written.len(), o.unchanged)),
                Err(e) => self.error(format!("map: {e:#}")),
            },
            Cmd::Welcome => self.open_welcome(),
            Cmd::Help => self.mode = Mode::Help,
            Cmd::Quit => self.quit = true,
        }
    }

    pub(crate) fn key_palette(&mut self, k: KeyEvent, mut query: String, mut sel: usize) -> Mode {
        let items = palette_items(&query);
        match k.code {
            KeyCode::Esc => return Mode::Normal,
            KeyCode::Enter => {
                if let Some(&i) = items.get(sel) {
                    self.run_cmd(COMMANDS[i].2);
                    return std::mem::replace(&mut self.mode, Mode::Normal);
                }
            }
            KeyCode::Down | KeyCode::Tab => sel = (sel + 1).min(items.len().saturating_sub(1)),
            KeyCode::Up | KeyCode::BackTab => sel = sel.saturating_sub(1),
            KeyCode::Char('n') if k.modifiers.contains(KeyModifiers::CONTROL) => sel = (sel + 1).min(items.len().saturating_sub(1)),
            KeyCode::Char('p') if k.modifiers.contains(KeyModifiers::CONTROL) => sel = sel.saturating_sub(1),
            KeyCode::Backspace => {
                query.pop();
                sel = 0;
            }
            KeyCode::Char(c) => {
                query.push(c);
                sel = 0;
            }
            _ => {}
        }
        Mode::Palette { query, sel }
    }

    pub(crate) fn draw_palette(&self, f: &mut Frame, query: &str, sel: usize) {
        let area = popup(f.area(), 56, 60);
        f.render_widget(Clear, area);
        let [input, list] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(area);
        f.render_widget(
            Paragraph::new(Line::from(vec!["› ".fg(ACCENT), Span::raw(query.to_string()), "▏".fg(ACCENT)])).block(pane_block(" what do you want to do? ", true)),
            input,
        );
        let items = palette_items(query);
        let rows: Vec<ListItem> = items
            .iter()
            .map(|&i| {
                let (label, key, _) = COMMANDS[i];
                ListItem::new(Line::from(vec![Span::raw(format!(" {label}")), Span::styled(if key.is_empty() { String::new() } else { format!("   {key}") }, Style::new().fg(DIM))]))
            })
            .collect();
        let mut st = ListState::default().with_selected(if items.is_empty() { None } else { Some(sel) });
        f.render_stateful_widget(List::new(rows).block(pane_block("", true)).highlight_style(hl(true)), list, &mut st);
    }

    // ───────────────────────────── changes ─────────────────────────────

    pub(crate) fn open_changes(&mut self) {
        let Some(repo) = &self.ws.repo else { return self.error("This folder isn't a git repository yet. Run `gitnext init` to start one.") };
        match ch::changes(repo) {
            Ok(cv) => {
                self.crows = rows_of(&cv);
                self.cv = Some(cv);
                let first = self.crows.iter().position(|r| !matches!(r, CRow::Group(_))).unwrap_or(0);
                self.mode = Mode::Changes { sel: first };
            }
            Err(e) => self.error(ch::humanize(&e)),
        }
    }

    fn refresh_changes(&mut self, sel: usize) -> Mode {
        self.reload_status();
        if let Some(repo) = &self.ws.repo {
            match ch::changes(repo) {
                Ok(cv) => {
                    self.crows = rows_of(&cv);
                    self.cv = Some(cv);
                }
                Err(e) => self.error(ch::humanize(&e)),
            }
        }
        Mode::Changes { sel: sel.min(self.crows.len().saturating_sub(1)) }
    }

    fn change_at(&self, r: CRow) -> Option<(GroupKind, &ch::FileEntry, Option<&SymChange>)> {
        let cv = self.cv.as_ref()?;
        match r {
            CRow::Group(_) => None,
            CRow::File(g, f) => Some((cv.groups[g].kind, &cv.groups[g].files[f], None)),
            CRow::Sym(g, f, c) => Some((cv.groups[g].kind, &cv.groups[g].files[f], Some(&cv.groups[g].files[f].changes[c]))),
        }
    }

    pub(crate) fn key_changes(&mut self, k: KeyEvent, mut sel: usize) -> Mode {
        let Some(repo) = &self.ws.repo else { return Mode::Normal };
        let n = self.crows.len();
        let step = |sel: usize, d: i32, rows: &[CRow]| -> usize {
            let mut s = sel as i32;
            loop {
                s += d;
                if s < 0 || s as usize >= rows.len() {
                    return sel;
                }
                if !matches!(rows[s as usize], CRow::Group(_)) {
                    return s as usize;
                }
            }
        };
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => return Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => sel = step(sel, 1, &self.crows),
            KeyCode::Up | KeyCode::Char('k') => sel = step(sel, -1, &self.crows),
            KeyCode::Char('J') | KeyCode::PageDown => {
                // next file
                let mut s = sel;
                while s + 1 < n {
                    s += 1;
                    if matches!(self.crows[s], CRow::File(..)) {
                        break;
                    }
                }
                sel = s;
            }
            KeyCode::Char(' ') => {
                let Some(row) = self.crows.get(sel).copied() else { return Mode::Changes { sel } };
                let Some((group, file, sym)) = self.change_at(row) else { return Mode::Changes { sel } };
                let unstage = group == GroupKind::Staged;
                let path = file.path.clone();
                let res = match sym {
                    Some(c) if file.what == "modified" => {
                        let label = ch::short(c);
                        ch::stage_symbol(repo, &path, &c.clone(), unstage).map(|_| format!("{} {label}", if unstage { "unstaged" } else { "staged" }))
                    }
                    _ => {
                        let r = if unstage { repo.unstage(std::slice::from_ref(&path)) } else { repo.add(std::slice::from_ref(&path)) };
                        r.map(|_| format!("{} {path}", if unstage { "unstaged" } else { "staged" }))
                    }
                };
                match res {
                    Ok(m) => self.flash(m),
                    Err(e) => self.error(ch::humanize(&e)),
                }
                return self.refresh_changes(sel);
            }
            KeyCode::Char('a') => {
                if let Err(e) = repo.add(&[".".into()]) {
                    self.error(ch::humanize(&e));
                } else {
                    self.flash("staged everything");
                }
                return self.refresh_changes(sel);
            }
            KeyCode::Char('u') => {
                let staged: Vec<String> = self.status.iter().filter(|e| e.staged.is_some()).map(|e| e.path.clone()).collect();
                if !staged.is_empty() {
                    if let Err(e) = repo.unstage(&staged) {
                        self.error(ch::humanize(&e));
                    } else {
                        self.flash("unstaged everything");
                    }
                }
                return self.refresh_changes(sel);
            }
            KeyCode::Enter => {
                if let Some(row) = self.crows.get(sel).copied() {
                    self.open_split(row);
                    if matches!(self.mode, Mode::Split { .. }) {
                        let m = std::mem::replace(&mut self.mode, Mode::Normal);
                        if let Mode::Split { title, rows, scroll, .. } = m {
                            return Mode::Split { title, rows, scroll, back: sel };
                        }
                    }
                }
            }
            KeyCode::Char('c') => {
                self.open_commit();
                return std::mem::replace(&mut self.mode, Mode::Normal);
            }
            KeyCode::Char('d') => {
                self.open_diff();
                return std::mem::replace(&mut self.mode, Mode::Normal);
            }
            KeyCode::Char('U') => {
                self.ask_undo();
                return std::mem::replace(&mut self.mode, Mode::Normal);
            }
            _ => {}
        }
        Mode::Changes { sel }
    }

    pub(crate) fn draw_changes(&self, f: &mut Frame, sel: usize) {
        let area = popup(f.area(), 90, 90);
        f.render_widget(Clear, area);
        let Some(cv) = &self.cv else { return };
        let [head, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }));
        f.render_widget(pane_block(&format!(" changes · {} ", cv.branch), true), area);
        let mut head_lines = vec![Line::from(Span::styled(cv.summary.clone(), Style::new().bold()))];
        if !cv.draft.is_empty() {
            head_lines.push(Line::from(vec!["suggested message: ".fg(DIM), Span::raw(cv.draft.clone())]));
        }
        f.render_widget(Paragraph::new(head_lines), head);
        let items: Vec<ListItem> = self.crows.iter().map(|&r| ListItem::new(self.crow_line(cv, r))).collect();
        let mut st = ListState::default().with_selected(if items.is_empty() { None } else { Some(sel) });
        let list = if items.is_empty() {
            List::new(vec![ListItem::new(Line::from("Nothing to show — the working tree matches the last commit.".fg(DIM)))])
        } else {
            List::new(items)
        };
        f.render_stateful_widget(list.highlight_style(hl(true)), body, &mut st);
    }

    fn crow_line(&self, cv: &ChangesView, r: CRow) -> Line<'static> {
        match r {
            CRow::Group(g) => {
                let gr = &cv.groups[g];
                let color = match gr.kind {
                    GroupKind::Conflicts => Color::Red,
                    GroupKind::Staged => Color::Green,
                    GroupKind::Changed => Color::Yellow,
                    GroupKind::New => ACCENT,
                };
                Line::from(vec![
                    Span::styled(format!("{} ({})", gr.title, gr.files.len()), Style::new().fg(color).bold()),
                    Span::styled(format!("  {}", gr.hint), Style::new().fg(DIM)),
                ])
            }
            CRow::File(g, fi) => {
                let file = &cv.groups[g].files[fi];
                let n = file.changes.iter().filter(|c| !c.test).count();
                let detail = if !file.semantic {
                    String::new()
                } else if n == 0 && file.changes.is_empty() {
                    if file.what == "modified" { "  comments / formatting only".into() } else { String::new() }
                } else {
                    format!("  {}", ch::plural(n.max(file.changes.len()), "symbol"))
                };
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{:<9}", file.what), Style::new().fg(DIM)),
                    Span::raw(file.path.clone()).bold(),
                    Span::styled(detail, Style::new().fg(DIM)),
                ])
            }
            CRow::Sym(g, fi, ci) => {
                let c = &cv.groups[g].files[fi].changes[ci];
                let (mark, color) = match c.change {
                    gitnext_ops::gitnext_index::semdiff::Change::Added => ("+", Color::Green),
                    gitnext_ops::gitnext_index::semdiff::Change::Removed => ("-", Color::Red),
                    gitnext_ops::gitnext_index::semdiff::Change::Signature => ("~", Color::Yellow),
                    _ => ("•", ACCENT),
                };
                let mut spans = vec![
                    Span::raw("      "),
                    Span::styled(format!("{mark} "), Style::new().fg(color).bold()),
                    Span::styled(format!("{} ", c.kind.as_str()), Style::new().fg(DIM)),
                    Span::raw(ch::short(c)),
                    Span::styled(format!("  {}", ch::verb(c)), Style::new().fg(DIM)),
                ];
                if c.test {
                    spans.push(Span::styled(" (test)", Style::new().fg(DIM)));
                }
                if let (Some(o), Some(n)) = (&c.old_signature, &c.new_signature) {
                    if o != n && c.change == gitnext_ops::gitnext_index::semdiff::Change::Signature {
                        spans.push(Span::styled(format!("   {} → {}", one_line(o, 40), one_line(n, 40)), Style::new().fg(DIM)));
                    }
                }
                Line::from(spans)
            }
        }
    }

    // ───────────────────────────── side by side ─────────────────────────────

    fn open_split(&mut self, row: CRow) {
        let Some(repo) = &self.ws.repo else { return };
        let Some((group, file, sym)) = self.change_at(row) else { return };
        let path = file.path.clone();
        let (old, new) = match group {
            GroupKind::Staged => (repo.read_head(&path), repo.read_index(&path)),
            GroupKind::New => (None, repo.read_worktree(&path)),
            _ => (repo.read_index(&path).or_else(|| repo.read_head(&path)), repo.read_worktree(&path)),
        };
        let title = match sym {
            Some(c) => format!(" {} · {} · {} ", ch::short(c), ch::verb(c), path),
            None => format!(" {path} · {} ", file.what),
        };
        let rows = ch::symbol_diff(old.as_deref(), new.as_deref(), sym);
        self.mode = Mode::Split { title, rows, scroll: 0, back: 0 };
    }

    pub(crate) fn draw_split(&self, f: &mut Frame, title: &str, rows: &[DiffRow], scroll: u16) {
        let area = popup(f.area(), 96, 92);
        f.render_widget(Clear, area);
        f.render_widget(pane_block(&format!("{title}· esc back "), true), area);
        let inner = area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let [hl_, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        let [l, r] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
        let [lh, rh] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(hl_);
        f.render_widget(Line::from(" before".fg(DIM)), lh);
        f.render_widget(Line::from(" after".fg(DIM)), rh);
        let lang = self.cv.as_ref().and(None::<Lang>);
        let _ = lang;
        let side = |old: bool| -> Vec<Line<'static>> {
            rows.iter()
                .map(|row| {
                    let cell = if old { &row.old } else { &row.new };
                    let (bg, sign) = match (row.kind, old) {
                        (RowKind::Same, _) => (None, " "),
                        (RowKind::Changed, true) | (RowKind::Removed, true) => (Some(Color::Rgb(70, 20, 20)), "-"),
                        (RowKind::Changed, false) | (RowKind::Added, false) => (Some(Color::Rgb(20, 60, 25)), "+"),
                        _ => (None, " "),
                    };
                    match cell {
                        Some((n, text)) => {
                            let mut st = Style::new();
                            if let Some(b) = bg {
                                st = st.bg(b);
                            }
                            Line::from(vec![Span::styled(format!("{n:>4} "), Style::new().fg(DIM)), Span::styled(format!("{sign} {}", text.replace('\t', "    ")), st)])
                        }
                        None => Line::from(Span::styled("     ·", Style::new().fg(DIM))),
                    }
                })
                .collect()
        };
        if rows.is_empty() {
            f.render_widget(Paragraph::new("no line changes (whitespace or the symbol moved)".fg(DIM)), body);
            return;
        }
        f.render_widget(Paragraph::new(side(true)).scroll((scroll, 0)), l);
        f.render_widget(Paragraph::new(side(false)).scroll((scroll, 0)), r);
    }

    // ───────────────────────────── commit / undo / history ─────────────────────────────

    pub(crate) fn commit_draft(&self) -> String {
        self.ws.repo.as_ref().and_then(|r| ch::changes(r).ok()).map(|cv| cv.draft).unwrap_or_default()
    }

    pub(crate) fn ask_undo(&mut self) {
        let Some(repo) = &self.ws.repo else { return self.error("not a git repository") };
        match repo.log(1) {
            Ok(v) if !v.is_empty() => {
                let c = &v[0];
                self.mode = Mode::Confirm {
                    question: format!("Undo the last commit “{}”? Its changes stay staged, nothing is lost.", c.summary),
                    action: Action::UndoCommit,
                };
            }
            _ => self.flash("there are no commits to undo"),
        }
    }

    pub(crate) fn key_confirm(&mut self, k: KeyEvent, question: String, action: Action) -> Mode {
        match k.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let Some(repo) = &self.ws.repo else { return Mode::Normal };
                let r = match &action {
                    Action::UndoCommit => repo.undo_commit().map(|c| format!("undid “{}” — its changes are staged; edit and commit again, or H to go back", c.summary)),
                    Action::RestoreTo { id, label } => repo.reset_soft(id).map(|_| format!("moved back to “{label}” — your files are unchanged, differences are staged")),
                };
                match r {
                    Ok(m) => self.flash(m),
                    Err(e) => self.error(ch::humanize(&e)),
                }
                self.reload_graph();
                self.reload_status();
                Mode::Normal
            }
            KeyCode::Char('n') | KeyCode::Esc => Mode::Normal,
            _ => Mode::Confirm { question, action },
        }
    }

    pub(crate) fn draw_confirm(&self, f: &mut Frame, q: &str) {
        let area = popup(f.area(), 56, 20);
        let area = Rect { height: area.height.clamp(6, 8), ..area };
        f.render_widget(Clear, area);
        let lines = vec![
            Line::raw(""),
            Line::from(Span::raw(format!(" {q}"))),
            Line::raw(""),
            Line::from(vec![Span::styled(" y", Style::new().fg(ACCENT)), " yes   ".fg(DIM), Span::styled("n", Style::new().fg(ACCENT)), " no".fg(DIM)]),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane_block(" are you sure? ", true)), area);
    }

    pub(crate) fn open_timeline(&mut self) {
        let Some(repo) = &self.ws.repo else { return self.error("not a git repository") };
        match repo.reflog(40) {
            Ok(v) if !v.is_empty() => self.mode = Mode::Timeline { entries: v, sel: 0 },
            Ok(_) => self.flash("no history yet"),
            Err(e) => self.error(ch::humanize(&e)),
        }
    }

    pub(crate) fn key_timeline(&mut self, k: KeyEvent, entries: Vec<ReflogEntry>, mut sel: usize) -> Mode {
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('H') => return Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(entries.len().saturating_sub(1)),
            KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
            KeyCode::Enter if sel > 0 => {
                let e = &entries[sel];
                let label = human_reflog(&e.message);
                return Mode::Confirm {
                    question: format!("Move the branch back to “{label}” ({})? Files on disk stay as they are; the difference shows up as staged changes.", e.short),
                    action: Action::RestoreTo { id: e.id.clone(), label },
                };
            }
            _ => {}
        }
        Mode::Timeline { entries, sel }
    }

    pub(crate) fn draw_timeline(&self, f: &mut Frame, entries: &[ReflogEntry], sel: usize) {
        let area = popup(f.area(), 80, 80);
        f.render_widget(Clear, area);
        let now = now_secs();
        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let when = gitnext_ops::agents::human_ago(now - e.time);
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:>9} ago  ", when), Style::new().fg(DIM)),
                    Span::styled(format!("{}  ", e.short), Style::new().fg(Color::Yellow)),
                    Span::raw(human_reflog(&e.message)),
                    Span::styled(if i == 0 { "   ← you are here".to_string() } else { String::new() }, Style::new().fg(ACCENT)),
                ]))
            })
            .collect();
        let mut st = ListState::default().with_selected(Some(sel));
        f.render_stateful_widget(
            List::new(items).block(pane_block(" time machine · enter: go back to that point · esc ", true)).highlight_style(hl(true)),
            area,
            &mut st,
        );
    }

    pub(crate) fn open_branches(&mut self) {
        let Some(repo) = &self.ws.repo else { return self.error("not a git repository") };
        let branches = match repo.branches() {
            Ok(b) => b,
            Err(e) => return self.error(ch::humanize(&e)),
        };
        let mut lines: Vec<Line<'static>> = Vec::new();
        if branches.is_empty() {
            lines.push(Line::from("No branches yet — make the first commit.".fg(DIM)));
        }
        let now = now_secs();
        for b in branches.iter().take(12) {
            let mark = if b.current { "● " } else { "  " };
            lines.push(Line::from(vec![Span::styled(mark, Style::new().fg(ACCENT)), Span::styled(b.name.clone(), Style::new().bold()), Span::styled(if b.current { "  (you are here)".to_string() } else { String::new() }, Style::new().fg(DIM))]));
            let rel = if b.ahead == 0 && b.behind == 0 {
                if b.compared_to.is_empty() || (b.current && b.upstream.is_none()) { "no upstream".to_string() } else { format!("in sync with {}", b.compared_to) }
            } else {
                match (b.ahead, b.behind) {
                    (a, 0) => format!("{} ahead of {} — not merged yet", ch::plural(a, "commit"), b.compared_to),
                    (0, h) => format!("{} behind {} — older than it", ch::plural(h, "commit"), b.compared_to),
                    (a, h) => format!("{} ahead and {h} behind {} — diverged", ch::plural(a, "commit"), b.compared_to),
                }
            };
            lines.push(Line::from(vec![Span::raw("    "), Span::styled(rel, Style::new().fg(if b.behind > 0 { Color::Yellow } else { DIM }))]));
            lines.push(Line::from(vec![
                Span::raw("    last: "),
                Span::raw(b.last_summary.clone()),
                Span::styled(format!(" · {} · {} ago", b.last_author, gitnext_ops::agents::human_ago(now - b.last_time)), Style::new().fg(DIM)),
            ]));
            // what the branch brings, in symbols (only for branches ahead of the current one)
            if !b.current && b.ahead > 0 && b.upstream.is_none() {
                let range = format!("{}..{}", b.compared_to, b.name);
                if let Ok(cl) = gitnext_ops::history::changelog(repo, &self.g, &range) {
                    let text = gitnext_ops::history::render_changelog(&cl);
                    for l in text.lines().skip(1).take_while(|l| !l.starts_with("commits")).filter(|l| !l.trim().is_empty()).take(5) {
                        lines.push(Line::from(Span::styled(format!("    {}", one_line(l.trim(), 100)), Style::new().fg(DIM))));
                    }
                }
            }
            lines.push(Line::raw(""));
        }
        self.mode = Mode::Page { title: " branches ".into(), lines, scroll: 0 };
    }

    pub(crate) fn open_welcome(&mut self) {
        let text = [
            "Welcome to gitnext",
            "",
            "This is your repository seen as code, not as files.",
            "",
            "Left to right: files · the functions and types in the selected file · details of the selected symbol",
            "(what it does, who calls it, what it calls). tab moves between panes, enter follows a link, b goes back.",
            "",
            "Git, in plain words:",
            "  s   Changes — what you changed, function by function. space stages a file or a single function.",
            "  c   Commit — a message is suggested from your changes; edit it or press enter.",
            "  U   Undo the last commit (your changes stay, nothing is lost).",
            "  H   Time machine — every earlier point of the branch you can return to.",
            "  B   Branches — how far ahead / behind each one is and what it changes.",
            "",
            "For AI agents:",
            "  a   what the agent read and edited (through `gitnext mcp`)",
            "  o   overview of the repository",
            "",
            "ctrl-p opens a list of everything. ? shows all keys. Any key closes this page.",
        ]
        .join("\n");
        self.mode = Mode::Page { title: " welcome ".into(), lines: style_page(&text), scroll: 0 };
    }
}

fn rows_of(cv: &ChangesView) -> Vec<CRow> {
    let mut v = Vec::new();
    for (g, gr) in cv.groups.iter().enumerate() {
        v.push(CRow::Group(g));
        for (f, file) in gr.files.iter().enumerate() {
            v.push(CRow::File(g, f));
            for c in 0..file.changes.len().min(30) {
                v.push(CRow::Sym(g, f, c));
            }
        }
    }
    v
}

fn one_line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max {
        s.chars().take(max - 1).collect::<String>() + "…"
    } else {
        s
    }
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub(crate) use gitnext_ops::changes::human_reflog;
