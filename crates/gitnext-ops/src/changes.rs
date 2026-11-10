//! Human-friendly git: the working tree described in words and symbols, shared
//! by the TUI and the desktop app.
//!
//! - [`changes`]: files grouped as "ready to commit" / "changed" / "new" /
//!   "conflicts", each with the functions and types that changed.
//! - [`stage_symbol`]: stage or unstage one function instead of a whole file.
//! - [`draft_message`]: a commit subject written from the semantic diff.
//! - [`symbol_diff`]: side-by-side before/after of one symbol.
//! - [`humanize`]: git errors rephrased with what to do next.

use anyhow::Result;
use gitnext_core::Repo;
use gitnext_index::semdiff::{diff_sources_in, Change, FileChange, SymChange};
use gitnext_parse::{last_segment, Kind};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Conflicts,
    Staged,
    Changed,
    New,
}

impl GroupKind {
    pub fn title(self) -> &'static str {
        match self {
            GroupKind::Conflicts => "Conflicts — fix these first",
            GroupKind::Staged => "Ready to commit",
            GroupKind::Changed => "Changed, not staged",
            GroupKind::New => "New files, not tracked yet",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            GroupKind::Conflicts => "both sides changed the same lines: open the file, keep the right version, remove the <<<<<<< ======= >>>>>>> markers, then stage it",
            GroupKind::Staged => "these go into the next commit",
            GroupKind::Changed => "stage a file or a single function to include it",
            GroupKind::New => "stage to start tracking",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    /// "modified", "new file", "deleted", "renamed", "conflict"
    pub what: &'static str,
    /// whether gitnext understands the language (symbol list is meaningful)
    pub semantic: bool,
    pub changes: Vec<SymChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub kind: GroupKind,
    pub title: &'static str,
    pub hint: &'static str,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangesView {
    pub branch: String,
    /// one sentence, e.g. "3 files ready to commit, 2 changed"
    pub summary: String,
    pub groups: Vec<Group>,
    /// commit subject suggested from the staged changes (or from everything if nothing is staged)
    pub draft: String,
}

fn hint_for(repo: &Repo, path: &str) -> Option<gitnext_parse::CrateHint> {
    let root = repo.root().to_path_buf();
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    gitnext_parse::find_crate(path, &mut read)
}

fn generated(path: &str) -> bool {
    path.starts_with(".gitnext/") || path == "AGENTS.md"
}

pub fn changes(repo: &Repo) -> Result<ChangesView> {
    let mut groups: Vec<Group> = [GroupKind::Conflicts, GroupKind::Staged, GroupKind::Changed, GroupKind::New]
        .into_iter()
        .map(|k| Group { kind: k, title: k.title(), hint: k.hint(), files: vec![] })
        .collect();
    let at = |k: GroupKind| groups_index(k);
    for e in repo.status()? {
        if generated(&e.path) {
            continue;
        }
        let hint = hint_for(repo, &e.path);
        if e.unstaged == Some('U') {
            groups[at(GroupKind::Conflicts)].files.push(FileEntry { path: e.path.clone(), what: "conflict", semantic: false, changes: vec![] });
            continue;
        }
        if let Some(st) = e.staged {
            let old = repo.read_head(&e.path);
            let new = if st == 'D' { None } else { repo.read_index(&e.path) };
            let d = diff_sources_in(&e.path, old.as_deref(), new.as_deref(), hint.as_ref());
            let what = match st {
                'A' => "new file",
                'D' => "deleted",
                'R' => "renamed",
                _ => "modified",
            };
            groups[at(GroupKind::Staged)].files.push(FileEntry { path: e.path.clone(), what, semantic: d.semantic, changes: d.changes });
        }
        match e.unstaged {
            Some('?') => {
                let new = repo.read_worktree(&e.path);
                let d = diff_sources_in(&e.path, None, new.as_deref(), hint.as_ref());
                groups[at(GroupKind::New)].files.push(FileEntry { path: e.path.clone(), what: "new file", semantic: d.semantic, changes: d.changes });
            }
            Some(u) => {
                let old = repo.read_index(&e.path).or_else(|| repo.read_head(&e.path));
                let new = if u == 'D' { None } else { repo.read_worktree(&e.path) };
                let d = diff_sources_in(&e.path, old.as_deref(), new.as_deref(), hint.as_ref());
                let what = if u == 'D' { "deleted" } else { "modified" };
                groups[at(GroupKind::Changed)].files.push(FileEntry { path: e.path.clone(), what, semantic: d.semantic, changes: d.changes });
            }
            None => {}
        }
    }
    let n = |k: GroupKind| groups[at(k)].files.len();
    let mut parts = Vec::new();
    if n(GroupKind::Conflicts) > 0 {
        parts.push(format!("{} with conflicts", plural(n(GroupKind::Conflicts), "file")));
    }
    if n(GroupKind::Staged) > 0 {
        parts.push(format!("{} ready to commit", plural(n(GroupKind::Staged), "file")));
    }
    if n(GroupKind::Changed) > 0 {
        parts.push(format!("{} changed", plural(n(GroupKind::Changed), "file")));
    }
    if n(GroupKind::New) > 0 {
        parts.push(format!("{} new", plural(n(GroupKind::New), "file")));
    }
    let summary = if parts.is_empty() { "Nothing changed since the last commit.".to_string() } else { cap_first(&parts.join(", ")) + "." };
    let draft_from: Vec<FileChange> = {
        let src = if n(GroupKind::Staged) > 0 { vec![GroupKind::Staged] } else { vec![GroupKind::Changed, GroupKind::New] };
        src.iter()
            .flat_map(|&k| groups[at(k)].files.iter())
            .map(|f| FileChange { path: f.path.clone(), status: f.what, semantic: f.semantic, changes: f.changes.clone() })
            .collect()
    };
    groups.retain(|g| !g.files.is_empty());
    Ok(ChangesView { branch: repo.branch(), summary, draft: draft_message(&draft_from), groups })
}

fn groups_index(k: GroupKind) -> usize {
    match k {
        GroupKind::Conflicts => 0,
        GroupKind::Staged => 1,
        GroupKind::Changed => 2,
        GroupKind::New => 3,
    }
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn cap_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Stage (or unstage) just one symbol's lines.
pub fn stage_symbol(repo: &Repo, path: &str, c: &SymChange, unstage: bool) -> Result<usize> {
    let old: Vec<(u32, u32)> = c.old_lines.into_iter().collect();
    let new: Vec<(u32, u32)> = c.new_lines.into_iter().collect();
    let n = repo.stage_ranges(path, &old, &new, unstage)?;
    if n == 0 {
        anyhow::bail!("no lines of {} to {} — it may have changed meanwhile; refresh", short(c), if unstage { "unstage" } else { "stage" });
    }
    Ok(n)
}

/// Readable name: `Type::method` for methods, the bare name otherwise.
pub fn short(c: &SymChange) -> String {
    let parts: Vec<&str> = c.qualified.split("::").collect();
    if c.kind == Kind::Method && parts.len() >= 2 {
        format!("{}::{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        last_segment(&c.qualified).to_string()
    }
}

/// What happened to a symbol, in words: "added", "changed signature", …
pub fn verb(c: &SymChange) -> &'static str {
    match c.change {
        Change::Added => "added",
        Change::Removed => "removed",
        Change::Signature => "signature changed",
        Change::Body => "changed",
        Change::Doc => "docs changed",
        Change::Renamed => "renamed",
    }
}

/// A commit subject from symbol changes, e.g. `core: add Task::age, change sync`.
/// The full symbol list goes into the trailer `gitnext commit` appends, so the
/// subject only names the most important few.
pub fn draft_message(files: &[FileChange]) -> String {
    let all: Vec<&SymChange> = files.iter().flat_map(|f| f.changes.iter()).filter(|c| !c.test).collect();
    let scope = common_scope(files.iter().map(|f| f.path.as_str()));
    let prefix = scope.map(|s| format!("{s}: ")).unwrap_or_default();
    if all.is_empty() {
        let tests = files.iter().any(|f| f.changes.iter().any(|c| c.test));
        if tests {
            return format!("{prefix}update tests");
        }
        return match files.len() {
            0 => String::new(),
            1 => format!("Update {}", files[0].path.rsplit('/').next().unwrap_or(&files[0].path)),
            n => format!("{prefix}update {n} files"),
        };
    }
    // most telling first: new API, signature changes, renames, removals, body edits
    let rank = |c: &SymChange| match c.change {
        Change::Added => 0,
        Change::Signature => 1,
        Change::Renamed => 2,
        Change::Removed => 3,
        Change::Body => 4,
        Change::Doc => 5,
    };
    let mut v = all.clone();
    v.sort_by_key(|c| (rank(c), !c.kind.is_type(), c.qualified.clone()));
    let word = |ch: Change| match ch {
        Change::Added => "add",
        Change::Signature => "change",
        Change::Renamed => "rename",
        Change::Removed => "remove",
        Change::Body => "update",
        Change::Doc => "document",
    };
    let mut phrases: Vec<(Change, Vec<String>)> = Vec::new();
    for c in &v {
        // methods of a type that is itself added are covered by the type
        if c.kind == Kind::Method && v.iter().any(|t| t.change == Change::Added && t.kind.is_type() && c.qualified.starts_with(&format!("{}::", t.qualified))) {
            continue;
        }
        let name = if c.change == Change::Renamed {
            format!("{} → {}", c.old_signature.as_deref().map(last_segment).unwrap_or("?"), last_segment(&c.qualified))
        } else {
            short(c)
        };
        match phrases.iter_mut().find(|(ch, _)| *ch == c.change) {
            Some((_, names)) if !names.contains(&name) => names.push(name),
            Some(_) => {}
            None => phrases.push((c.change, vec![name])),
        }
    }
    let mut out = prefix.clone();
    let mut first = true;
    for (ch, names) in &phrases {
        let shown: Vec<&String> = names.iter().take(3).collect();
        let more = names.len().saturating_sub(shown.len());
        let mut p = format!("{} {}", word(*ch), shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        if more > 0 {
            p.push_str(&format!(" +{more}"));
        }
        let candidate = if first { p.clone() } else { format!("; {p}") };
        if !first && out.len() + candidate.len() > 72 {
            break;
        }
        out.push_str(&candidate);
        first = false;
    }
    if prefix.is_empty() {
        cap_first(&out)
    } else {
        out
    }
}

/// Area name shared by all paths: the crate / package folder, e.g. `core`, `taskflow_report`.
fn common_scope<'a>(paths: impl Iterator<Item = &'a str>) -> Option<String> {
    let dirs: Vec<Vec<&str>> = paths.map(|p| p.split('/').collect::<Vec<_>>()).map(|mut v| {
        v.pop();
        v
    }).collect();
    let first = dirs.first()?;
    let mut n = first.len();
    for d in &dirs[1..] {
        n = n.min(d.iter().zip(first).take_while(|(a, b)| a == b).count());
    }
    let common: Vec<&str> = first[..n].iter().copied().filter(|s| !matches!(*s, "src" | "lib" | "crates" | "packages" | "include")).collect();
    common.last().map(|s| s.to_string())
}

/// Plain-text form of [`ChangesView`] for `gitnext status`.
pub fn render_changes(cv: &ChangesView) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "on {} · {}", cv.branch, cv.summary);
    for g in &cv.groups {
        let _ = writeln!(s, "\n{} — {}", g.title, g.hint);
        for f in &g.files {
            let n = f.changes.iter().filter(|c| !c.test).count();
            let note = if f.semantic && f.changes.is_empty() && f.what == "modified" { "  (comments / formatting only)" } else { "" };
            let _ = writeln!(s, "  {:<9} {}{note}", f.what, f.path);
            if f.what == "modified" || n <= 6 {
                for c in f.changes.iter().take(20) {
                    let mark = match c.change {
                        Change::Added => '+',
                        Change::Removed => '-',
                        Change::Signature => '~',
                        _ => '*',
                    };
                    let _ = writeln!(s, "      {mark} {} {}  {}{}", c.kind.as_str(), short(c), verb(c), if c.test { " (test)" } else { "" });
                }
            } else {
                let _ = writeln!(s, "      {}", plural(f.changes.len(), "symbol"));
            }
        }
    }
    if !cv.draft.is_empty() {
        let _ = writeln!(s, "\nsuggested message: {}", cv.draft);
    }
    if !cv.groups.is_empty() {
        let _ = writeln!(s, "next: gitnext add <file|Symbol> · gitnext commit (asks to use the suggested message) · gitnext undo");
    }
    s
}

