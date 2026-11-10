//! Symbol-aware history: when did this function appear and how did it change?
//! And a semantic changelog between two revisions with breaking-change detection.

use anyhow::{Context, Result};
use gitnext_core::Repo;
use gitnext_index::semdiff::{diff_sources_in, Change, FileChange};
use gitnext_index::Graph;
use gitnext_parse::{analyze_in, find_crate, CrateHint, Symbol};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct HistoryEvent {
    pub commit: String,
    pub date: String,
    pub author: String,
    pub message: String,
    /// added | removed | signature | body | doc
    pub change: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_signature: Option<String>,
}

pub fn date(secs: i64) -> String {
    // civil date from unix seconds (UTC), no chrono dependency
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

fn blob_at(repo: &git2::Repository, tree: &git2::Tree, path: &str) -> Option<(git2::Oid, String)> {
    let e = tree.get_path(Path::new(path)).ok()?;
    let b = repo.find_blob(e.id()).ok()?;
    Some((e.id(), String::from_utf8(b.content().to_vec()).ok()?))
}

fn find_sym(path: &str, src: &str, hint: Option<&CrateHint>, qualified: &str, trait_impl: &Option<String>) -> Option<Symbol> {
    let a = analyze_in(Path::new(path), src, hint).ok()??;
    a.symbols.into_iter().find(|s| s.qualified == qualified && &s.trait_impl == trait_impl)
}

/// Walk up to `limit` commits touching the symbol's file, newest first.
pub fn history(repo: &Repo, g: &Graph, i: usize, limit: usize) -> Result<Vec<HistoryEvent>> {
    let r = &g.syms[i];
    let (path, mut q, ti) = (r.file.clone(), r.sym.qualified.clone(), r.sym.trait_impl.clone());
    let git = &repo.inner;
    let mut read = |p: &str| std::fs::read_to_string(repo.root().join(p)).ok();
    let hint = find_crate(&path, &mut read);
    let mut walk = git.revwalk()?;
    if walk.push_head().is_err() {
        return Ok(vec![]);
    }
    walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
    let mut out = Vec::new();
    for (n, oid) in walk.enumerate() {
        if n >= limit {
            break;
        }
        let c = git.find_commit(oid?)?;
        let tree = c.tree()?;
        let new = blob_at(git, &tree, &path);
        let parent_tree = c.parent(0).ok().and_then(|p| p.tree().ok());
        let old = parent_tree.as_ref().and_then(|t| blob_at(git, t, &path));
        if new.as_ref().map(|x| x.0) == old.as_ref().map(|x| x.0) {
            continue; // file untouched by this commit
        }
        let ns = new.as_ref().and_then(|(_, s)| find_sym(&path, s, hint.as_ref(), &q, &ti));
        let os = old.as_ref().and_then(|(_, s)| find_sym(&path, s, hint.as_ref(), &q, &ti));
        // a rename in this commit? then keep following the old name
        if os.is_none() && ns.is_some() {
            if let (Some((_, o)), Some((_, nw))) = (&old, &new) {
                let d = diff_sources_in(&path, Some(o), Some(nw), hint.as_ref());
                if let Some(rn) = d.changes.iter().find(|c| c.change == Change::Renamed && c.qualified == q) {
                    let from = rn.old_signature.clone().unwrap_or_default();
                    out.push(HistoryEvent {
                        commit: c.id().to_string()[..7].to_string(),
                        date: date(c.time().seconds()),
                        author: c.author().name().unwrap_or("").to_string(),
                        message: c.summary().ok().flatten().unwrap_or("").to_string(),
                        change: "renamed",
                        old_signature: Some(from.clone()),
                        new_signature: Some(q.clone()),
                    });
                    q = from;
                    continue;
                }
            }
        }
        let change = match (&os, &ns) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (Some(o), Some(n)) if o.signature != n.signature => "signature",
            (Some(o), Some(n)) if o.body_hash != n.body_hash => "body",
            (Some(o), Some(n)) if o.doc != n.doc => "doc",
            _ => continue,
        };
        out.push(HistoryEvent {
            commit: c.id().to_string()[..7].to_string(),
            date: date(c.time().seconds()),
            author: c.author().name().unwrap_or("").to_string(),
            message: c.summary().ok().flatten().unwrap_or("").to_string(),
            change,
            old_signature: (change == "signature").then(|| os.as_ref().map(|s| s.signature.clone())).flatten(),
            new_signature: (change == "signature").then(|| ns.as_ref().map(|s| s.signature.clone())).flatten(),
        });
        if change == "added" {
            break; // reached its birth
        }
    }
    Ok(out)
}

