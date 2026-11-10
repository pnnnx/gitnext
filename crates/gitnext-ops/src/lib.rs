//! High-level workflows shared by the CLI and the TUI: open a workspace,
//! refresh the index, regenerate the map, semantic diff, smart commit, context packs.

use anyhow::{bail, Result};
use gitnext_core::Repo;
use gitnext_docs::MapOutput;
use gitnext_index::semdiff::{diff_sources_in, FileChange};
use gitnext_index::{Graph, Index, RefreshStats, GITNEXT_DIR};
use gitnext_parse::Kind;
use serde::Serialize;
pub mod agents;
pub mod bench;
pub mod changes;
pub mod edit;
pub mod explain;
pub mod history;
pub mod insight;
pub mod journal;
pub mod locate;
pub mod look;
pub mod lsp;
pub mod render;

use std::path::{Path, PathBuf};

pub use gitnext_core;
pub use gitnext_docs;
pub use gitnext_index;
pub use gitnext_parse;

pub struct Workspace {
    pub root: PathBuf,
    pub repo: Option<Repo>,
}

impl Workspace {
    /// Discover the git repository around `cwd`; fall back to `cwd` itself
    /// so read-only commands (map, symbols…) work in any folder.
    pub fn open(cwd: &Path) -> Result<Self> {
        match Repo::discover(cwd) {
            Ok(repo) => Ok(Workspace { root: repo.root().to_path_buf(), repo: Some(repo) }),
            Err(_) => Ok(Workspace { root: cwd.canonicalize()?, repo: None }),
        }
    }

    pub fn repo(&self) -> Result<&Repo> {
        self.repo.as_ref().ok_or_else(|| anyhow::anyhow!("not a git repository (run `gitnext init`)"))
    }

    pub fn index(&self) -> Result<(Index, RefreshStats)> {
        let mut idx = Index::open(&self.root)?;
        let stats = idx.refresh()?;
        Ok((idx, stats))
    }

    /// Refresh the index and load the graph.
    pub fn graph(&self) -> Result<Graph> {
        let (idx, _) = self.index()?;
        idx.graph()
    }

    pub fn write_map(&self, g: &Graph) -> Result<MapOutput> {
        gitnext_docs::write_map(&self.root, g)
    }
}

#[derive(Debug, Serialize)]
pub struct InitResult {
    pub root: PathBuf,
    pub created_repo: bool,
    pub stats: RefreshStats,
    pub map_files: usize,
}

/// `gitnext init`: git init (if needed) + index + map.
pub fn init(path: &Path) -> Result<InitResult> {
    std::fs::create_dir_all(path)?;
    let (repo, created) = Repo::init(path)?;
    let ws = Workspace { root: repo.root().to_path_buf(), repo: Some(repo) };
    let (idx, stats) = ws.index()?;
    let g = idx.graph()?;
    let out = ws.write_map(&g)?;
    Ok(InitResult { root: ws.root.clone(), created_repo: created, stats, map_files: out.written.len() + out.unchanged })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// HEAD → working tree (what `git diff HEAD` shows)
    Worktree,
    /// HEAD → index (what will be committed)
    Staged,
}

fn is_generated(path: &str) -> bool {
    path.starts_with(&format!("{GITNEXT_DIR}/")) || path == "AGENTS.md"
}

pub fn semantic_diff(repo: &Repo, mode: DiffMode) -> Result<Vec<FileChange>> {
    let mut out = Vec::new();
    let root = repo.root().to_path_buf();
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    for e in repo.status()? {
        if is_generated(&e.path) {
            continue;
        }
        let (old, new) = match mode {
            DiffMode::Staged => {
                let Some(st) = e.staged else { continue };
                (repo.read_head(&e.path), if st == 'D' { None } else { repo.read_index(&e.path) })
            }
            DiffMode::Worktree => {
                let deleted = e.unstaged == Some('D') || (e.staged == Some('D') && e.unstaged.is_none());
                (repo.read_head(&e.path), if deleted { None } else { repo.read_worktree(&e.path) })
            }
        };
        if old.is_none() && new.is_none() {
            continue;
        }
        let hint = gitnext_parse::find_crate(&e.path, &mut read);
        out.push(diff_sources_in(&e.path, old.as_deref(), new.as_deref(), hint.as_ref()));
    }
    Ok(out)
}