/// Find a change for `q` (a symbol name) in the given group: `(path, change)`.
pub fn find_change(cv: &ChangesView, kind: GroupKind, q: &str) -> Vec<(String, SymChange)> {
    let q2 = q.replace('.', "::");
    cv.groups
        .iter()
        .filter(|g| g.kind == kind)
        .flat_map(|g| g.files.iter())
        .filter(|f| f.what == "modified")
        .flat_map(|f| f.changes.iter().map(move |c| (f.path.clone(), c.clone())))
        .filter(|(_, c)| c.qualified == q2 || c.qualified.ends_with(&format!("::{q2}")) || last_segment(&c.qualified) == q2)
        .collect()
}

// ------------------------------------------------------------------ side by side

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Same,
    Changed,
    Removed,
    Added,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffRow {
    pub kind: RowKind,
    pub old: Option<(u32, String)>,
    pub new: Option<(u32, String)>,
}

/// Before/after rows of one symbol (or of the whole file when the symbol has no range).
pub fn symbol_diff(old_text: Option<&str>, new_text: Option<&str>, c: Option<&SymChange>) -> Vec<DiffRow> {
    let slice = |t: Option<&str>, r: Option<(u32, u32)>| -> (u32, Vec<String>) {
        let lines: Vec<String> = t.unwrap_or("").lines().map(str::to_string).collect();
        match r {
            Some((a, b)) if a >= 1 => {
                let a = (a as usize).min(lines.len().max(1));
                let b = (b as usize).min(lines.len());
                (a as u32, lines.get(a - 1..b).map(<[String]>::to_vec).unwrap_or_default())
            }
            _ if c.is_some() => (1, vec![]),
            _ => (1, lines),
        }
    };
    let (o0, old) = slice(old_text, c.and_then(|c| c.old_lines));
    let (n0, new) = slice(new_text, c.and_then(|c| c.new_lines));
    line_diff(&old, &new, o0, n0)
}

