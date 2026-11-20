//! A test fails, and the code changed since the tests last passed: which change broke it?
//! `why` shows those changes; a small model often does not tell from them which one is at
//! fault and spends its steps. Here each change is undone on its own and the failing test run
//! again: the change whose undoing alone makes the test pass is the cause, proven, and the
//! answer gives the function as it should be. In the agent runs (Gemma 4 12B, 102 seeded bugs,
//! `eval/results` run 11) this took the bugs fixed from 48% to 89%, for the same tokens.
//!
//! The files are put back after every run. What a run is about to change is written down
//! first (`.gitnext/prove-<pid>/`), so a process killed in between has its changes put back by
//! the next `why`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gitnext_index::Graph;

use crate::check::{self, Step};

/// At most this many changes are tried, the most suspect first.
pub const MAX_TRIED: usize = 6;
/// Trying stops once this much time went on it (seconds; `GITNEXT_PROVE_SECONDS`).
const BUDGET: u64 = 300;
const DIR: &str = "prove-";

/// One hunk of `git diff -U0 <commit>`: lines `new..new+plus.len()` of the file as it is now
/// were `minus` then.
#[derive(Clone, Debug, PartialEq)]
pub struct Hunk {
    pub file: String,
    pub new: u32,
    pub count: u32,
    pub minus: Vec<String>,
    pub plus: Vec<String>,
}

pub struct Proof {
    /// the commit the map was learned on: the tests passed there
    pub since: String,
    /// the changes in the project's own code since then (tests left out)
    pub changes: usize,
    /// how many were undone and tried
    pub tried: usize,
    pub cause: Option<Hunk>,
    /// the function the cause is in, and the function as it should be
    pub symbol: Option<usize>,
    pub fixed: Option<String>,
    /// the cause is one line that stands in one place only: it can be put back by itself
    pub unique_line: bool,
}

/// The hunks of `git diff -U0` text.
pub fn parse_hunks(diff: &str) -> Vec<Hunk> {
    let (mut out, mut file): (Vec<Hunk>, Option<String>) = (Vec::new(), None);
    for l in diff.lines() {
        if let Some(p) = l.strip_prefix("+++ ") {
            file = p.strip_prefix("b/").map(String::from); // `+++ /dev/null`: the file was deleted
        } else if l.starts_with("--- ") {
            // the old side's name: the new side's is the one that counts
        } else if let Some(rest) = l.strip_prefix("@@ ") {
            let Some(f) = &file else { continue };
            let new = rest.split(' ').find(|x| x.starts_with('+')).unwrap_or("+0");
            let mut it = new[1..].split(',');
            let start: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let count: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
            out.push(Hunk { file: f.clone(), new: start, count, minus: vec![], plus: vec![] });
        } else if let (Some(h), Some(f)) = (out.last_mut(), &file) {
            if &h.file != f {
                continue;
            }
            if let Some(x) = l.strip_prefix('-') {
                h.minus.push(x.to_string());
            } else if let Some(x) = l.strip_prefix('+') {
                h.plus.push(x.to_string());
            }
        }
    }
    out
}

/// The text with one hunk put back as it was.
pub fn undone(text: &str, h: &Hunk) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    // `+n,0`: nothing is there now; what was removed goes after line n
    let at = if h.count == 0 { h.new as usize } else { (h.new as usize).saturating_sub(1) };
    let end = (at + h.count as usize).min(lines.len());
    let at = at.min(end);
    lines.splice(at..end, h.minus.iter().map(String::as_str));
    lines.join("\n")
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The changes since `commit` in the project's own code, its tests left out.
pub fn changes_since(root: &Path, commit: &str) -> Vec<Hunk> {
    let Some(diff) = git(root, &["diff", "-U0", "--no-color", "--no-ext-diff", "--no-renames", commit, "--"]) else { return vec![] };
    parse_hunks(&diff).into_iter().filter(|h| !gitnext_parse::is_test_path(&h.file)).collect()
}

/// One undone file, written down before it is touched; put back when dropped.
struct Undo {
    root: PathBuf,
    dir: PathBuf,
    file: String,
    was: Vec<u8>,
    wrote: Vec<u8>,
}