/// `YYYY-MM-DD` of a unix timestamp (UTC).
pub fn history_date(secs: i64) -> String {
    history::date(secs)
}

/// Symbol-level diff between a revision and the working tree.
pub fn since(repo: &Repo, rev: &str) -> Result<Vec<FileChange>> {
    let root = repo.root().to_path_buf();
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let mut out = Vec::new();
    for path in repo.changed_since(rev)? {
        if is_generated(&path) {
            continue;
        }
        let old = repo.read_at(rev, &path);
        let new = repo.read_worktree(&path);
        if old.is_none() && new.is_none() {
            continue;
        }
        let hint = gitnext_parse::find_crate(&path, &mut read);
        out.push(diff_sources_in(&path, old.as_deref(), new.as_deref(), hint.as_ref()));
    }
    Ok(out)
}

/// What a client has already seen of the code: symbol → (body hash, signature).
#[derive(Debug, Default, Clone)]
pub struct Snapshot(pub std::collections::HashMap<(String, String), (String, String)>);

impl Snapshot {
    pub fn of(g: &Graph) -> Self {
        Snapshot(
            g.syms
                .iter()
                .filter(|s| !(s.sym.kind == Kind::Module && s.sym.start_line == 1))
                .map(|s| ((s.file.clone(), s.sym.qualified.clone()), (s.sym.body_hash.clone(), s.sym.signature.clone())))
                .collect(),
        )
    }

    /// Changes from this snapshot to the current graph, as compact lines.
    pub fn diff(&self, g: &Graph) -> Vec<String> {
        let now = Snapshot::of(g);
        let mut out = Vec::new();
        let kind_of = |file: &str, q: &str| {
            g.syms.iter().find(|s| s.file == file && s.sym.qualified == q).map(|s| s.sym.kind.as_str()).unwrap_or("symbol")
        };
        for (k, (h, sig)) in &now.0 {
            match self.0.get(k) {
                None => out.push(format!("+ {} {}  {}", kind_of(&k.0, &k.1), k.1, k.0)),
                Some((oh, osig)) if osig != sig => out.push(format!("~ {} {}: `{osig}` → `{sig}`", kind_of(&k.0, &k.1), k.1)),
                Some((oh, _)) if oh != h => out.push(format!("* {} {} (body)", kind_of(&k.0, &k.1), k.1)),
                _ => {}
            }
        }
        for k in self.0.keys() {
            if !now.0.contains_key(k) {
                out.push(format!("- {}  {}", k.1, k.0));
            }
        }
        out.sort();
        out
    }
}

/// Commit-message trailer listing symbol-level changes.
pub fn semantic_trailer(changes: &[FileChange], max_lines: usize) -> Option<String> {
    let mut lines = Vec::new();
    let mut total = 0;
    for fc in changes.iter().filter(|f| !f.changes.is_empty()) {
        if fc.status != "modified" {
            total += fc.changes.len();
            lines.push(format!("  {} ({}, {} symbols)", fc.path, fc.status, fc.changes.len()));
            continue;
        }
        lines.push(format!("  {} ({})", fc.path, fc.status));
        for c in &fc.changes {
            total += 1;
            lines.push(format!("    {}", c.line()));
        }
    }
    if total == 0 {
        return None;
    }
    let shown = lines.len().min(max_lines);
    let mut s = String::from("Semantic-Changes (gitnext):\n");
    s.push_str(&lines[..shown].join("\n"));
    if lines.len() > shown {
        s.push_str(&format!("\n    … and {} more lines", lines.len() - shown));
    }
    Some(s)
}

#[derive(Debug, Clone)]
pub struct CommitOpts {
    pub all: bool,
    pub map: bool,
    pub semantic: bool,
}

impl Default for CommitOpts {
    fn default() -> Self {
        CommitOpts { all: false, map: true, semantic: true }
    }
}