pub fn render_history(g: &Graph, i: usize, ev: &[HistoryEvent]) -> String {
    let mut s = format!("history of {} ({}), {} changes:\n", g.short_name(i), g.syms[i].file, ev.len());
    for e in ev {
        let what = match e.change {
            "added" => "+ added".to_string(),
            "removed" => "- removed".to_string(),
            "signature" => format!("~ `{}` → `{}`", e.old_signature.as_deref().unwrap_or(""), e.new_signature.as_deref().unwrap_or("")),
            "body" => "* body".to_string(),
            "renamed" => format!("→ renamed from {}", e.old_signature.as_deref().unwrap_or("?").rsplit("::").next().unwrap_or("")),
            _ => "* docs".to_string(),
        };
        let _ = writeln!(s, "{} {} {}  {what}  \"{}\"", e.date, e.commit, e.author, e.message);
    }
    if ev.is_empty() {
        s.push_str("not changed in the scanned history (uncommitted or older than the limit)\n");
    }
    s
}

// ───────────────────────────── changelog ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct Changelog {
    pub range: String,
    pub commits: Vec<(String, String, String)>,
    pub files: Vec<FileChange>,
    /// `line` of changes that break callers: (change line, external caller count)
    pub breaking: Vec<(String, usize)>,
}

/// Resolve `a..b` (b defaults to HEAD; a past the first commit = empty tree).
fn trees<'r>(git: &'r git2::Repository, range: &str) -> Result<(Option<git2::Tree<'r>>, git2::Tree<'r>, Option<git2::Oid>, git2::Oid)> {
    let (a, b) = match range.split_once("..") {
        Some((a, b)) => (a.to_string(), if b.is_empty() { "HEAD".to_string() } else { b.to_string() }),
        None => (range.to_string(), "HEAD".to_string()),
    };
    let bo = git.revparse_single(&b).with_context(|| format!("unknown revision `{b}`"))?;
    let bt = bo.peel_to_tree()?;
    let bid = bo.peel_to_commit()?.id();
    let ao = git.revparse_single(&a).ok();
    let at = ao.as_ref().and_then(|o| o.peel_to_tree().ok());
    let aid = ao.and_then(|o| o.peel_to_commit().ok()).map(|c| c.id());
    Ok((at, bt, aid, bid))
}

