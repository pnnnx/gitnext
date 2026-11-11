//! `look`: one call that answers whatever the agent needs next, decided by the
//! shape of the target — a file or folder (outline), a symbol (everything about
//! it at once), an error (where it happened) or words (where they appear).
//!
//! Every agent step re-sends the whole system prompt, so one answer that saves a
//! follow-up call is worth far more than a shorter answer. For a symbol, `look`
//! therefore returns in one go: signature and doc, the code, who calls it, what
//! it calls, the tests that reach it with the command to run them, and the impact.

use crate::render::{self, Opts};
use crate::{agents, locate, Workspace};
use gitnext_index::Graph;
use std::fmt::Write as _;

/// `lite`: phrase hints for the small-model tool names (`replace(symbol, code)`).
pub fn look(ws: &Workspace, g: &Graph, what: &str, lite: bool) -> String {
    let root = &ws.root;
    let what = what.trim();
    let opts = Opts::default();
    if what.is_empty() || what == "." || what == "/" {
        let mut s = crate::insight::overview(ws.repo.as_ref(), g, root);
        s.push_str("next: look at a folder, file or function from above\n");
        return s;
    }
    // an error / traceback
    let errorish = what.contains('\n') && ["Traceback", "Error", "error", "panicked", "FAILED"].iter().any(|k| what.contains(k));
    if errorish || what.starts_with("Traceback") {
        let s = locate::render_locate(g, root, what, true);
        if !s.starts_with("no repo locations") {
            return s;
        }
    }
    // a file or folder (exact, or a unique file name like `utils.py`)
    let p = what.trim_start_matches("./").trim_end_matches('/');
    let file = g.files.iter().find(|f| f.path == p).map(|f| f.path.clone()).or_else(|| {
        let v: Vec<&str> = g.files.iter().filter(|f| f.path.ends_with(&format!("/{p}"))).map(|f| f.path.as_str()).collect();
        (v.len() == 1).then(|| v[0].to_string())
    });
    if let Some(f) = file {
        if g.files.iter().any(|x| x.path == f && x.parsed) {
            let mut s = render::outline(g, root, &f, Opts { tests: true, ..opts });
            s.push_str("next: look at several names from this list in one call\n");
            return s;
        }
        return std::fs::read_to_string(root.join(&f)).map(|t| agents::cap_tokens(&t, 1500)).unwrap_or_else(|e| format!("{f}: {e}\n"));
    }
    if g.files.iter().any(|f| f.path.starts_with(&format!("{p}/"))) {
        let mut s = render::outline(g, root, p, opts);
        if s.trim().is_empty() || s.starts_with("no parsed") {
            s = render::deps(g, p);
        }
        s.push_str("next: look at a file, or at several functions at once\n");
        return s;
    }
    // a symbol (also "class Foo", "def bar", "Foo.bar()")
    let words: Vec<&str> = what.split_whitespace().collect();
    let name = match words.as_slice() {
        [kw, n] if matches!(*kw, "class" | "def" | "fn" | "function" | "func" | "struct" | "method" | "trait" | "enum" | "type") => *n,
        _ => what,
    };
    let name = name.trim_end_matches("()").trim_end_matches(':');
    let ids = g.find(name);
    if !ids.is_empty() {
        let first = g.syms[ids[0]].sym.qualified.clone();
        if ids.iter().all(|&i| g.syms[i].sym.qualified == first) {
            return symbol(ws, g, &ids, lite);
        }
        let mut s = format!("{} things are called `{name}` — look at one by its full name (or several at once):\n", ids.len());
        for &i in ids.iter().take(12) {
            let r = &g.syms[i];
            let _ = writeln!(s, "  {} ({}) {}:{}", g.short_name(i), r.sym.kind.as_str(), r.file, r.sym.start_line);
        }
        return s;
    }
    words_hits(g, root, what)
}

/// Several targets in one call: each answered as by [`look`], separated.
pub fn look_many(ws: &Workspace, g: &Graph, targets: &[String], lite: bool) -> String {
    if targets.len() == 1 {
        return look(ws, g, &targets[0], lite);
    }
    targets.iter().map(|t| format!("=== {t}\n{}", look(ws, g, t, lite))).collect::<Vec<_>>().join("\n")
}

