//! `gitnext why <test>`: a test fails — run it and answer in one step what an agent otherwise
//! needs four for (find the test, run it, read a screen of output, guess where to look):
//! the failure in a few lines, and the code the test reaches, most likely culprit first.
//!
//! A small model that is told "the test `test_greater_than` fails" asks for `greater_than`,
//! finds nothing and invents a function. Here the path from the test to the code is the answer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use gitnext_index::Graph;
use gitnext_parse::EdgeKind;

use crate::check::{self, Step};
use crate::locate::{self, TestRef};

/// Code the tests reach is listed up to this many functions; the code itself is shown for the first.
const LISTED: usize = 8;
const SHOWN: usize = 2;
/// Lines of code shown per function.
const CODE_LINES: usize = 45;
/// How far from the test a function may be (calls between them).
const DEPTH: usize = 6;

pub struct Suspect {
    pub symbol: usize,
    /// calls between the test and this function (1 = the test calls it itself)
    pub depth: usize,
    /// words its name shares with the test's name (`test_greater_than` — `matches_greater`)
    pub overlap: usize,
    /// the failure's own output names a line inside it: position among those lines, first = 0
    pub in_failure: Option<usize>,
}

pub struct Why {
    pub tests: Vec<usize>,
    pub steps: Vec<Step>,
    pub suspects: Vec<Suspect>,
}

impl Why {
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|s| !s.ok)
    }
}

/// Runnable tests by name. `greater_than` finds `test_greater_than` / `TestGreaterThan` when no
/// test has exactly that name; at most three, the shortest names first.
pub fn find_tests(g: &Graph, name: &str) -> Vec<usize> {
    let runnable = |i: &usize| g.syms[*i].sym.test && locate::is_runnable(g, *i);
    let exact: Vec<usize> = g.find(name).into_iter().filter(runnable).collect();
    if !exact.is_empty() {
        return exact.into_iter().take(3).collect();
    }
    let bare = name.rsplit("::").next().unwrap_or(name).rsplit('.').next().unwrap_or(name);
    let want: Vec<String> = locate::name_words(bare).into_iter().filter(|w| w != "test" && w != "tests").collect();
    if want.is_empty() {
        return vec![];
    }
    let mut hits: Vec<usize> = (0..g.syms.len())
        .filter(runnable)
        .filter(|&i| {
            let have: Vec<String> = locate::name_words(&g.syms[i].sym.name).into_iter().filter(|w| w != "test" && w != "tests").collect();
            have == want
        })
        .collect();
    hits.sort_by_key(|&i| (g.syms[i].sym.name.len(), g.syms[i].file.clone()));
    hits.truncate(3);
    hits
}

/// Tests whose names are close to `name`, to offer when none matched.
pub fn similar_tests(g: &Graph, name: &str) -> Vec<usize> {
    let want: HashSet<String> = locate::name_words(name).into_iter().filter(|w| w != "test").collect();
    let mut v: Vec<(usize, usize)> = (0..g.syms.len())
        .filter(|&i| g.syms[i].sym.test && locate::is_runnable(g, i))
        .map(|i| (locate::name_words(&g.syms[i].sym.name).iter().filter(|w| want.contains(*w)).count(), i))
        .filter(|(n, _)| *n > 0)
        .collect();
    v.sort_by_key(|&(n, i)| (std::cmp::Reverse(n), g.syms[i].sym.name.len()));
    v.into_iter().take(5).map(|(_, i)| i).collect()
}

