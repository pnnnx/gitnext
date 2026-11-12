//! `gitnext learn-tests`: run the project's tests once with coverage and remember which
//! test runs which function. `tests-for`, `look` and `check` then name the tests that
//! really execute a function instead of the ones the call graph can reach.
//!
//! Python (pytest + pytest-cov) for now: measured on click, the map finds 100% of the
//! tests that fail when a function breaks (the call graph: 19%), 98% of the tests it
//! names do fail, for one test run that takes about 2.3× longer.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use gitnext_index::{Graph, TestMap};

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
}

fn python(root: &Path) -> String {
    [".venv/bin/python", "venv/bin/python"].iter().find(|p| root.join(p).exists()).map(|p| p.to_string()).unwrap_or_else(|| "python3".into())
}

/// `tests/test_x.py::TestA::test_b[param]|run` → (`tests/test_x.py`, `TestA::test_b`)
fn test_of(context: &str) -> Option<(String, String)> {
    let id = context.split('|').next()?;
    let (file, rest) = id.split_once("::")?;
    let name = rest.split('[').next()?;
    (!name.is_empty()).then(|| (file.to_string(), name.to_string()))
}

pub fn learn(root: &Path, g: &Graph) -> Result<Learned> {
    if !g.files.iter().any(|f| f.path.ends_with(".py")) {
        bail!("learn-tests reads pytest coverage, and this repository has no Python files (other languages are not supported yet)");
    }
    let py = python(root);
    let has = |module: &str| Command::new(&py).args(["-c", &format!("import {module}")]).current_dir(root).output().map(|o| o.status.success()).unwrap_or(false);
    if !has("pytest") {
        bail!("pytest is not installed for `{py}`: {py} -m pip install pytest pytest-cov");
    }
    if !has("pytest_cov") {
        bail!("pytest-cov is not installed for `{py}`: {py} -m pip install pytest-cov");
    }
    let data = root.join(".gitnext").join("coverage.db");
    std::fs::create_dir_all(root.join(".gitnext"))?;
    for old in ["coverage.db", "coverage.db-journal"] {
        let _ = std::fs::remove_file(root.join(".gitnext").join(old));
    }
    let args = ["-m", "pytest", "-q", "-p", "no:cacheprovider", "--cov=.", "--cov-context=test", "--cov-report="];
    let command = format!("{py} {}", args.join(" "));
    let started = Instant::now();
    let out = Command::new(&py).args(args).env("COVERAGE_FILE", &data).current_dir(root).output().with_context(|| format!("running `{command}`"))?;
    let seconds = started.elapsed().as_secs_f32();
    if !data.exists() {
        let text = String::from_utf8_lossy(&out.stdout);
        let tail: Vec<&str> = text.lines().rev().take(8).collect();
        bail!("`{command}` wrote no coverage data:\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
    }
    let coverage = gitnext_index::read_coverage(&data)?;
    let _ = std::fs::remove_file(&data);

    // line → the innermost function around it, per file; the first line (`def …:`) runs at
    // import, not when the function is called, so a body starts on the line after it
    let prefix = format!("{}/", root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display());
    let mut by_file: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in g.syms.iter().enumerate() {
        if s.sym.kind.is_callable() && !s.sym.test && s.sym.end_line > s.sym.start_line {
            by_file.entry(s.file.as_str()).or_default().push(i);
        }
    }
    let mut owner_of: HashMap<&str, Vec<Option<usize>>> = HashMap::new();
    for (file, syms) in &mut by_file {
        syms.sort_by_key(|&i| std::cmp::Reverse(g.syms[i].sym.end_line - g.syms[i].sym.start_line)); // outer first, inner overwrite
        let last = syms.iter().map(|&i| g.syms[i].sym.end_line).max().unwrap_or(0) as usize;
        let mut lines = vec![None; last + 1];
        for &i in syms.iter() {
            for l in g.syms[i].sym.start_line + 1..=g.syms[i].sym.end_line {
                lines[l as usize] = Some(i);
            }
        }
        owner_of.insert(file, lines);
    }
    let mut pairs: BTreeSet<(usize, String, String)> = BTreeSet::new();
    for (file, context, lines) in &coverage {
        let rel = file.strip_prefix(&prefix).unwrap_or(file);
        let (Some(owners), Some((tfile, tname))) = (owner_of.get(rel), test_of(context)) else { continue };
        for &l in lines {
            if let Some(Some(i)) = owners.get(l as usize) {
                pairs.insert((*i, tfile.clone(), tname.clone()));
            }
        }
    }
    if pairs.is_empty() {
        bail!("the coverage run recorded no test that ran a function of this repository (`{command}`, {seconds:.0}s)");
    }
    let rows: Vec<(String, String, String, String)> = pairs.iter().map(|(i, f, n)| (g.syms[*i].sym.qualified.clone(), g.syms[*i].sym.body_hash.clone(), f.clone(), n.clone())).collect();
    let map = TestMap::write(root, "pytest-cov", &rows)?;
    let (functions, tests, n) = map.size();
    let with_body = by_file.values().map(Vec::len).sum::<usize>();
    Ok(Learned { command, seconds, tests_failed: !out.status.success(), functions, tests, pairs: n, never_run: with_body.saturating_sub(functions) })
}

pub fn render(l: &Learned) -> String {
    let mut s = format!(
        "learned which tests run which functions: {} functions, {} tests, {} pairs ({:.0}s: {})\n",
        l.functions, l.tests, l.pairs, l.seconds, l.command
    );
    if l.never_run > 0 {
        s.push_str(&format!("{} functions were run by no test — for those `tests-for` still uses the call graph\n", l.never_run));
    }
    if l.tests_failed {
        s.push_str("note: the test run itself reported failures; the map holds what ran\n");
    }
    s.push_str("`tests-for`, `look` and `check` now use the map; run `gitnext learn-tests` again after larger changes\n");
    s
}