/// Everything about one symbol (ids = it and its declaration twins).
fn symbol(ws: &Workspace, g: &Graph, ids: &[usize], lite: bool) -> String {
    let root = &ws.root;
    let r = &g.syms[ids[0]];
    // a constructor is "called" by instantiating its class
    let i = match (&r.sym.parent, r.sym.kind) {
        (Some(owner), gitnext_parse::Kind::Method) if r.sym.name == "__init__" || owner.rsplit("::").next() == Some(r.sym.name.as_str()) => {
            g.find(owner).into_iter().find(|&c| g.syms[c].sym.kind.is_type()).unwrap_or(ids[0])
        }
        _ => ids[0],
    };
    let mut s = String::new();
    // the code (with doc comment and file:lines) — usually the reason for the call
    s.push_str(&render::read(g, root, ids));
    // who calls it: sure ones listed, guesses counted
    let callers = g.callers(i);
    let (tests, real): (Vec<_>, Vec<_>) = callers.iter().partition(|(c, _)| g.syms[*c].sym.test);
    let (sure, maybe): (Vec<_>, Vec<_>) = real.iter().partition(|(c, _)| !g.is_guess(*c, i));
    if !sure.is_empty() || !maybe.is_empty() {
        let list: Vec<String> = sure.iter().take(12).map(|(c, l)| format!("{} {}:{l}", g.short_name(*c), short_file(&g.syms[*c].file))).collect();
        let more = sure.len().saturating_sub(12);
        let _ = writeln!(
            s,
            "called by ({}): {}{}{}",
            sure.len(),
            if list.is_empty() { "—".to_string() } else { list.join(", ") },
            if more > 0 { format!(" +{more}") } else { String::new() },
            if maybe.is_empty() { String::new() } else { format!(" · maybe {} more via calls on objects of unknown type", maybe.len()) }
        );
    } else if r.sym.kind.is_callable() {
        let _ = writeln!(s, "called by: nothing in the repo{}", if tests.is_empty() { "" } else { " except tests" });
    }
    if r.sym.kind.is_callable() {
        let c = g.callees(ids[0]);
        if !c.resolved.is_empty() {
            let list: Vec<String> = c.resolved.iter().take(12).map(|(t, _)| g.short_name(*t).to_string()).collect();
            let _ = writeln!(s, "calls: {}{}", list.join(", "), if c.resolved.len() > 12 { format!(" +{}", c.resolved.len() - 12) } else { String::new() });
        }
        if let Some(l) = crate::libs::describe(root, g, ids[0]) {
            let _ = writeln!(s, "libraries: {l}");
        }
    }
    if r.sym.kind.is_type() {
        let h = g.hierarchy(ids[0]);
        if !h.extends.is_empty() {
            let _ = writeln!(s, "is a: {}", h.extends.join(", "));
        }
        if !h.implements.is_empty() {
            let _ = writeln!(s, "implements: {}", h.implements.join(", "));
        }
        let subs: Vec<&str> = h.subtypes.iter().chain(&h.implementors).map(|x| x.short.as_str()).collect();
        if !subs.is_empty() {
            let _ = writeln!(s, "extended / implemented by: {}", subs.join(", "));
        }
    }
    // tests that reach it, and how to run exactly those
    let t = locate::tests_for(g, i, 4);
    if t.is_empty() {
        let _ = writeln!(s, "tests: none reach it through the call graph");
    } else {
        // most likely to fail first: name those, one command for them and one for the rest
        let top = &t[..t.len().min(locate::TOP_TESTS)];
        let names: Vec<String> = top.iter().take(8).map(|x| g.short_name(x.symbol).to_string()).collect();
        let _ = writeln!(s, "tests ({}): {}{}", t.len(), names.join(", "), if t.len() > 8 { " …" } else { "" });
        for cmd in locate::run_commands(g, root, top).into_iter().take(2) {
            let _ = writeln!(s, "  run: {cmd}");
        }
        if top.len() < t.len() {
            if let Some(cmd) = locate::file_commands(g, root, &t).into_iter().next() {
                let _ = writeln!(s, "  all of them: {cmd}");
            }
        }
    }
    // what a change would touch
    let imp = g.impact(i, 3);
    let sure: Vec<_> = imp.iter().filter(|n| n.certain && !n.test).collect();
    let maybe = imp.iter().filter(|n| !n.certain).count();
    let mut files: Vec<&str> = sure.iter().map(|n| n.symbol.file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let _ = writeln!(s, "changing it affects: {} symbols in {} files{}", sure.len(), files.len(), if maybe > 0 { format!(" (+{maybe} uncertain)") } else { String::new() });
    let short = g.short_name(ids[0]);
    if lite {
        let _ = writeln!(s, "to change it: replace(symbol=\"{short}\", code=<whole new definition>)");
    } else {
        let _ = writeln!(s, "next: edit replace {short} · rename {short} <new> · impact {short} for the full tree");
    }
    s
}

fn short_file(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn words_hits(g: &Graph, root: &std::path::Path, what: &str) -> String {
    let mut s = String::new();
    let hits = g.search(what, 10);
    if !hits.is_empty() {
        s.push_str("functions and classes:\n");
        for i in hits {
            let r = &g.syms[i];
            let sum = r.sym.summary.as_deref().map(|x| format!(" — {x}")).unwrap_or_default();
            let _ = writeln!(s, "  {} ({}) {}:{}{sum}", g.short_name(i), r.sym.kind.as_str(), r.file, r.sym.start_line);
        }
    }
    let low = what.to_lowercase();
    let mut text_hits = Vec::new();
    'files: for f in g.files.iter().filter(|f| f.lines < 20_000) {
        let Ok(t) = std::fs::read_to_string(root.join(&f.path)) else { continue };
        for (n, line) in t.lines().enumerate() {
            if line.to_lowercase().contains(&low) {
                text_hits.push(format!("  {}:{}: {}", f.path, n + 1, line.trim().chars().take(120).collect::<String>()));
                if text_hits.len() >= 20 {
                    break 'files;
                }
            }
        }
    }
    if !text_hits.is_empty() {
        s.push_str("text:\n");
        s.push_str(&text_hits.join("\n"));
        s.push('\n');
    }
    if s.is_empty() {
        s = format!("nothing found for `{what}`. Try a file name, a function name or other words; look . shows the whole project\n");
    }
    s
}
