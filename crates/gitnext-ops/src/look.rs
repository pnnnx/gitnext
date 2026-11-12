//! `look`: one call that answers whatever the agent needs next, decided by the
//! shape of the target — a file or folder (outline), a symbol (everything about
//! it at once), a place in a file (`src/eval.rs:62`, `tests/t.py:120-154`,
//! `src/eval.rs::matches_greater`), an error (where it happened) or words (where they appear).
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
    // a shell habit: `tests/test_x.py | grep -A 20 name` — the file is what was asked for
    let what = match what.split_once(" | ") {
        Some((a, _)) if file_of(g, a.trim()).is_some() => a.trim(),
        _ => what,
    };
    // a place in a file, the way tracebacks, test runners and models write it
    if let Some(s) = place(ws, g, what, lite) {
        return s;
    }
    // a file or folder (exact, or a unique file name like `utils.py`)
    let p = what.trim_start_matches("./").trim_end_matches('/');
    if let Some(f) = file_of(g, what) {
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
    let mut ids = g.find(name);
    // a bare name that is also somebody's full name (C `main`) must not hide its namesakes
    // (`run::main`, `cli::main`): picking one silently sends the agent to the wrong code
    if !ids.is_empty() && !name.contains("::") && !name.contains('.') && !name.contains('@') {
        for i in g.named(name) {
            let k = g.syms[i].sym.kind;
            if !ids.contains(&i) && (k.is_callable() || k.is_type()) {
                ids.push(i);
            }
        }
    }
    if !ids.is_empty() {
        return one_or_choice(ws, g, name, &ids, lite);
    }
    words_hits(g, root, what)
}

/// The symbol, or — when the ids are different things with one name — the list to pick from.
fn one_or_choice(ws: &Workspace, g: &Graph, name: &str, ids: &[usize], lite: bool) -> String {
    let first = &g.syms[ids[0]].sym.qualified;
    if ids.iter().all(|&i| &g.syms[i].sym.qualified == first) {
        return symbol(ws, g, ids, lite);
    }
    let mut s = format!("{} things are called `{name}` — look at one by its full name (or several at once):\n", ids.len());
    for &i in ids.iter().take(12) {
        let r = &g.syms[i];
        // a name that would only bring this list back is pinned by its line: `main@78`
        let short = g.short_name(i);
        let label = if g.find(short).iter().all(|&x| g.syms[x].sym.qualified == r.sym.qualified) && short != name { short.to_string() } else { format!("{}@{}", r.sym.name, r.sym.start_line) };
        let _ = writeln!(s, "  {label} ({}) {}:{}", r.sym.kind.as_str(), r.file, r.sym.start_line);
    }
    s
}

/// The repo file `p` names: its path, or a file name / path tail that only one file has (`utils.py`).
fn file_of(g: &Graph, p: &str) -> Option<String> {
    let p = p.trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() {
        return None;
    }
    g.files.iter().find(|f| f.path == p).map(|f| f.path.clone()).or_else(|| {
        let v: Vec<&str> = g.files.iter().filter(|f| f.path.ends_with(&format!("/{p}"))).map(|f| f.path.as_str()).collect();
        (v.len() == 1).then(|| v[0].to_string())
    })
}

/// Lines of a file the way they get written: `62`, `L62`, `120-154`, `L120-L154`.
fn line_range(s: &str) -> Option<(usize, usize)> {
    let num = |x: &str| x.trim().trim_start_matches(['L', 'l']).parse::<usize>().ok().filter(|&n| n > 0);
    match s.split_once('-') {
        Some((a, b)) => Some((num(a)?, num(b)?)).filter(|(a, b)| a <= b),
        None => num(s).map(|n| (n, n)),
    }
}

/// Most lines one answer shows; a longer range says how to ask for the rest.
const RANGE_LINES: usize = 200;

