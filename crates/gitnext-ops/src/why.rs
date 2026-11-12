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
    /// how many tests ran it in the learned coverage run (0 without a map): code that few tests
    /// run and this failing one does is likelier at fault than what every test runs
    pub tests: usize,
    /// the learned map (`gitnext learn-tests`) says the test really ran it
    pub ran: bool,
    /// its code is not what it was when the map was learned
    pub changed: bool,
}

pub struct Why {
    pub tests: Vec<usize>,
    pub steps: Vec<Step>,
    pub suspects: Vec<Suspect>,
    /// the suspects are what the test ran in the learned coverage run, not what the call graph reaches
    pub learned: bool,
    /// the commit the map was learned on, when it is known
    pub since: Option<String>,
}

impl Why {
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|s| !s.ok)
    }
}

/// Tests run at once when a name fits several (`test_basic` of five files): the ones that fail are kept.
const SAME_NAME: usize = 6;

/// Runnable tests by name. `greater_than` finds `test_greater_than` / `TestGreaterThan` when no
/// test has exactly that name (at most three, the shortest names first). The id a test runner
/// prints is taken as it is: `tests/test_x.py::TestA::test_b[case]`, `TestRun/subtest`.
pub fn find_tests(g: &Graph, name: &str) -> Vec<usize> {
    let runnable = |i: &usize| g.syms[*i].sym.test && locate::is_runnable(g, *i);
    let name = name.trim().split('[').next().unwrap_or(name);
    // `path/to/test_x.py::rest`: the file narrows the search
    let (file, name) = match name.split_once("::") {
        Some((f, rest)) if f.contains('.') && g.files.iter().any(|x| x.path == f || x.path.ends_with(&format!("/{f}"))) => (Some(f), rest),
        _ => (None, name),
    };
    let in_file = |i: &usize| file.is_none_or(|f| g.syms[*i].file == f || g.syms[*i].file.ends_with(&format!("/{f}")));
    let mut exact: Vec<usize> = g.find(name).into_iter().filter(runnable).filter(in_file).collect();
    if exact.is_empty() && name.contains('/') && !name.contains("::") {
        // Go's `TestRun/subtest`
        exact = g.find(name.split('/').next().unwrap_or(name)).into_iter().filter(runnable).collect();
    }
    if !exact.is_empty() {
        return exact.into_iter().take(SAME_NAME).collect();
    }
    let bare = name.rsplit("::").next().unwrap_or(name).rsplit('.').next().unwrap_or(name);
    let want: Vec<String> = locate::name_words(bare).into_iter().filter(|w| w != "test" && w != "tests").collect();
    if want.is_empty() {
        return vec![];
    }
    let mut hits: Vec<usize> = (0..g.syms.len())
        .filter(runnable)
        .filter(in_file)
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

/// symbol → what it calls (resolved through the call graph).
fn calls(g: &Graph) -> HashMap<usize, Vec<usize>> {
    let mut forward: HashMap<usize, Vec<usize>> = HashMap::new();
    for t in 0..g.syms.len() {
        for &(src, kind, _) in g.referrers(t) {
            if kind == EdgeKind::Calls {
                forward.entry(src).or_default().push(t);
            }
        }
    }
    forward
}

/// Non-test functions reachable from the tests through calls, with the shortest distance.
fn reached(g: &Graph, forward: &HashMap<usize, Vec<usize>>, tests: &[usize]) -> Vec<(usize, usize)> {
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
    let mut w = Why { tests: tests.to_vec(), steps, suspects: Vec::new(), learned: false, since: None };
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
    // several tests were run (one name, several files): the code to look at is what the failing ones use
    let whats: Vec<(&str, Option<&str>)> =
        w.steps.iter().filter(|s| !s.ok).flat_map(|s| s.digest.failures.iter()).map(|f| (f.what.split('[').next().unwrap_or(&f.what), f.file.as_deref())).collect();
    let failing = |strict: bool| -> Vec<usize> {
        let hit = |t: &usize| {
            let r = &g.syms[*t];
            whats.iter().any(|(x, file)| {
                let name = x.rsplit("::").next().unwrap_or(x).rsplit('.').next().unwrap_or(x);
                name == r.sym.name && (!strict || x.contains(r.file.as_str()) || file.is_some_and(|f| f == r.file || f.ends_with(&format!("/{}", r.file))))
            })
        };
        tests.iter().copied().filter(hit).collect()
    };
    let narrowed = [failing(true), failing(false)].into_iter().find(|v| !v.is_empty()).unwrap_or_else(|| tests.to_vec());
    let tests = narrowed.as_slice();
    w.tests = narrowed.clone();
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
    let rank = std::env::var("GITNEXT_WHY_RANK").unwrap_or_default();
    let mut by: HashMap<usize, Suspect> = HashMap::new();
    // what the tests really ran, when a coverage run was learned and knows them: a call through a
    // CLI runner, a decorator or a trait is in it, and code on a branch the test never takes is not
    if rank != "static" {
        if let Some(map) = gitnext_index::TestMap::open_existing(root) {
            w.since = map.meta("commit");
            let by_qual: HashMap<&str, usize> =
                g.syms.iter().enumerate().filter(|(_, r)| !r.sym.test && r.sym.kind.is_callable() && !r.sym.decl).map(|(i, r)| (r.sym.qualified.as_str(), i)).collect();
            for &t in tests {
                let r = &g.syms[t];
                let class = r.sym.parent.as_deref().map(|p| p.rsplit("::").next().unwrap_or(p));
                let mut ran = class.map(|c| map.ran_by(&r.file, &format!("{c}::{}", r.sym.name))).unwrap_or_default();
                if ran.is_empty() {
                    ran = map.ran_by(&r.file, &r.sym.name);
                }
                for (q, hash, n) in ran {
                    let Some(&i) = by_qual.get(q.as_str()) else { continue }; // renamed or removed since
                    let changed = g.syms[i].sym.body_hash != hash;
                    by.entry(i).or_insert(Suspect { symbol: i, depth: 0, overlap: overlap(i), in_failure: None, tests: n as usize, ran: true, changed });
                }
            }
        }
    }
    w.learned = !by.is_empty();
    if w.learned {
        for (pos, &i) in named.iter().enumerate() {
            by.entry(i).or_insert(Suspect { symbol: i, depth: 0, overlap: overlap(i), in_failure: None, tests: 0, ran: false, changed: false }).in_failure = Some(pos);
        }
    } else {
        let forward = calls(g);
        for (i, d) in reached(g, &forward, tests) {
            by.insert(i, Suspect { symbol: i, depth: d, overlap: overlap(i), in_failure: None, tests: 0, ran: false, changed: false });
        }
        for (pos, &i) in named.iter().enumerate() {
            by.entry(i).or_insert(Suspect { symbol: i, depth: DEPTH + 1, overlap: overlap(i), in_failure: None, tests: 0, ran: false, changed: false }).in_failure = Some(pos);
        }
    }
    let mut v: Vec<Suspect> = by.into_values().collect();
    let learned = w.learned;
    v.sort_by_key(|s| {
        let named = (s.in_failure.is_none(), s.in_failure.unwrap_or(0));
        let place = (g.syms[s.symbol].file.clone(), g.syms[s.symbol].sym.start_line);
        let words = usize::MAX - s.overlap;
        if learned {
            // code that changed since the test last passed under coverage, then what few other tests run
            // (20 seeded bugs in click and sqlparse: the culprit is first in 90%; without the "changed"
            // mark first in 20%, among the first 8 in 80%; the call graph alone: 5% and 20%)
            let same = !s.changed || rank == "nochange";
            return (named, usize::from(same), s.tests, words, 0, place);
        }
        // measured on 40 seeded bugs: counting the tests that reach a function through the call
        // graph does not help (almost every test reaches the same code), the distance does a little
        (named, words, s.depth, 0, 0, place)
    });
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

/// Lines shown of what changed in a function since the map was learned.
const CHANGED_LINES: usize = 10;

/// What changed inside lines `a..=b` of `file` since `commit`: the removed and the added lines.
fn changed_since(root: &Path, commit: &str, file: &str, a: u32, b: u32) -> Vec<String> {
    let Ok(out) = std::process::Command::new("git").args(["diff", "-U0", "--no-color", "--no-ext-diff", commit, "--", file]).current_dir(root).output() else { return vec![] };
    if !out.status.success() {
        return vec![];
    }
    hunks_within(&String::from_utf8_lossy(&out.stdout), a, b)
}

/// The `-` / `+` lines of the hunks of a `git diff -U0` that touch lines `a..=b` of the new file.
pub fn hunks_within(diff: &str, a: u32, b: u32) -> Vec<String> {
    let (mut out, mut inside) = (Vec::new(), false);
    for l in diff.lines() {
        if let Some(rest) = l.strip_prefix("@@ ") {
            // `-12,2 +12,3 @@ …`: the new side; a count of 0 is a pure removal at that line
            let new = rest.split(' ').find(|x| x.starts_with('+')).unwrap_or("+0");
            let mut it = new[1..].split(',');
            let start: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let count: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
            inside = start <= b && start + count.max(1) > a;
        } else if inside && (l.starts_with('-') || l.starts_with('+')) && !l.starts_with("---") && !l.starts_with("+++") {
            out.push(format!("{} {}", &l[..1], l[1..].trim()));
        }
    }
    out
}

/// Everything the ranking knows, for scripts (`gitnext why <test> --json`).
pub fn json(g: &Graph, w: &Why) -> serde_json::Value {
    serde_json::json!({
        "tests": w.tests.iter().map(|&t| g.syms[t].sym.qualified.as_str()).collect::<Vec<_>>(),
        "failed": w.failed(),
        "learned": w.learned,
        "failures": w.steps.iter().flat_map(|s| s.digest.failures.iter()).map(|f| serde_json::json!({ "what": f.what, "file": f.file, "line": f.line, "message": f.message, "count": f.count })).collect::<Vec<_>>(),
        "suspects": w.suspects.iter().map(|x| {
            let r = &g.syms[x.symbol];
            serde_json::json!({ "symbol": r.sym.qualified, "file": r.file, "line": r.sym.start_line, "depth": x.depth, "overlap": x.overlap, "tests": x.tests, "named": x.in_failure.is_some(), "ran": x.ran, "changed": x.changed })
        }).collect::<Vec<_>>(),
    })
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
    let what = if w.learned { "code the test ran (learned coverage)" } else { "code the test reaches" };
    let _ = writeln!(s, "\n{what}, most likely first ({} of {total}):", total.min(LISTED));
    for (k, x) in w.suspects.iter().take(LISTED).enumerate() {
        let r = &g.syms[x.symbol];
        let how = match x.in_failure {
            Some(_) => "named in the failure".to_string(),
            None if x.changed => "changed since `learn-tests`".to_string(),
            None if x.ran => format!("run by {}", crate::changes::plural(x.tests, "test")),
            None => format!("{} from the test", crate::changes::plural(x.depth, "call")),
        };
        let _ = writeln!(s, "{:>2}. {}  {}:{}  ({how})", k + 1, g.short_name(x.symbol), r.file, r.sym.start_line);
    }
    // what was changed in the first of them since the tests ran under coverage: usually the bug itself
    if let (Some(commit), Some(x)) = (&w.since, w.suspects.iter().take(SHOWN).find(|x| x.changed)) {
        let r = &g.syms[x.symbol];
        let lines = changed_since(root, commit, &r.file, r.sym.start_line, r.sym.end_line);
        if !lines.is_empty() {
            let _ = writeln!(s, "\nchanged in {} since `learn-tests` (- as it was, + as it is now):", g.short_name(x.symbol));
            for l in lines.iter().take(CHANGED_LINES) {
                let _ = writeln!(s, "  {l}");
            }
            if lines.len() > CHANGED_LINES {
                let _ = writeln!(s, "  … {} more changed lines: `git diff {} -- {}`", lines.len() - CHANGED_LINES, &commit[..commit.len().min(8)], r.file);
            }
        }
    }
    let _ = writeln!(s);
    // the code of the first; of the second only when something points at it too
    for (k, x) in w.suspects.iter().take(SHOWN).enumerate() {
        if k > 0 && x.in_failure.is_none() && x.overlap == 0 && !x.changed {
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