/// Non-test functions reachable from the tests through calls, with the shortest distance.
fn reached(g: &Graph, tests: &[usize]) -> Vec<(usize, usize)> {
    let mut forward: HashMap<usize, Vec<usize>> = HashMap::new();
    for t in 0..g.syms.len() {
        for &(src, kind, _) in g.referrers(t) {
            if kind == EdgeKind::Calls {
                forward.entry(src).or_default().push(t);
            }
        }
    }
    let mut depth: HashMap<usize, usize> = tests.iter().map(|&t| (t, 0)).collect();
    let mut queue: VecDeque<usize> = tests.iter().copied().collect();
    while let Some(i) = queue.pop_front() {
        let d = depth[&i];
        if d >= DEPTH || depth.len() > 4000 {
            continue;
        }
        for &n in forward.get(&i).map(Vec::as_slice).unwrap_or(&[]) {
            if let std::collections::hash_map::Entry::Vacant(e) = depth.entry(n) {
                e.insert(d + 1);
                queue.push_back(n);
            }
        }
    }
    let mut out: Vec<(usize, usize)> = depth.into_iter().filter(|&(i, d)| d > 0 && !g.syms[i].sym.test && g.syms[i].sym.kind.is_callable() && !g.syms[i].sym.decl).collect();
    out.sort_unstable();
    out
}

/// Run the tests; when they fail, rank the code they reach.
pub fn why(root: &Path, g: &Graph, tests: &[usize], timeout: Duration) -> Why {
    let refs: Vec<TestRef> = tests.iter().map(|&t| TestRef { symbol: t, how: "asked".into() }).collect();
    let steps: Vec<Step> = check::test_commands(g, root, &refs).iter().map(|c| check::run(root, g, "tests", c, timeout)).collect();
    explain(root, g, tests, steps)
}

/// The same from runs already made: which code to look at, given how the tests failed.
pub fn explain(root: &Path, g: &Graph, tests: &[usize], steps: Vec<Step>) -> Why {
    let mut w = Why { tests: tests.to_vec(), steps, suspects: Vec::new() };
    if !w.failed() {
        return w;
    }
    // lines of non-test code the output names: the digest's own locations first, then the rest (stack frames)
    let mut named: Vec<usize> = Vec::new();
    let mut add = |i: Option<usize>| {
        if let Some(i) = i.filter(|&i| !g.syms[i].sym.test && g.syms[i].sym.kind.is_callable()) {
            if !named.contains(&i) {
                named.push(i);
            }
        }
    };
    for st in w.steps.iter().filter(|s| !s.ok) {
        for f in &st.digest.failures {
            if let (Some(p), Some(n)) = (&f.file, f.line) {
                add(locate::match_file(g, root, p).and_then(|file| locate::innermost(g, &file, n)));
            }
        }
        for fr in &st.frames {
            add(fr.symbol);
        }
    }
    let test_words: HashSet<String> =
        tests.iter().flat_map(|&t| locate::name_words(&g.syms[t].sym.name)).filter(|x| !matches!(x.as_str(), "test" | "tests" | "should" | "it" | "the" | "a")).collect();
    let overlap = |i: usize| -> usize {
        let s = &g.syms[i].sym;
        let mut words: HashSet<String> = locate::name_words(&s.name).into_iter().collect();
        if let Some(p) = &s.parent {
            words.extend(locate::name_words(p.rsplit("::").next().unwrap_or(p)));
        }
        words.iter().filter(|x| test_words.contains(*x)).count()
    };
    let mut by: HashMap<usize, Suspect> = HashMap::new();
    for (i, d) in reached(g, tests) {
        by.insert(i, Suspect { symbol: i, depth: d, overlap: overlap(i), in_failure: None });
    }
    for (pos, &i) in named.iter().enumerate() {
        by.entry(i).or_insert(Suspect { symbol: i, depth: DEPTH + 1, overlap: overlap(i), in_failure: None }).in_failure = Some(pos);
    }
    let mut v: Vec<Suspect> = by.into_values().collect();
    v.sort_by_key(|s| (s.in_failure.is_none(), s.in_failure.unwrap_or(0), std::cmp::Reverse(s.overlap), s.depth, g.syms[s.symbol].file.clone(), g.syms[s.symbol].sym.start_line));
    w.suspects = v;
    w
}

/// The source line at a place the failure names, when the file is in the repo.
fn source_line(g: &Graph, root: &Path, file: &str, line: u32) -> Option<(String, String)> {
    let f = locate::match_file(g, root, file)?;
    let text = std::fs::read_to_string(root.join(&f)).ok()?;
    let l = text.lines().nth(line.checked_sub(1)? as usize)?.trim().to_string();
    (!l.is_empty()).then_some((f, l))
}