#[derive(Debug, Serialize)]
pub struct CommitResult {
    pub id: String,
    pub message: String,
    pub changes: Vec<FileChange>,
    pub map_updated: usize,
}

/// `gitnext commit`: optional `-a`, refresh + stage the map, append the
/// semantic trailer, commit.
pub fn commit(ws: &Workspace, message: &str, opts: &CommitOpts) -> Result<CommitResult> {
    let repo = ws.repo()?;
    if message.trim().is_empty() {
        bail!("empty commit message");
    }
    if opts.all {
        repo.add(&[".".into()])?;
    }
    let user_staged = repo.status()?.iter().any(|e| e.staged.is_some() && !is_generated(&e.path));
    if !user_staged {
        bail!("nothing staged to commit (use `gitnext add <paths>` or `gitnext commit -a`)");
    }
    let changes = if opts.semantic { semantic_diff(repo, DiffMode::Staged)? } else { vec![] };
    let mut map_updated = 0;
    if opts.map {
        let g = ws.graph()?;
        let out = ws.write_map(&g)?;
        map_updated = out.written.len() + out.removed.len();
        repo.add(&[GITNEXT_DIR.into(), "AGENTS.md".into()])?;
    }
    let mut msg = message.trim_end().to_string();
    if let Some(t) = semantic_trailer(&changes, 40) {
        msg.push_str("\n\n");
        msg.push_str(&t);
    }
    msg.push('\n');
    let id = repo.commit(&msg)?;
    Ok(CommitResult { id: id.to_string(), message: msg, changes, map_updated })
}

// ───────────────────────────── context packs ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct ContextItem {
    pub qualified: String,
    pub kind: String,
    pub file: String,
    pub lines: (u32, u32),
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ContextPack {
    pub query: String,
    pub token_budget: usize,
    pub tokens_used: usize,
    pub dirs: Vec<(String, String)>,
    pub items: Vec<ContextItem>,
}

fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Pick the symbols most relevant to a free-text task and pack them
/// (bodies first, then signatures only) into an approximate token budget.
pub fn context(ws: &Workspace, g: &Graph, query: &str, token_budget: usize) -> ContextPack {
    let terms: Vec<String> = words(query).into_iter().filter(|w| w.len() >= 3).collect();
    let mut scored: Vec<(usize, usize)> = Vec::new();
    for (i, s) in g.syms.iter().enumerate() {
        if s.sym.kind == Kind::Module && s.sym.start_line == 1 {
            continue;
        }
        let name_w = words(&s.sym.name);
        let qual = s.sym.qualified.to_lowercase();
        let doc = s.sym.doc.as_deref().unwrap_or("").to_lowercase();
        let sig = s.sym.signature.to_lowercase();
        let file = s.file.to_lowercase();
        let mut score = 0;
        for t in &terms {
            if name_w.iter().any(|w| w == t) {
                score += 6;
            } else if name_w.iter().any(|w| w.starts_with(t.as_str()) || t.starts_with(w.as_str()) && w.len() >= 4) {
                score += 4;
            }
            if qual.contains(t.as_str()) {
                score += 2;
            }
            if file.contains(t.as_str()) {
                score += 2;
            }
            if doc.contains(t.as_str()) {
                score += 2;
            }
            if sig.contains(t.as_str()) {
                score += 1;
            }
        }
        if score > 0 {
            scored.push((score, i));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    // Seed with top hits, then pull in direct callers/callees of the best few.
    let mut picked: Vec<(usize, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (rank, &(score, i)) in scored.iter().take(12).enumerate() {
        if seen.insert(i) {
            picked.push((i, format!("matches query (score {score})")));
        }
        if rank < 3 {
            for (c, _) in g.callees(i).resolved.into_iter().take(4) {
                if seen.insert(c) {
                    picked.push((c, format!("called by {}", g.syms[i].sym.name)));
                }
            }
            for (c, _) in g.callers(i).into_iter().take(4) {
                if seen.insert(c) {
                    picked.push((c, format!("calls {}", g.syms[i].sym.name)));
                }
            }
        }
    }

    let budget_chars = token_budget * 4;
    let mut used = 0usize;
    let mut dirs: Vec<(String, String)> = Vec::new();
    let ctx = gitnext_docs::Ctx::new(&ws.root, g);
    for (i, _) in &picked {
        let d = gitnext_index::dir_of(&g.syms[*i].file).to_string();
        if !dirs.iter().any(|(x, _)| *x == d) {
            let p = ctx.dir_purpose(&d).unwrap_or_default();
            used += d.len() + p.len() + 8;
            dirs.push((d, p));
        }
    }
    let mut file_cache: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut items = Vec::new();
    for (i, reason) in picked {
        let r = &g.syms[i];
        let header = r.sym.signature.len() + r.sym.doc.as_deref().map_or(0, str::len) + r.file.len() + 32;
        if used + header > budget_chars {
            break;
        }
        used += header;
        let lines = file_cache.entry(r.file.clone()).or_insert_with(|| {
            std::fs::read_to_string(ws.root.join(&r.file))
                .map(|t| t.lines().map(str::to_string).collect())
                .unwrap_or_default()
        });
        let (a, b) = (r.sym.start_line as usize, r.sym.end_line as usize);
        let body = (a >= 1 && b <= lines.len()).then(|| lines[a - 1..b].join("\n"));
        let body = body.filter(|b| used + b.len() <= budget_chars && r.sym.kind != Kind::Module);
        if let Some(b) = &body {
            used += b.len();
        }
        items.push(ContextItem {
            qualified: r.sym.qualified.clone(),
            kind: r.sym.kind.as_str().into(),
            file: r.file.clone(),
            lines: (r.sym.start_line, r.sym.end_line),
            signature: r.sym.signature.clone(),
            doc: r.sym.doc.clone(),
            body,
            reason,
        });
    }
    ContextPack { query: query.into(), token_budget, tokens_used: used / 4, dirs, items }
}

pub fn render_context_md(p: &ContextPack) -> String {
    let mut s = format!("# Context: {}\n\n_~{} / {} tokens_\n\n", p.query, p.tokens_used, p.token_budget);
    if !p.dirs.is_empty() {
        s.push_str("## Areas\n\n");
        for (d, purpose) in &p.dirs {
            let d = if d.is_empty() { "/" } else { d };
            if purpose.is_empty() {
                s.push_str(&format!("- `{d}`\n"));
            } else {
                s.push_str(&format!("- `{d}`: {purpose}\n"));
            }
        }
        s.push('\n');
    }
    for it in &p.items {
        s.push_str(&format!("## `{}` ({}, {}:{}-{})\n\n_{}_\n\n", it.qualified, it.kind, it.file, it.lines.0, it.lines.1, it.reason));
        match &it.body {
            Some(b) => s.push_str(&format!("```rust\n{b}\n```\n\n")),
            None => {
                if let Some(d) = &it.doc {
                    s.push_str(&format!("{d}\n\n"));
                }
                s.push_str(&format!("```rust\n{}\n```\n\n", it.signature));
            }
        }
    }
    if p.items.is_empty() {
        s.push_str("_No matching symbols. Try other words or `gitnext symbols`._\n");
    }
    s
}

/// Plain-text semantic diff (`+ added`, `- removed`, `~ signature`, `* body/docs`).
/// New / deleted files are summarised as a symbol count unless `full`.
pub fn render_diff(d: &[FileChange], full: bool) -> String {
    if d.is_empty() {
        return "no changes\n".into();
    }
    let mut s = String::new();
    for fc in d {
        let tag = if fc.semantic { "" } else { " (no parser)" };
        if !full && fc.status != "modified" && !fc.changes.is_empty() {
            s.push_str(&format!("{} {} ({} symbols)\n", fc.status, fc.path, fc.changes.len()));
            continue;
        }
        s.push_str(&format!("{} {}{tag}\n", fc.status, fc.path));
        for c in &fc.changes {
            s.push_str(&c.line());
            s.push('\n');
        }
    }
    s
}