/// A place in a file: `src/eval.rs:62` (the function around the line), `tests/t.py:120-154`
/// (those lines), `src/eval.rs::matches_greater`, `tests/t.py::TestA::test_b[case]`,
/// `eval.rs matches_greater`, `pad src/display.rs`. None: `what` is not written that way.
fn place(ws: &Workspace, g: &Graph, what: &str, lite: bool) -> Option<String> {
    let words: Vec<&str> = what.split_whitespace().collect();
    // written as a path (`eval.rs`, `src/eval`): a bare word is a name, even if a file is called that
    let path = |p: &str| if p.contains(['.', '/']) { file_of(g, p) } else { None };
    let (file, rest) = match words.as_slice() {
        [one] => {
            let (a, b) = one.split_once(':')?;
            (path(a)?, b.trim_start_matches(':'))
        }
        [a, b] => match (path(a), path(b)) {
            (Some(f), None) => (f, *b),
            (None, Some(f)) => (f, *a),
            _ => return None,
        },
        _ => return None,
    };
    let rest = rest.trim_end_matches("()").trim_end_matches(':');
    if rest.is_empty() {
        return None;
    }
    let inside = |a: usize, b: usize| -> Vec<usize> {
        (0..g.syms.len())
            .filter(|&i| {
                let s = &g.syms[i].sym;
                g.syms[i].file == file && (s.kind.is_callable() || s.kind.is_type()) && s.start_line as usize <= b && s.end_line as usize >= a
            })
            .collect()
    };
    if let Some((a, b)) = line_range(rest) {
        // one line: the function it is in, with everything `look` says about a function
        if a == b {
            if let Some(i) = inside(a, a).into_iter().min_by_key(|&i| g.syms[i].sym.end_line - g.syms[i].sym.start_line) {
                return Some(symbol(ws, g, &[i], lite));
            }
        }
        let text = std::fs::read_to_string(ws.root.join(&file)).ok()?;
        let lines: Vec<&str> = text.lines().collect();
        // one line outside any function: the lines around it
        let (a, b) = if a == b { (a.saturating_sub(10).max(1), a + 20) } else { (a, b) };
        if a > lines.len() {
            return Some(format!("{file} has {} lines\n", lines.len()));
        }
        let end = b.min(lines.len()).min(a + RANGE_LINES - 1);
        let fence = gitnext_parse::lang_for(std::path::Path::new(&file)).map(|l| l.fence()).unwrap_or("");
        let mut s = format!("{file}:{a}-{end} (of {} lines)\n```{fence}\n", lines.len());
        for l in &lines[a - 1..end] {
            let _ = writeln!(s, "{l}");
        }
        s.push_str("```\n");
        if end < b.min(lines.len()) {
            let _ = writeln!(s, "… cut at {RANGE_LINES} lines: ask for {file}:{}-{} for the rest", end + 1, b.min(lines.len()));
        }
        let mut owners = inside(a, end);
        owners.sort_by_key(|&i| g.syms[i].sym.start_line);
        if !owners.is_empty() {
            let list: Vec<String> = owners.iter().take(8).map(|&i| format!("{} ({}-{})", g.short_name(i), g.syms[i].sym.start_line, g.syms[i].sym.end_line)).collect();
            let _ = writeln!(s, "these lines are in: {}{}", list.join(", "), if owners.len() > 8 { " …" } else { "" });
            let _ = writeln!(s, "{}", if lite { "to change one: replace(symbol=<its name>, code=<whole new definition>)" } else { "next: look at one by name for its callers and tests" });
        }
        return Some(s);
    }
    // a name in that file; a test id keeps its class and loses its case: `TestA::test_b[case]`
    let name = rest.split('[').next().unwrap_or(rest).replace('.', "::");
    let last = name.rsplit("::").next().unwrap_or(&name);
    let here = |v: Vec<usize>| -> Vec<usize> { v.into_iter().filter(|&i| g.syms[i].file == file).collect() };
    let mut ids = here(g.find(&name));
    if ids.is_empty() {
        ids = here(g.named(last));
    }
    if ids.is_empty() {
        let outline = render::outline(g, &ws.root, &file, Opts { tests: true, ..Opts::default() });
        return Some(format!("no `{rest}` in {file} — what is there:\n{outline}"));
    }
    Some(one_or_choice(ws, g, last, &ids, lite))
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
        // how far to trust the list: measured share of real callers among the sure ones;
        // a small model is not shown the doubtful ones at all
        let trust = crate::trust::callers_sure(&r.file);
        let _ = writeln!(
            s,
            "called by ({}{}): {}{}{}",
            sure.len(),
            if trust.is_empty() || sure.is_empty() { String::new() } else { format!(", {trust}") },
            if list.is_empty() { "—".to_string() } else { list.join(", ") },
            if more > 0 { format!(" +{more}") } else { String::new() },
            if maybe.is_empty() || lite { String::new() } else { format!(" · maybe {} more via calls on objects of unknown type", maybe.len()) }
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
    let (t, source) = locate::tests_for_at(g, root, i, 4);
    if t.is_empty() {
        let _ = writeln!(s, "tests: none reach it through the call graph");
    } else {
        // most likely to fail first: name those, one command for them and one for the rest
        let top = &t[..t.len().min(locate::TOP_TESTS)];
        let names: Vec<String> = top.iter().take(8).map(|x| g.short_name(x.symbol).to_string()).collect();
        let trust = if source == locate::Source::Graph { crate::trust::tests_short(&r.file) } else { "; from a coverage run".to_string() };
        let _ = writeln!(s, "tests ({}{trust}): {}{}", t.len(), names.join(", "), if t.len() > 8 { " …" } else { "" });
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

/// Lines that contain `what` (any text: a string literal, an operator, a message), each with
/// the function it sits in — "who builds this string" answered without grep.
pub fn text_hits(g: &Graph, root: &std::path::Path, what: &str, limit: usize) -> Vec<String> {
    let low = what.to_lowercase();
    let mut hits = Vec::new();
    'files: for f in g.files.iter().filter(|f| f.lines < 20_000) {
        let Ok(t) = std::fs::read_to_string(root.join(&f.path)) else { continue };
        for (n, line) in t.lines().enumerate() {
            if line.to_lowercase().contains(&low) {
                let ln = n as u32 + 1;
                // the innermost function / type around the line
                let owner = (0..g.syms.len())
                    .filter(|&i| g.syms[i].file == f.path && g.syms[i].sym.start_line <= ln && g.syms[i].sym.end_line >= ln && (g.syms[i].sym.kind.is_callable() || g.syms[i].sym.kind.is_type()))
                    .min_by_key(|&i| g.syms[i].sym.end_line - g.syms[i].sym.start_line);
                let within = owner.map(|i| format!(" (in {})", g.short_name(i))).unwrap_or_default();
                hits.push(format!("  {}:{ln}{within}: {}", f.path, line.trim().chars().take(120).collect::<String>()));
                if hits.len() >= limit {
                    break 'files;
                }
            }
        }
    }
    hits
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
    let text_hits = text_hits(g, root, what, 20);
    if !text_hits.is_empty() {
        s.push_str("text:\n");
        s.push_str(&text_hits.join("\n"));
        s.push('\n');
    }
    if s.is_empty() {
        // several words and no line has them all: the ones that are names are still an answer
        let mut seen = std::collections::HashSet::new();
        let names: Vec<usize> = what
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|w| w.len() > 2)
            .flat_map(|w| g.find(w).into_iter().take(2))
            .filter(|&i| (g.syms[i].sym.kind.is_callable() || g.syms[i].sym.kind.is_type()) && seen.insert(i))
            .take(10)
            .collect();
        if what.split_whitespace().count() > 1 && !names.is_empty() {
            s.push_str("nothing has all these words; the ones that are names — look at one:\n");
            for i in names {
                let r = &g.syms[i];
                let _ = writeln!(s, "  {} ({}) {}:{}", g.short_name(i), r.sym.kind.as_str(), r.file, r.sym.start_line);
            }
            return s;
        }
        s = format!("nothing found for `{what}`. Try a function name, a file name, lines of a file (`src/app.py:120-160`) or other words; look . shows the whole project\n");
    }
    s
}