pub fn render(g: &Graph, root: &Path, w: &Why) -> String {
    let names: Vec<&str> = w.tests.iter().map(|&t| g.syms[t].sym.name.as_str()).collect();
    let mut s = String::new();
    if w.steps.is_empty() {
        let _ = writeln!(s, "why: no command known to run {} (see `gitnext tests-for`)", names.join(", "));
        return s;
    }
    let secs: f32 = w.steps.iter().map(|x| x.seconds).sum();
    if !w.failed() {
        let _ = writeln!(s, "why: {} — passes ({secs:.1}s), nothing to explain", names.join(", "));
        for st in &w.steps {
            let _ = writeln!(s, "  {}", st.command);
        }
        return s;
    }
    let _ = writeln!(s, "why: {} — FAILS ({secs:.1}s)", names.join(", "));
    for st in w.steps.iter().filter(|x| !x.ok) {
        let _ = writeln!(s, "  {}", st.command);
        if st.timed_out {
            let _ = writeln!(s, "  timed out");
        }
        if st.digest.failures.is_empty() {
            for l in st.output.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
                let _ = writeln!(s, "  {l}");
            }
            continue;
        }
        // one failing test: its totals line says nothing the header did not
        let text = crate::digest::render(&st.digest, 3);
        let one = st.digest.kind == "tests" && st.digest.failures.len() == 1;
        s.extend(text.lines().skip(usize::from(one)).map(|l| format!("{l}\n")));
        // the line of the first failure, when the tool did not print it itself
        if let Some(f) = st.digest.failures.first() {
            if let (Some(p), Some(n)) = (&f.file, f.line) {
                if !f.message.iter().any(|m| m.starts_with("> ")) {
                    if let Some((file, text)) = source_line(g, root, p, n) {
                        let _ = writeln!(s, "    {file}:{n}: {text}");
                    }
                }
            }
        }
    }
    if w.suspects.is_empty() {
        let _ = writeln!(s, "no code of the project is reached from the test through the call graph: read the test (`gitnext look {}`)", names.first().copied().unwrap_or(""));
        return s;
    }
    let total = w.suspects.len();
    let _ = writeln!(s, "\ncode the test reaches, most likely first ({} of {total}):", total.min(LISTED));
    for (k, x) in w.suspects.iter().take(LISTED).enumerate() {
        let r = &g.syms[x.symbol];
        let how = match x.in_failure {
            Some(_) => "named in the failure".to_string(),
            None => format!("{} from the test", crate::changes::plural(x.depth, "call")),
        };
        let _ = writeln!(s, "{:>2}. {}  {}:{}  ({how})", k + 1, g.short_name(x.symbol), r.file, r.sym.start_line);
    }
    let _ = writeln!(s);
    // the code of the first; of the second only when something points at it too
    for (k, x) in w.suspects.iter().take(SHOWN).enumerate() {
        if k > 0 && x.in_failure.is_none() && x.overlap == 0 {
            break;
        }
        let r = &g.syms[x.symbol];
        let Ok(text) = std::fs::read_to_string(root.join(&r.file)) else { continue };
        let (a, b) = (r.sym.start_line as usize, r.sym.end_line as usize);
        let body: Vec<&str> = text.lines().skip(a.saturating_sub(1)).take(b + 1 - a).collect();
        let fence = gitnext_parse::lang_for(Path::new(&r.file)).map(|l| l.fence()).unwrap_or("");
        let _ = writeln!(s, "{}:{a}-{b} {}\n```{fence}", r.file, g.short_name(x.symbol));
        for l in body.iter().take(CODE_LINES) {
            let _ = writeln!(s, "{l}");
        }
        if body.len() > CODE_LINES {
            let _ = writeln!(s, "… {} more lines: `gitnext look {}`", body.len() - CODE_LINES, g.short_name(x.symbol));
        }
        let _ = writeln!(s, "```");
    }
    let _ = writeln!(s, "next: fix the code, not the test (`gitnext edit replace <function>`), then `gitnext check`");
    s
}
