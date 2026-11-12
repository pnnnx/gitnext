//! Operation log: every file write made through gitnext (replace / insert / delete / rename, from the CLI
//! or MCP) is one numbered operation in `.gitnext/ops.jsonl` (local, git-ignored) that can be undone on its
//! own — `gitnext ops`, `gitnext undo --op N` — in the spirit of `jj op log` / `jj undo`.
//!
//! An operation keeps, per file, the changed blocks of lines (old and new text with 3 lines of context)
//! and the sha1 of the file before and after. Undo puts the old block back: directly when the file is
//! exactly as the operation left it; otherwise only where the new block with its context is found exactly
//! once, so later edits elsewhere in the file survive. When it is not found, undo refuses and writes nothing.
//! The undo is itself an operation, so undoing it redoes the change.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::sync::Mutex;

/// Lines of context stored around each changed block.
const CONTEXT: usize = 3;
/// The log is cut to its newer half when it grows past either limit; ids keep counting.
pub const MAX_BYTES: usize = 5 << 20;
pub const MAX_OPS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hunk {
    /// 0-based line in the file *after* the operation where `new` starts
    pub at: usize,
    pub old: Vec<String>,
    pub new: Vec<String>,
    /// up to 3 lines before / after `new`; fewer only at the start / end of the file
    pub pre: Vec<String>,
    pub post: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOp {
    pub path: String,
    /// sha1 of the whole file before / after the operation
    pub before: String,
    pub after: String,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub id: u64,
    /// unix seconds
    pub ts: i64,
    /// `cli` or `mcp:<session>`
    pub actor: String,
    pub tool: String,
    pub target: String,
    pub files: Vec<FileOp>,
    /// set on undo operations: the operation they took back
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<u64>,
}

type Change = (String, String, String);

static ACTOR: Mutex<Option<String>> = Mutex::new(None);
thread_local! {
    static LAST: RefCell<Option<u64>> = const { RefCell::new(None) };
    static GROUP: RefCell<Option<Vec<Change>>> = const { RefCell::new(None) };
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Who writes the operations of this process (default `cli`; the MCP server sets `mcp:<session>`).
pub fn set_actor(a: impl Into<String>) {
    *lock(&ACTOR) = Some(a.into());
}

/// Id of the last operation this process recorded, once.
pub fn take_last() -> Option<u64> {
    LAST.with(|l| l.borrow_mut().take())
}

pub fn path(root: &Path) -> PathBuf {
    root.join(gitnext_index::GITNEXT_DIR).join("ops.jsonl")
}

fn sha(s: &str) -> String {
    git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).map(|o| o.to_string()).unwrap_or_default()
}

/// Lines with their terminators: joining them gives the text back byte for byte (CRLF, no final newline).
fn lines(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Changed blocks between two versions of a file. The common head and tail are cut off; when the rest
/// has the same number of lines (rename, text edits) it is split into runs of changed lines.
pub fn hunks(before: &str, after: &str) -> Vec<Hunk> {
    let (a, b) = (lines(before), lines(after));
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let (ae, be) = (a.len() - s, b.len() - s);
    if p == ae && p == be {
        return vec![];
    }
    let mut spans = Vec::new(); // (old start, old end, new start, new end)
    if ae - p == be - p {
        let mut k = p;
        while k < ae {
            if a[k] == b[k] {
                k += 1;
                continue;
            }
            let st = k;
            while k < ae && a[k] != b[k] {
                k += 1;
            }
            spans.push((st, k, st, k));
        }
    } else {
        spans.push((p, ae, p, be));
    }
    spans
        .into_iter()
        .map(|(a0, a1, b0, b1)| Hunk {
            at: b0,
            old: owned(&a[a0..a1]),
            new: owned(&b[b0..b1]),
            pre: owned(&b[b0.saturating_sub(CONTEXT)..b0]),
            post: owned(&b[b1..(b1 + CONTEXT).min(b.len())]),
        })
        .collect()
}

/// Record files written by one gitnext action as an operation. Inside a [`Group`] the files join the
/// group's operation instead. Never fails the edit: a log that cannot be written is a warning.
pub fn record(root: &Path, tool: &str, target: &str, changes: &[Change]) -> Option<u64> {
    let grouped = GROUP.with(|g| {
        let mut g = g.borrow_mut();
        let Some(g) = g.as_mut() else { return false };
        for (p, b, a) in changes {
            match g.iter_mut().find(|c| &c.0 == p) {
                Some(c) => c.2 = a.clone(),
                None => g.push((p.clone(), b.clone(), a.clone())),
            }
        }
        true
    });
    if grouped {
        return None;
    }
    write_op(root, tool, target, None, changes)
}

/// All files this thread writes while a `Group` lives make one operation (e.g. `rename --text`: the rename and the
/// text pass). It is written when the group is dropped.
pub struct Group {
    root: PathBuf,
    tool: String,
    target: String,
    outer: bool,
}

impl Group {
    pub fn start(root: &Path, tool: &str, target: &str) -> Group {
        let outer = GROUP.with(|g| {
            let mut g = g.borrow_mut();
            let outer = g.is_some();
            if !outer {
                *g = Some(Vec::new());
            }
            outer
        });
        Group { root: root.to_path_buf(), tool: tool.into(), target: target.into(), outer }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if self.outer {
            return;
        }
        let changes = GROUP.with(|g| g.borrow_mut().take()).unwrap_or_default();
        write_op(&self.root, &self.tool, &self.target, None, &changes);
    }
}

fn write_op(root: &Path, tool: &str, target: &str, undoes: Option<u64>, changes: &[Change]) -> Option<u64> {
    let files: Vec<FileOp> = changes
        .iter()
        .filter(|(_, b, a)| b != a)
        .map(|(p, b, a)| FileOp { path: p.clone(), before: sha(b), after: sha(a), hunks: hunks(b, a) })
        .collect();
    if files.is_empty() {
        return None;
    }
    let op = Op {
        id: 0,
        ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        actor: lock(&ACTOR).clone().unwrap_or_else(|| "cli".into()),
        tool: tool.into(),
        target: target.into(),
        files,
        undoes,
    };
    match append(root, op) {
        Ok(id) => {
            LAST.with(|l| *l.borrow_mut() = Some(id));
            Some(id)
        }
        Err(e) => {
            eprintln!("warning: the operation was not logged, `gitnext undo --op` will not see it: {e:#}");
            None
        }
    }
}

#[derive(Deserialize)]
struct IdOnly {
    id: u64,
}

/// Append under an exclusive file lock, so a CLI and an MCP server writing at once get distinct ids.
fn append(root: &Path, mut op: Op) -> Result<u64> {
    let p = path(root);
    std::fs::create_dir_all(p.parent().unwrap_or(root))?;
    let mut f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(&p).with_context(|| p.display().to_string())?;
    f.lock()?;
    let mut text = String::new();
    f.read_to_string(&mut text)?;
    let last = text.lines().rev().find_map(|l| serde_json::from_str::<IdOnly>(l).ok()).map_or(0, |x| x.id);
    op.id = last + 1;
    let line = serde_json::to_string(&op)? + "\n";
    let mut all: Vec<&str> = text.split_inclusive('\n').filter(|l| l.ends_with('\n')).collect();
    if text.len() + line.len() > MAX_BYTES || all.len() >= MAX_OPS {
        // keep the newer half (by count and by size); ids are never reused
        let (mut keep, mut bytes) = (0, line.len());
        for l in all.iter().rev() {
            if keep + 1 >= MAX_OPS / 2 || bytes + l.len() > MAX_BYTES / 2 {
                break;
            }
            keep += 1;
            bytes += l.len();
        }
        all.drain(..all.len() - keep);
        let mut out = all.concat();
        out.push_str(&line);
        f.set_len(0)?;
        f.write_all(out.as_bytes())?;
    } else {
        // a line cut short by a crash would glue onto ours
        if !text.is_empty() && !text.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        f.write_all(line.as_bytes())?;
    }
    Ok(op.id)
}

pub fn load(root: &Path) -> Vec<Op> {
    std::fs::read_to_string(path(root)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// The undo that currently takes `id` back (an undo that was itself undone does not count).
fn undone_by(ops: &[Op], id: u64) -> Option<u64> {
    ops.iter().filter(|o| o.undoes == Some(id)).map(|o| o.id).find(|&u| undone_by(ops, u).is_none())
}

fn describe(o: &Op) -> String {
    format!("#{} {} {}", o.id, o.tool, o.target)
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoReport {
    pub undone: u64,
    pub what: String,
    /// id of the undo operation itself (undo it to redo)
    pub op: Option<u64>,
    pub files: Vec<String>,
}

fn find_block(cur: &[&str], h: &Hunk) -> Vec<usize> {
    let pat: Vec<&str> = h.pre.iter().chain(&h.new).chain(&h.post).map(String::as_str).collect();
    if pat.is_empty() || pat.len() > cur.len() {
        return vec![];
    }
    // no anchor to the start / end of the file: code appended after the last function or an import added
    // on top is the most common later change, and "exactly one match" already guards a short context
    (0..=cur.len() - pat.len()).filter(|&i| cur[i..i + pat.len()] == pat[..]).map(|i| i + h.pre.len()).collect()
}

/// The file with the operation's blocks put back, or why that is not possible.
fn revert_file(root: &Path, ops: &[Op], o: &Op, f: &FileOp) -> Result<(String, String)> {
    let cur = std::fs::read_to_string(root.join(&f.path)).with_context(|| format!("{} is gone; nothing undone", f.path))?;
    let c = lines(&cur);
    let mut spans: Vec<(usize, &Hunk)> = Vec::new();
    if sha(&cur) == f.after {
        spans.extend(f.hunks.iter().map(|h| (h.at, h)));
    } else {
        for h in &f.hunks {
            let found = find_block(&c, h);
            if found.len() != 1 {
                let later: Vec<String> = ops.iter().filter(|x| x.id > o.id && x.files.iter().any(|y| y.path == f.path)).map(describe).collect();
                let by = if later.is_empty() { "by hand or another tool".to_string() } else { format!("by {}", later.join(", ")) };
                let what = if found.is_empty() { "were changed" } else { "now appear more than once" };
                bail!(
                    "{}: lines {}-{} written by operation #{} {what} since ({by}); nothing undone{}",
                    f.path,
                    h.at + 1,
                    h.at + h.new.len().max(1),
                    o.id,
                    if later.is_empty() { String::new() } else { " — undo those first".into() }
                );
            }
            spans.push((found[0], h));
        }
    }
    spans.sort_by_key(|s| s.0);
    if spans.windows(2).any(|w| w[0].0 + w[0].1.new.len() > w[1].0) {
        bail!("{}: the blocks of operation #{} overlap in the current file; nothing undone", f.path, o.id);
    }
    let mut out: Vec<&str> = c.clone();
    for (at, h) in spans.iter().rev() {
        out.splice(*at..*at + h.new.len(), h.old.iter().map(String::as_str));
    }
    let new = out.concat();
    Ok((cur, new))
}

/// Undo one operation. Every file is checked first; if any cannot be put back, nothing is written.
pub fn undo(root: &Path, id: u64) -> Result<UndoReport> {
    let ops = load(root);
    let Some(o) = ops.iter().find(|o| o.id == id) else {
        let first = ops.first().map_or(String::new(), |o| format!(" (the log keeps #{} and later)", o.id));
        bail!("no operation #{id}{first}; `gitnext ops` lists them");
    };
    if let Some(u) = undone_by(&ops, id) {
        bail!("operation #{id} is already undone by #{u}; `gitnext undo --op {u}` brings it back");
    }
    let mut changes = Vec::new();
    for f in &o.files {
        // the log is a local file anyone can write: an operation never reaches outside the repository
        if f.path.is_empty() || !Path::new(&f.path).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
            bail!("operation #{id} names a file outside the repository ({}); nothing undone", f.path);
        }
        let (cur, new) = revert_file(root, &ops, o, f)?;
        changes.push((f.path.clone(), cur, new));
    }
    for (p, _, new) in &changes {
        std::fs::write(root.join(p), new).with_context(|| format!("writing {p}"))?;
    }
    let what = describe(o);
    let op = write_op(root, "undo", &what, Some(id), &changes);
    Ok(UndoReport { undone: id, what, op, files: changes.into_iter().map(|c| c.0).collect() })
}

pub fn render_undo(r: &UndoReport) -> String {
    let mut s = format!("✓ undid {} — {}\n", r.what, r.files.join(", "));
    if let Some(op) = r.op {
        let _ = writeln!(s, "  this undo is #{op}: `gitnext undo --op {op}` brings the change back");
    }
    s
}

/// Newest first: number, time, who, what, files with lines removed / added.
pub fn render(ops: &[Op], n: usize) -> String {
    if ops.is_empty() {
        return "no operations yet: edits made through gitnext (edit, rename, MCP edit tools) are listed here\n".into();
    }
    let mut s = String::new();
    for o in ops.iter().rev().take(n) {
        let files: Vec<String> = o
            .files
            .iter()
            .map(|f| {
                let (old, new) = f.hunks.iter().fold((0, 0), |(a, b), h| (a + h.old.len(), b + h.new.len()));
                format!("{} −{old} +{new}", f.path)
            })
            .collect();
        let mark = undone_by(ops, o.id).map_or(String::new(), |u| format!("  [undone by #{u}]"));
        let _ = writeln!(s, "#{:<4} {}  {:<10} {} {}{mark}", o.id, crate::journal::hm(o.ts), o.actor, o.tool, o.target);
        let _ = writeln!(s, "      {}", files.join(", "));
    }
    s.push_str("undo one: `gitnext undo --op N` (refuses, changing nothing, if that code was rewritten since)\n");
    s
}
