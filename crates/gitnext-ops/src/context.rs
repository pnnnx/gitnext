//! `context`: pick the code a task is about and hand it over in one block.
//!
//! Small local models (3–14B) rarely call tools, even when told to; what they
//! do use is text already in the prompt. So instead of waiting for the model
//! to `look`, the agent runs `gitnext context "<task>"` itself before sending
//! the request (loca's `before_prompt` hook) and the model starts with the
//! relevant definitions, who uses them and which tests reach them.
//!
//! Picking is plain and predictable — no embeddings:
//! - names in the task that exist in the index (`estimate_tokens`, `LoadTools`,
//!   `Agent.run`) → their code, and every place they are used;
//! - files in the task (`tests/test_brief.py`, `agent.py`) → for a test file,
//!   the code its tests call; for other files, an outline;
//! - a traceback → the code where it happened;
//! - "the test fails" with nothing named → the tests and the code they call.
//!
//! Nothing found → empty output, so the hook adds nothing to the prompt.

use crate::render::{self, Opts};
use crate::{locate, Workspace};
use gitnext_index::Graph;
use gitnext_parse::Kind;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// Default size of the block, in tokens (≈ 4 chars each): fits next to a
/// system prompt and a few turns in a 16k context.
pub const DEFAULT_BUDGET: usize = 2000;

const STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "file", "files", "code", "test", "tests", "function",
    "functions", "class", "method", "fix", "add", "make", "find", "where", "which", "what", "why", "how", "does", "should",
    "rename", "change", "changes", "remove", "delete", "update", "write", "read", "only", "path", "name", "new", "old",
    "all", "any", "not", "use", "used", "fails", "failing", "error", "bug", "project", "repo", "answer", "return", "returns",
    "python3", "python", "cargo", "pytest", "unittest", "run", "true", "false", "none", "self", "str", "int", "list", "dict",
];

#[derive(Default)]
struct Pick {
    score: f32,
    why: Vec<&'static str>,
}

