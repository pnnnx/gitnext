//! `gitnext learn-tests`: run the project's tests once with coverage and remember which
//! test runs which function. `tests-for`, `look` and `check` then name the tests that
//! really execute a function instead of the ones the call graph can reach.
//!
//! Each language has its own way to get "this test ran these lines" (`learn/<language>.rs`);
//! what they return is the same — pairs "function, test" — and the rest is shared.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{bail, Result};
use gitnext_index::{Graph, TestMap};

mod go;
mod js;
mod python;
mod rust;

/// What the test run of one language gave.
pub(crate) struct Run {
    /// what measured it (`pytest-cov`), kept in the map
    pub source: &'static str,
    pub command: String,
    pub seconds: f32,
    /// the test run itself failed (the pairs are still what ran)
    pub tests_failed: bool,
    /// (function: its index in the graph, the test's file relative to the repository, the test's
    /// name: `test_b` or `TestA::test_b`, parameters and subtests dropped). The file and the last
    /// part of the name must be those of the test's symbol in the graph: that is how `tests-for`
    /// and `why` find it again.
    pub pairs: BTreeSet<(usize, String, String)>,
    /// what the user should know about this run, one line each: where an instrumented build was
    /// left and how large it is, how many tests could not be run alone or were not found in the graph
    pub notes: Vec<String>,
}

/// The language is in the repository when a file ends with one of its suffixes; its tests are
/// run once by the function.
type Learner = fn(&Path, &Graph) -> Result<Run>;
const LANGUAGES: &[(&str, &[&str], Learner)] = &[
    ("Python", &[".py"], python::run), ("Go", &[".go"], go::run), ("Rust", &[".rs"], rust::run),
    ("JavaScript/TypeScript", &[".ts", ".tsx", ".mts", ".js", ".jsx", ".mjs", ".cjs"], js::run),
];

pub struct Learned {
    pub command: String,
    pub seconds: f32,
    /// the test run itself failed (the map is still built from what ran)
    pub tests_failed: bool,
    pub functions: usize,
    pub tests: usize,
    pub pairs: usize,
    /// functions with a body that no test ran
    pub never_run: usize,
    /// from the languages' runs, and why a language of the repository gave no map
    pub notes: Vec<String>,
}

/// file → for each line, the innermost function with a body around it (tests are not counted).
/// In Python the first line (`def …:`) runs when the module is loaded, not when the function is
/// called, so a body starts on the line after it.
pub(crate) fn owners(g: &Graph) -> HashMap<&str, Vec<Option<usize>>> {
    owners_at(g, false)
}

/// The same with the first line counted when `header`: in a compiled language (Go, Rust) coverage
/// books the first block of a function to the line of its signature, and that line runs only when
/// the function is called. One-line functions are then counted too.
pub(crate) fn owners_at(g: &Graph, header: bool) -> HashMap<&str, Vec<Option<usize>>> {
    let mut by_file: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in g.syms.iter().enumerate() {
        if s.sym.kind.is_callable() && !s.sym.test && (header || s.sym.end_line > s.sym.start_line) {
            by_file.entry(s.file.as_str()).or_default().push(i);
        }
    }
    let mut owner_of: HashMap<&str, Vec<Option<usize>>> = HashMap::new();
    for (file, syms) in &mut by_file {
        syms.sort_by_key(|&i| std::cmp::Reverse(g.syms[i].sym.end_line - g.syms[i].sym.start_line)); // outer first, inner overwrite
        let last = syms.iter().map(|&i| g.syms[i].sym.end_line).max().unwrap_or(0) as usize;
        let mut lines = vec![None; last + 1];
        for &i in syms.iter() {
            for l in g.syms[i].sym.start_line + u32::from(!header)..=g.syms[i].sym.end_line {
                lines[l as usize] = Some(i);
            }
        }
        owner_of.insert(file, lines);
    }
    owner_of
}

pub fn learn(root: &Path, g: &Graph) -> Result<Learned> {
    let here: Vec<&(&str, &[&str], Learner)> = LANGUAGES.iter().filter(|(_, exts, _)| g.files.iter().any(|f| exts.iter().any(|e| f.path.ends_with(e)))).collect();
    if here.is_empty() {
        bail!("learn-tests has no way to run the tests of this repository with coverage (it knows: {})", LANGUAGES.iter().map(|l| l.0).collect::<Vec<_>>().join(", "));
    }
    let (mut runs, mut errors) = (Vec::new(), Vec::new());
    for (name, _, run) in here {
        match run(root, g) {
            Ok(r) if r.pairs.is_empty() => errors.push(format!("{name}: the coverage run recorded no test that ran a function of this repository (`{}`, {:.0}s)", r.command, r.seconds)),
            Ok(r) => runs.push(r),
            Err(e) => errors.push(format!("{name}: {e:#}")),
        }
    }
    if runs.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    let rows: Vec<(String, String, String, String)> =
        runs.iter().flat_map(|r| r.pairs.iter()).map(|(i, f, n)| (g.unique_name(*i), g.syms[*i].sym.body_hash.clone(), f.clone(), n.clone())).collect();
    let map = TestMap::write(root, &runs.iter().map(|r| r.source).collect::<Vec<_>>().join("+"), &rows)?;
    // the commit the tests ran on: `why` shows what changed in a function since then
    if let Ok(out) = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output() {
        let commit = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !commit.is_empty() {
            map.set_meta("commit", &commit)?;
        }
    }
    let (functions, tests, n) = map.size();
    // Python counts a body from the line after `def`; the compiled languages from the signature
    let header = runs.iter().all(|r| r.source != "pytest-cov");
    let with_body = owners_at(g, header).values().flat_map(|lines| lines.iter().flatten()).collect::<std::collections::HashSet<_>>().len();
    Ok(Learned {
        command: runs.iter().map(|r| r.command.as_str()).collect::<Vec<_>>().join("; "),
        seconds: runs.iter().map(|r| r.seconds).sum(),
        tests_failed: runs.iter().any(|r| r.tests_failed),
        functions,
        tests,
        pairs: n,
        never_run: with_body.saturating_sub(functions),
        notes: runs.iter().flat_map(|r| r.notes.iter().cloned()).chain(errors.into_iter().filter(|e| !e.ends_with(": not supported yet"))).collect(),
    })
}

pub fn render(l: &Learned) -> String {
    let mut s = format!(
        "learned which tests run which functions: {} functions, {} tests, {} pairs ({:.0}s: {})\n",
        l.functions, l.tests, l.pairs, l.seconds, l.command
    );
    if l.never_run > 0 {
        s.push_str(&format!("{} functions were run by no test — for those `tests-for` still uses the call graph\n", l.never_run));
    }
    for n in &l.notes {
        s.push_str(&format!("note: {n}\n"));
    }
    if l.tests_failed {
        s.push_str("note: the test run itself reported failures; the map holds what ran\n");
    }
    s.push_str("`tests-for`, `look` and `check` now use the map; run `gitnext learn-tests` again after larger changes\n");
    s
}
