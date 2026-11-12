//! Python: pytest with pytest-cov, one context per test (`--cov-context=test`). Measured on click:
//! the map finds 100% of the tests that fail when a function breaks (the call graph: 19%), 98% of
//! the tests it names do fail, for one test run that takes about 2.3× longer.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use gitnext_index::Graph;

use super::{owners, Run};

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

pub(super) fn run(root: &Path, g: &Graph) -> Result<Run> {
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

    let prefix = format!("{}/", root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display());
    let owner_of = owners(g);
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
    Ok(Run { source: "pytest-cov", command, seconds, tests_failed: !out.status.success(), pairs, notes: Vec::new() })
}