/// Picked code for `task`, at most ~`budget` tokens. Empty when nothing matches.
pub fn context(ws: &Workspace, g: &Graph, task: &str, budget: usize) -> String {
    let root = &ws.root;
    let budget = budget.max(200);
    let mut picks: HashMap<usize, Pick> = HashMap::new();
    let add = |picks: &mut HashMap<usize, Pick>, i: usize, s: f32, why: &'static str| {
        let r = &g.syms[i].sym;
        if r.kind == Kind::Module && r.start_line <= 1 {
            return;
        }
        let p = picks.entry(i).or_default();
        p.score += s;
        if !p.why.contains(&why) {
            p.why.push(why);
        }
    };
    let mut head = String::new();

    // a traceback / compiler error: where it happened comes first
    let errorish = task.lines().count() > 1 && ["Traceback", "Error", "error[", "panicked", "FAILED", "line "].iter().any(|k| task.contains(k));
    if errorish {
        let s = locate::render_locate(g, root, task, false);
        if !s.starts_with("no repo locations") {
            head = crate::agents::cap_tokens(&s, budget / 3);
        }
    }

    // files named in the task
    let mut outline_files: Vec<String> = Vec::new();
    for tok in task.split(|c: char| c.is_whitespace() || "`'\"«»(),;:<>[]{}".contains(c)) {
        let t = tok.trim_matches(|c: char| c == '.' || c == '/').trim_start_matches("./");
        if !t.contains('.') && !t.contains('/') {
            continue;
        }
        let Some(file) = resolve_file(g, t) else { continue };
        let in_file = g.in_file(&file);
        let tests: Vec<usize> = in_file.iter().copied().filter(|&i| g.syms[i].sym.test && g.syms[i].sym.kind.is_callable()).collect();
        if !tests.is_empty() {
            // a test file: what its tests exercise is what the task is about
            for &t in &tests {
                for (c, _) in g.callees(t).resolved {
                    if !g.syms[c].sym.test {
                        add(&mut picks, c, 6.0, "called by the test");
                    }
                }
                add(&mut picks, t, 1.5, "the test");
            }
        } else if !outline_files.contains(&file) {
            // shown whole or as an outline after the picked symbols
            outline_files.push(file);
        }
    }

    // names that exist in the index; names that do not (a rename target)
    let mut seen = HashSet::new();
    let mut named: Vec<(String, String)> = Vec::new(); // (as written, qualified)
    let mut unknown: Vec<String> = Vec::new();
    for tok in identifiers(task) {
        if !seen.insert(tok.clone()) {
            continue;
        }
        let low = tok.to_lowercase();
        let codey = tok.contains('_') || tok.contains('.') || tok.contains("::") || tok.chars().skip(1).any(|c| c.is_uppercase());
        if tok.len() < 3 || (!codey && STOP.contains(&low.as_str())) {
            continue;
        }
        let ids = g.find(&tok);
        let distinct: HashSet<&str> = ids.iter().map(|&i| g.syms[i].sym.qualified.as_str()).collect();
        if ids.is_empty() && (codey || tok.len() >= 4) && !STOP.contains(&low.as_str()) {
            unknown.push(tok.clone());
        }
        if !ids.is_empty() && distinct.len() <= 4 {
            if distinct.len() == 1 {
                named.push((tok.clone(), g.syms[ids[0]].sym.qualified.clone()));
            }
            let share = 10.0 / distinct.len() as f32;
            let mut done = HashSet::new();
            for &i in &ids {
                if done.insert(g.syms[i].sym.qualified.as_str()) {
                    add(&mut picks, i, share, "named in the task");
                }
            }
        } else if codey && ids.is_empty() {
            for i in g.search(&tok, 3) {
                add(&mut picks, i, 2.0, "name looks like it");
            }
        }
    }

    // "the test fails" and nothing named: the tests and what they call (small repos)
    let low = task.to_lowercase();
    if picks.is_empty() && head.is_empty() && ["тест", "test"].iter().any(|w| low.contains(w)) {
        let tests: Vec<usize> = (0..g.syms.len()).filter(|&i| g.syms[i].sym.test && g.syms[i].sym.kind.is_callable()).take(12).collect();
        for &t in &tests {
            add(&mut picks, t, 1.0, "a test");
            for (c, _) in g.callees(t).resolved {
                if !g.syms[c].sym.test {
                    add(&mut picks, c, 3.0, "called by a test");
                }
            }
        }
    }

    // nothing named: the best matches by words (names, docs, paths)
    if picks.is_empty() && head.is_empty() && outline_files.is_empty() {
        for (n, it) in crate::context(ws, g, task, budget).items.iter().filter(|it| it.reason.starts_with("matches")).take(6).enumerate() {
            if let Some(&i) = g.find(&it.qualified).iter().find(|&&i| g.syms[i].file == it.file && g.syms[i].sym.start_line == it.lines.0) {
                add(&mut picks, i, if n < 2 { 6.0 } else { 3.0 }, "words match");
            }
        }
    }
    if picks.is_empty() && head.is_empty() && outline_files.is_empty() {
        return String::new();
    }

    // a method picked together with its class: keep the class, drop the method
    let picked: HashSet<String> = picks.keys().map(|&i| g.syms[i].sym.qualified.clone()).collect();
    let mut order: Vec<(usize, Pick)> = picks
        .into_iter()
        .filter(|(i, p)| !(p.score < 6.0 && g.syms[*i].sym.parent.as_ref().is_some_and(|o| picked.contains(o))))
        .collect();
    order.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(g.syms[a.0].file.cmp(&g.syms[b.0].file)).then(g.syms[a.0].sym.start_line.cmp(&g.syms[b.0].sym.start_line)));

    let mut out = String::from("[gitnext] Code related to this task, picked from the index (may be incomplete; read files for more):\n");
    // a rename: one command does all of it — small models otherwise edit, grep and re-check for dozens of steps
    let renaming = ["переимен", "rename"].iter().any(|w| low.contains(w));
    if renaming && named.len() == 1 {
        let old = &named[0].0;
        let new_name = unknown.iter().map(String::as_str).find(|n| n != old).unwrap_or("<new_name>");
        let _ = writeln!(
            out,
            "To rename `{old}` everywhere in one step (definition, calls, imports, tests, __all__), run in the shell:\n  gitnext rename {old} {new_name} --text\n(--text also renames it in comments and strings). It prints every place it changed. Then run the tests once and finish."
        );
    }
    out.push_str(&head);
    let mut used = tokens(&out);
    let mut rest: Vec<String> = Vec::new();
    let mut shown = 0usize;
    for (n, (i, p)) in order.iter().enumerate() {
        let r = &g.syms[*i];
        let strong = p.score >= 6.0;
        let mut block = String::new();
        let code = render::read(g, root, &[*i]);
        let code_tokens = tokens(&code);
        let left = budget.saturating_sub(used);
        // a test from a named test file shows what is expected: its code, when short
        let small_test = (p.why.contains(&"the test") || p.why.contains(&"a test")) && code_tokens <= 250;
        if (strong || small_test) && code_tokens <= left.saturating_sub(60) && code_tokens <= budget * 3 / 4 {
            block.push_str(&code);
        } else if strong || (n < 8 && left > 120) {
            let lines = r.sym.end_line.saturating_sub(r.sym.start_line) + 1;
            let _ = writeln!(block, "{}:{}-{} {}{}", r.file, r.sym.start_line, r.sym.end_line, r.sym.signature.chars().take(160).collect::<String>(), if lines > 3 { format!(" — {lines} lines, not shown") } else { String::new() });
        } else {
            rest.push(format!("{} {}:{}", g.short_name(*i), short(&r.file), r.sym.start_line));
            continue;
        }
        if strong {
            block.push_str(&used_in(g, *i));
            if let Some(l) = crate::libs::describe(root, g, *i) {
                let _ = writeln!(block, "libraries: {l}");
            }
            if p.why.contains(&"named in the task") {
                block.push_str(&text_mentions(g, root, *i));
            }
            if !r.sym.test {
                let t = locate::tests_for_at(g, root, *i, 3).0;
                if !t.is_empty() {
                    let names: Vec<&str> = t.iter().take(5).map(|x| g.short_name(x.symbol)).collect();
                    let _ = writeln!(block, "tests: {}{}", names.join(", "), if t.len() > 5 { format!(" +{}", t.len() - 5) } else { String::new() });
                    if let Some(cmd) = locate::run_commands(g, root, &t[..t.len().min(locate::TOP_TESTS)]).into_iter().next() {
                        let _ = writeln!(block, "  run: {}", cmd.chars().take(200).collect::<String>());
                    }
                }
            }
        }
        if used + tokens(&block) > budget {
            rest.push(format!("{} {}:{}", g.short_name(*i), short(&r.file), r.sym.start_line));
            continue;
        }
        used += tokens(&block);
        out.push_str(&block);
        shown += 1;
    }
    for f in &outline_files {
        let left = budget.saturating_sub(used);
        if left <= 100 {
            break;
        }
        // a small named file: whole (the bug may be in a string, not a signature); else its outline
        let text = std::fs::read_to_string(root.join(f)).unwrap_or_default();
        let block = if tokens(&text) + 20 <= left.min(budget / 2) {
            let fence = gitnext_parse::lang_for(std::path::Path::new(f)).map(|l| l.fence()).unwrap_or("");
            format!("{f} (whole file):\n```{fence}\n{}\n```\n", text.trim_end())
        } else {
            format!("outline of {f}:\n{}", crate::agents::cap_tokens(&render::outline(g, root, f, Opts::default()), left))
        };
        used += tokens(&block);
        out.push_str(&block);
    }
    if !rest.is_empty() {
        let _ = writeln!(out, "also related: {}", rest.iter().take(12).cloned().collect::<Vec<_>>().join(", "));
    }
    if shown == 0 && head.is_empty() && outline_files.is_empty() {
        return String::new();
    }
    out
}