impl Undo {
    fn apply(root: &Path, h: &Hunk) -> Option<Undo> {
        let path = root.join(&h.file);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return None;
        }
        let was = std::fs::read(&path).ok()?;
        let wrote = undone(&String::from_utf8(was.clone()).ok()?, h).into_bytes();
        let dir = root.join(gitnext_index::GITNEXT_DIR).join(format!("{DIR}{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        // on disk before the file is touched: what it was, what it is about to be, and where
        std::fs::write(dir.join("was"), &was).ok()?;
        std::fs::write(dir.join("wrote"), &wrote).ok()?;
        std::fs::write(dir.join("file"), &h.file).ok()?;
        let undo = Undo { root: root.to_path_buf(), dir, file: h.file.clone(), was, wrote };
        let after = check::past_the_second_of(std::slice::from_ref(&path));
        check::write_later(&path, &undo.wrote, after).ok()?;
        Some(undo)
    }
}

impl Drop for Undo {
    fn drop(&mut self) {
        let path = self.root.join(&self.file);
        // somebody wrote the file meanwhile: theirs stays, what it was is left in the folder
        if std::fs::read(&path).ok().as_ref() != Some(&self.wrote) {
            return;
        }
        let after = check::past_the_second_of(std::slice::from_ref(&path));
        if check::write_later(&path, &self.was, after).is_ok() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Changes a killed `why` left undone are put back: only where the file is still what it wrote.
pub fn recover(root: &Path) -> Vec<String> {
    let mut back = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join(gitnext_index::GITNEXT_DIR)) else { return back };
    for dir in entries.flatten().map(|e| e.path()) {
        let Some(pid) = dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix(DIR)).map(String::from) else { continue };
        let alive = Command::new("kill").args(["-0", &pid]).stdin(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if alive && pid != std::process::id().to_string() {
            continue; // another `why` is running its test right now
        }
        let (Ok(file), Ok(was), Ok(wrote)) = (std::fs::read_to_string(dir.join("file")), std::fs::read(dir.join("was")), std::fs::read(dir.join("wrote"))) else {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        };
        let path = root.join(&file);
        if std::fs::read(&path).ok() == Some(wrote) && std::fs::write(&path, was).is_ok() {
            back.push(file);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    back
}

/// The function around a line: the innermost callable.
fn function_at(g: &Graph, file: &str, line: u32) -> Option<usize> {
    (0..g.syms.len())
        .filter(|&i| g.syms[i].file == file && g.syms[i].sym.kind.is_callable() && g.syms[i].sym.start_line <= line && g.syms[i].sym.end_line >= line)
        .min_by_key(|&i| g.syms[i].sym.end_line - g.syms[i].sym.start_line)
}

/// Undo the changes since `since` one at a time, most suspect first, and run the failing test
/// commands each time. `rank` orders the functions (the suspects of `why`, first = most likely).
pub fn prove(root: &Path, g: &Graph, since: &str, failing: &[&Step], rank: &[usize], timeout: Duration) -> Option<Proof> {
    if std::env::var("GITNEXT_PROVE").as_deref() == Ok("0") || failing.is_empty() || failing.iter().any(|s| s.timed_out) {
        return None;
    }
    let mut hunks = changes_since(root, since);
    if hunks.is_empty() {
        return None;
    }
    let changes = hunks.len();
    let place = |h: &Hunk| function_at(g, &h.file, h.new.max(1)).and_then(|f| rank.iter().position(|&r| r == f)).unwrap_or(usize::MAX);
    hunks.sort_by_key(|h| place(h)); // stable: the rest stay in the diff's order
    hunks.truncate(MAX_TRIED);
    let budget = Duration::from_secs(std::env::var("GITNEXT_PROVE_SECONDS").ok().and_then(|x| x.parse().ok()).unwrap_or(BUDGET));
    let started = Instant::now();
    let mut proof = Proof { since: since.to_string(), changes, tried: 0, cause: None, symbol: None, fixed: None, unique_line: false };
    for h in hunks {
        if started.elapsed() > budget {
            break;
        }
        let Some(undo) = Undo::apply(root, &h) else { continue };
        proof.tried += 1;
        let passed = failing.iter().all(|s| check::run(root, g, "tests", &s.command, timeout).ok);
        drop(undo);
        if passed {
            proof.cause = Some(h);
            break;
        }
    }
    if proof.tried == 0 {
        return None;
    }
    let Some(h) = proof.cause.clone() else { return Some(proof) };
    proof.symbol = function_at(g, &h.file, h.new.max(1));
    if let (Some(i), Ok(text)) = (proof.symbol, std::fs::read_to_string(root.join(&h.file))) {
        // the function's lines in the file with the change undone: it ends as many lines later
        // as the change took away
        let r = &g.syms[i];
        let (a, b) = (r.sym.start_line as usize, r.sym.end_line as usize);
        let end = (b + h.minus.len()).saturating_sub(h.plus.len());
        let after = undone(&text, &h);
        let lines: Vec<&str> = after.split('\n').collect();
        if a >= 1 && end >= a && end <= lines.len() {
            proof.fixed = Some(lines[a - 1..end].join("\n"));
        }
    }
    if h.minus.len() == 1 && h.plus.len() == 1 {
        let line = h.plus[0].trim();
        let places = git(root, &["grep", "-F", "-c", "-e", line, "--"]).unwrap_or_default();
        let rows: Vec<&str> = places.lines().filter(|l| !l.is_empty()).collect();
        proof.unique_line = !line.is_empty() && rows.len() == 1 && rows[0].ends_with(":1");
    }
    Some(proof)
}