pub fn changelog(repo: &Repo, g: &Graph, range: &str) -> Result<Changelog> {
    let git = &repo.inner;
    let (at, bt, aid, bid) = trees(git, range)?;
    let diff = git.diff_tree_to_tree(at.as_ref(), Some(&bt), None)?;
    let mut read = |p: &str| std::fs::read_to_string(repo.root().join(p)).ok();
    let mut files = Vec::new();
    for d in diff.deltas() {
        let path = d.new_file().path().or_else(|| d.old_file().path()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        if path.starts_with(".gitnext/") || path == "AGENTS.md" {
            continue;
        }
        let blob = |id: git2::Oid| (!id.is_zero()).then(|| git.find_blob(id).ok().and_then(|b| String::from_utf8(b.content().to_vec()).ok())).flatten();
        let (old, new) = (blob(d.old_file().id()), blob(d.new_file().id()));
        let hint = find_crate(&path, &mut read);
        files.push(diff_sources_in(&path, old.as_deref(), new.as_deref(), hint.as_ref()));
    }
    // commits in range
    let mut walk = git.revwalk()?;
    walk.push(bid)?;
    if let Some(a) = aid {
        walk.hide(a)?;
    }
    let mut commits = Vec::new();
    for oid in walk.take(200) {
        let c = git.find_commit(oid?)?;
        commits.push((c.id().to_string()[..7].to_string(), c.summary().ok().flatten().unwrap_or("").to_string(), c.author().name().unwrap_or("").to_string()));
    }
    // breaking: public API removed or re-signed while other files still call / use it
    let mut breaking = Vec::new();
    for fc in &files {
        for c in &fc.changes {
            if !matches!(c.change, Change::Removed | Change::Signature | Change::Renamed) {
                continue;
            }
            let q = if c.change == Change::Renamed { c.old_signature.clone().unwrap_or_default() } else { c.qualified.clone() };
            // refs to the symbol in the *current* graph from other files
            let ext: usize = g
                .find(&c.qualified)
                .into_iter()
                .chain(g.find(&q))
                .map(|i| g.referrers(i).iter().filter(|(s, _, _)| g.syms[*s].file != fc.path).count())
                .max()
                .unwrap_or(0);
            if ext > 0 || c.change == Change::Removed {
                breaking.push((format!("{}  {}", c.line(), fc.path), ext));
            }
        }
    }
    Ok(Changelog { range: range.into(), commits, files, breaking })
}

fn area(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    match segs.len() {
        0 | 1 => "/".into(),
        2 => segs[0].into(),
        _ => format!("{}/{}", segs[0], segs[1]),
    }
}

pub fn render_changelog(c: &Changelog) -> String {
    let n: usize = c.files.iter().map(|f| f.changes.len()).sum();
    let mut s = format!("changelog {}: {} commits, {} files, {} symbol changes\n", c.range, c.commits.len(), c.files.len(), n);
    if !c.breaking.is_empty() {
        let _ = writeln!(s, "⚠ breaking ({}):", c.breaking.len());
        for (l, ext) in &c.breaking {
            let _ = writeln!(s, "  {l}{}", if *ext > 0 { format!("  ({ext} external refs)") } else { String::new() });
        }
    }
    let mut added: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut changed: BTreeMap<String, usize> = BTreeMap::new();
    let mut tests = 0;
    for f in &c.files {
        for ch in &f.changes {
            if ch.test {
                tests += 1;
                continue;
            }
            match ch.change {
                Change::Added => {
                    let name = ch.qualified.rsplit("::").take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("::");
                    let v = added.entry(area(&f.path)).or_default();
                    if !v.contains(&name) {
                        v.push(name); // header prototype + definition count once
                    }
                }
                Change::Body | Change::Doc => *changed.entry(area(&f.path)).or_default() += 1,
                _ => {}
            }
        }
    }
    if !added.is_empty() {
        let _ = writeln!(s, "added:");
        for (a, v) in &added {
            let shown: Vec<&str> = v.iter().take(12).map(String::as_str).collect();
            let more = if v.len() > 12 { format!(" +{}", v.len() - 12) } else { String::new() };
            let _ = writeln!(s, "  {a}: {}{more}", shown.join(", "));
        }
    }
    if !changed.is_empty() {
        let v: Vec<String> = changed.iter().map(|(a, n)| format!("{a} ({n})")).collect();
        let _ = writeln!(s, "changed bodies/docs: {}", v.join(", "));
    }
    if tests > 0 {
        let _ = writeln!(s, "tests: {tests} test symbols added/changed");
    }
    let other: Vec<&str> = c.files.iter().filter(|f| !f.semantic).map(|f| f.path.as_str()).collect();
    if !other.is_empty() {
        let _ = writeln!(s, "other files: {}", other.join(", "));
    }
    if !c.commits.is_empty() {
        let _ = writeln!(s, "commits:");
        for (id, msg, who) in c.commits.iter().take(30) {
            let _ = writeln!(s, "  {id} {msg} ({who})");
        }
    }
    s
}