/// Every place the symbol is referenced, grouped by file: what a rename or a
/// signature change has to touch.
fn used_in(g: &Graph, i: usize) -> String {
    let mut by_file: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    let mut maybe = 0;
    for &(src, _, line) in g.referrers(i) {
        if src == i {
            continue;
        }
        if g.is_guess(src, i) {
            maybe += 1;
            continue;
        }
        by_file.entry(g.syms[src].file.as_str()).or_default().push(line);
    }
    if by_file.is_empty() {
        return if g.syms[i].sym.kind.is_callable() { "used in: nothing else in the repo\n".into() } else { String::new() };
    }
    let total: usize = by_file.values().map(Vec::len).sum();
    let parts: Vec<String> = by_file
        .iter_mut()
        .take(10)
        .map(|(f, ls)| {
            ls.sort_unstable();
            ls.dedup();
            format!("{f}:{}", ls.iter().take(6).map(u32::to_string).collect::<Vec<_>>().join(","))
        })
        .collect();
    format!(
        "used in ({total}): {}{}{}\n",
        parts.join("; "),
        if by_file.len() > 10 { format!("; +{} files", by_file.len() - 10) } else { String::new() },
        if maybe > 0 { format!(" · maybe {maybe} more") } else { String::new() }
    )
}

/// Lines that mention the name as a word but are not references in the graph
/// (comments, strings, docs): what a rename leaves behind unless handled.
fn text_mentions(g: &Graph, root: &std::path::Path, i: usize) -> String {
    let name = g.syms[i].sym.name.as_str();
    if name.len() < 4 {
        return String::new();
    }
    let mut known: HashSet<(&str, u32)> = g.referrers(i).iter().map(|&(src, _, l)| (g.syms[src].file.as_str(), l)).collect();
    known.insert((g.syms[i].file.as_str(), g.syms[i].sym.start_line));
    let word = |line: &str| {
        line.match_indices(name).any(|(k, _)| {
            let before = line[..k].chars().next_back();
            let after = line[k + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    };
    let mut hits = Vec::new();
    let generated = |p: &str| p.starts_with(".gitnext/") || matches!(p, "AGENTS.md" | "CLAUDE.md" | "LOCA.md");
    for f in g.files.iter().filter(|f| f.lines < 20_000 && !generated(&f.path)) {
        let Ok(t) = std::fs::read_to_string(root.join(&f.path)) else { continue };
        if !t.contains(name) {
            continue;
        }
        for (n, line) in t.lines().enumerate() {
            let ln = n as u32 + 1;
            if word(line) && !known.contains(&(f.path.as_str(), ln)) {
                hits.push(format!("{}:{ln}", f.path));
            }
        }
    }
    if hits.is_empty() {
        return String::new();
    }
    let more = hits.len().saturating_sub(8);
    hits.truncate(8);
    format!("also as text (comments, strings): {}{}\n", hits.join(", "), if more > 0 { format!(" +{more}") } else { String::new() })
}

fn resolve_file(g: &Graph, t: &str) -> Option<String> {
    if let Some(f) = g.files.iter().find(|f| f.path == t) {
        return Some(f.path.clone());
    }
    let v: Vec<&str> = g.files.iter().filter(|f| f.path.ends_with(&format!("/{t}"))).map(|f| f.path.as_str()).collect();
    (v.len() == 1).then(|| v[0].to_string())
}

/// ASCII identifiers, keeping `a.b` / `a::b` together (trailing `()` dropped).
fn identifiers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut k = 0;
    while k <= chars.len() {
        let c = chars.get(k).copied().unwrap_or(' ');
        let joins = (c == '.' || c == ':') && !cur.is_empty() && chars.get(k + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == '_' || *n == ':');
        if c.is_ascii_alphanumeric() || c == '_' || joins {
            cur.push(c);
        } else if !cur.is_empty() {
            let t = cur.trim_matches(|c: char| c == '.' || c == ':').to_string();
            if t.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
                // a file name is handled as a file, not a symbol
                if !t.rsplit('.').next().is_some_and(|e| matches!(e, "py" | "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "md" | "toml" | "json")) {
                    out.push(t);
                }
            }
            cur.clear();
        }
        k += 1;
    }
    out
}

fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn short(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::identifiers;

    #[test]
    fn picks_code_names() {
        let v = identifiers("Переименуй estimate_tokens в approx_tokens; см. Agent.run() и tests/test_brief.py");
        assert!(v.contains(&"estimate_tokens".to_string()));
        assert!(v.contains(&"Agent.run".to_string()));
        assert!(!v.iter().any(|t| t.ends_with(".py")));
    }
}