/// LCS line diff, pairing a removed run with the added run that follows it.
pub fn line_diff(old: &[String], new: &[String], o0: u32, n0: u32) -> Vec<DiffRow> {
    let (n, m) = (old.len(), new.len());
    if n * m > 4_000_000 {
        // huge: no alignment, just both sides
        let mut rows: Vec<DiffRow> = old.iter().enumerate().map(|(i, l)| DiffRow { kind: RowKind::Removed, old: Some((o0 + i as u32, l.clone())), new: None }).collect();
        rows.extend(new.iter().enumerate().map(|(i, l)| DiffRow { kind: RowKind::Added, old: None, new: Some((n0 + i as u32, l.clone())) }));
        return rows;
    }
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i] == new[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut rows = Vec::new();
    let mut del: Vec<usize> = Vec::new();
    let mut add: Vec<usize> = Vec::new();
    let flush = |del: &mut Vec<usize>, add: &mut Vec<usize>, rows: &mut Vec<DiffRow>| {
        let k = del.len().max(add.len());
        for t in 0..k {
            let o = del.get(t).map(|&x| (o0 + x as u32, old[x].clone()));
            let nw = add.get(t).map(|&x| (n0 + x as u32, new[x].clone()));
            let kind = match (&o, &nw) {
                (Some(_), Some(_)) => RowKind::Changed,
                (Some(_), None) => RowKind::Removed,
                _ => RowKind::Added,
            };
            rows.push(DiffRow { kind, old: o, new: nw });
        }
        del.clear();
        add.clear();
    };
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            flush(&mut del, &mut add, &mut rows);
            rows.push(DiffRow { kind: RowKind::Same, old: Some((o0 + i as u32, old[i].clone())), new: Some((n0 + j as u32, new[j].clone())) });
            i += 1;
            j += 1;
        } else if j < m && (i == n || dp[i][j + 1] >= dp[i + 1][j]) {
            add.push(j);
            j += 1;
        } else {
            del.push(i);
            i += 1;
        }
    }
    flush(&mut del, &mut add, &mut rows);
    rows
}

/// Reflog messages in words: "commit: Fix x" → "committed “Fix x”".
pub fn human_reflog(m: &str) -> String {
    let (kind, rest) = m.split_once(": ").unwrap_or((m, ""));
    match kind {
        "commit" | "commit (initial)" => format!("committed “{rest}”"),
        "commit (amend)" => format!("amended “{rest}”"),
        "commit (merge)" => format!("merged: {rest}"),
        k if k.starts_with("checkout") => format!("switched: {}", rest.replace("moving from ", "").replace(" to ", " → ")),
        k if k.starts_with("reset") => {
            let to = rest.trim().trim_start_matches("moving to ");
            let to = if to.len() == 40 && to.bytes().all(|b| b.is_ascii_hexdigit()) { &to[..7] } else { to };
            format!("reset to {to}")
        }
        k if k.starts_with("pull") => format!("pulled {rest}"),
        k if k.starts_with("rebase") => format!("rebase: {rest}"),
        k if k.starts_with("merge") => format!("merged {rest}"),
        "" => "(no message)".into(),
        _ => m.to_string(),
    }
}

// ------------------------------------------------------------------ errors

/// Rephrase common git failures in plain words with the next step.
pub fn humanize(e: &anyhow::Error) -> String {
    let raw = format!("{e:#}");
    let low = raw.to_lowercase();
    let msg = if low.contains("nothing to commit") {
        "Nothing is staged. Stage files or functions first (space), or commit everything with ctrl-a."
    } else if low.contains("identity not configured") || low.contains("config value 'user.name'") {
        "Git doesn't know who you are yet. Run once: git config --global user.name \"Your Name\" and git config --global user.email you@example.com"
    } else if low.contains("conflict") {
        "Some files have conflicts. Open them, keep the right version, remove the <<<<<<< ======= >>>>>>> markers, then stage them."
    } else if low.contains("not a git repository") {
        "This folder isn't a git repository yet. Run `gitnext init` to start one."
    } else if low.contains("unborn") || low.contains("reference 'refs/heads/") && low.contains("not found") {
        "There are no commits yet — make the first commit."
    } else if low.contains("index.lock") || low.contains("locked") {
        "Another git program is working in this repository (index.lock exists). Wait for it to finish, or delete .git/index.lock if nothing is running."
    } else if low.contains("authentication") || low.contains("permission denied (publickey)") {
        "The remote refused your credentials. Check your SSH key or token."
    } else {
        return raw;
    };
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitnext_index::semdiff::diff_sources;

    #[test]
    fn draft_names_the_important_changes() {
        let old = "pub fn a(x: u32) -> u32 { x }\nfn b() {}\n";
        let new = "pub fn a(x: u64) -> u64 { x }\nfn b() { println!(); }\npub fn c() {}\n";
        let d = diff_sources("crates/core/src/lib.rs", Some(old), Some(new));
        let m = draft_message(&[d]);
        assert_eq!(m, "core: add c; change a; update b");
    }

    #[test]
    fn side_by_side_pairs_changed_lines() {
        let a: Vec<String> = ["x", "y", "z"].iter().map(|s| s.to_string()).collect();
        let b: Vec<String> = ["x", "Y", "z", "w"].iter().map(|s| s.to_string()).collect();
        let rows = line_diff(&a, &b, 1, 1);
        let kinds: Vec<RowKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, vec![RowKind::Same, RowKind::Changed, RowKind::Same, RowKind::Added]);
    }

    #[test]
    fn stage_one_function() {
        let dir = std::env::temp_dir().join(format!("gitnext-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (repo, _) = Repo::init(&dir).unwrap();
        repo.inner.config().unwrap().set_str("user.name", "t").unwrap();
        repo.inner.config().unwrap().set_str("user.email", "t@t").unwrap();
        let v1 = "fn a() {\n    1;\n}\n\nfn b() {\n    2;\n}\n\nfn c() {\n    3;\n}\n";
        std::fs::write(dir.join("m.rs"), v1).unwrap();
        repo.add(&[]).unwrap();
        repo.commit("one").unwrap();
        let v2 = v1.replace("1;", "10;").replace("3;", "30;");
        std::fs::write(dir.join("m.rs"), &v2).unwrap();
        let view = changes(&repo).unwrap();
        let changed = view.groups.iter().find(|g| g.kind == GroupKind::Changed).unwrap();
        let f = &changed.files[0];
        let c = f.changes.iter().find(|c| c.qualified.ends_with("c")).unwrap();
        stage_symbol(&repo, &f.path, c, false).unwrap();
        let staged = repo.read_index("m.rs").unwrap();
        assert!(staged.contains("30;") && staged.contains("    1;"), "{staged}");
        // and back
        let view = changes(&repo).unwrap();
        let st = view.groups.iter().find(|g| g.kind == GroupKind::Staged).unwrap();
        let c = st.files[0].changes.iter().find(|c| c.qualified.ends_with("c")).unwrap();
        stage_symbol(&repo, "m.rs", c, true).unwrap();
        assert_eq!(repo.read_index("m.rs").unwrap(), v1);
        // undo
        repo.add(&[]).unwrap();
        repo.commit("two").unwrap();
        let undone = repo.undo_commit().unwrap();
        assert_eq!(undone.summary, "two");
        assert_eq!(repo.read_index("m.rs").unwrap(), v2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
