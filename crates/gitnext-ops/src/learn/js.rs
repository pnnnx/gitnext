//! JavaScript / TypeScript: every test file of vitest run alone with v8 coverage
//! (`vitest run <file> --coverage.enabled --coverage.provider=v8 --coverage.reporter=json`).
//! A test is the file: `it('…', () => …)` is not a named function, and the index knows a test
//! file by its `module` symbol, so the pairs name that symbol. Measured on yaml and zustand: the
//! map finds every test file that fails when a function breaks (the call graph: 16% and 68%), for
//! 2.6–6.8× a plain test run.
//!
//! A statement that ran belongs to the innermost function whose body holds its line, from the
//! line after the declaration: the declaration runs when the module is imported, and counting it
//! would book every function of every imported module to the test. jest is not learned.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use gitnext_index::Graph;
use gitnext_parse::Kind;

use super::{owners, Run};

/// One test file with coverage that takes longer than this is given up.
const FILE_LIMIT: Duration = Duration::from_secs(600);
/// How many file names a note lists before it says "and N more".
const NAMES: usize = 5;

pub(super) fn run(root: &Path, g: &Graph) -> Result<Run> {
    run_with("npx", root, g)
}

/// The temporary reports; removed however `run` ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn last_line(text: &str) -> &str {
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

/// The test runner `package.json` names: a dependency first (`vitest` wins over `jest`), then the
/// `test` script. `@testing-library/jest-dom` and the like are not jest.
fn runner(package_json: &str) -> Option<&'static str> {
    let v: serde_json::Value = serde_json::from_str(package_json).ok()?;
    let has = |name: &str| ["devDependencies", "dependencies"].iter().any(|k| v.get(k).and_then(|d| d.get(name)).is_some());
    if has("vitest") {
        return Some("vitest");
    }
    if has("jest") {
        return Some("jest");
    }
    let script = v.get("scripts").and_then(|s| s.get("test")).and_then(|s| s.as_str()).unwrap_or("");
    ["vitest", "jest"].into_iter().find(|r| script.split(|c: char| !c.is_alphanumeric()).any(|w| w == *r))
}

/// Learning can start: vitest in `package.json` and installed with its v8 coverage, and an `npx`
/// that runs. Nothing is installed here; the message says what to run. Gives vitest's version.
fn ready(npx: &str, root: &Path) -> Result<String> {
    let Ok(package) = std::fs::read_to_string(root.join("package.json")) else {
        bail!("no package.json at the root of the repository (packages in subdirectories are not learned yet)");
    };
    match runner(&package) {
        Some("vitest") => {}
        Some(other) => bail!("the tests run with {other}; learn-tests knows vitest only"),
        None => bail!("package.json names no test runner learn-tests knows (it knows vitest)"),
    }
    let installed = std::fs::read_to_string(root.join("node_modules/vitest/package.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let Some(version) = installed.and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from)) else {
        bail!("vitest is not installed in node_modules: run `npm install` first");
    };
    if !root.join("node_modules/@vitest/coverage-v8").is_dir() {
        bail!("vitest's v8 coverage is not installed: npm i -D @vitest/coverage-v8@{version}");
    }
    match Command::new(npx).arg("--version").stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(version),
        Ok(o) => bail!("`{npx} --version` failed: {}", last_line(&String::from_utf8_lossy(&o.stderr))),
        Err(_) => bail!("`{npx}` is not in PATH: install Node.js (https://nodejs.org) to learn which test runs which function"),
    }
}

/// `path` relative to the repository, when it is inside it.
fn relative(path: &str, roots: &[String]) -> Option<String> {
    roots.iter().find_map(|r| path.strip_prefix(&format!("{r}/"))).map(String::from)
}

/// `vitest list --filesOnly --json` → the test files, relative to the repository.
fn listed_files(json: &str, roots: &[String]) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap_or_default();
    let mut out: Vec<String> = v.as_array().into_iter().flatten().filter_map(|e| e.get("file")?.as_str()).filter_map(|f| relative(f, roots)).collect();
    out.sort();
    out.dedup();
    out
}

/// `coverage-final.json` (istanbul format) → file relative to the repository → the first lines
/// of the statements that ran.
fn covered_lines(json: &str, roots: &[String]) -> BTreeMap<String, BTreeSet<u32>> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut out = BTreeMap::new();
    for (path, file) in v.as_object().into_iter().flatten() {
        let Some(rel) = relative(path, roots) else { continue };
        let (Some(counts), Some(map)) = (file.get("s").and_then(|s| s.as_object()), file.get("statementMap")) else { continue };
        let lines: BTreeSet<u32> = counts
            .iter()
            .filter(|(_, n)| n.as_u64().is_some_and(|n| n > 0))
            .filter_map(|(k, _)| map.get(k)?.get("start")?.get("line")?.as_u64())
            .map(|l| l as u32)
            .collect();
        if !lines.is_empty() {
            out.insert(rel, lines);
        }
    }
    out
}

/// vitest's `--reporter=json` result: did every test of the file pass. The counters are read from
/// the text: the result quotes the tests' strings, and a lone surrogate (`"\\ud800"`, yaml's tests
/// have them) is not JSON serde_json accepts. They stand at the top, before `testResults`.
fn passed(json: &str) -> bool {
    let count = |key: &str| {
        let at = json.find(&format!("\"{key}\":"))? + key.len() + 3;
        json[at..].trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse::<u64>().ok()
    };
    count("numTotalTests").is_some() && count("numFailedTests") == Some(0) && count("numFailedTestSuites") == Some(0)
}

/// Why a test file of vitest's `--reporter=json` result failed as a whole: the first line of its message.
fn load_error(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let m = v.get("testResults")?.as_array()?.iter().find_map(|r| r.get("message")?.as_str().filter(|m| !m.trim().is_empty()))?;
    Some(m.lines().next().unwrap_or("").trim().chars().take(160).collect())
}

fn some(names: &[String]) -> String {
    let more = names.len().saturating_sub(NAMES);
    let head = names.iter().take(NAMES).cloned().collect::<Vec<_>>().join(", ");
    if more > 0 {
        format!("{head} and {more} more")
    } else {
        head
    }
}

/// What coverage counts: every top directory with code of the index (`src/**/*.{ts,…}`) and the
/// files with code at the root. The project's own `coverage.include` is overridden: it may name
/// nothing (zustand's `src/**/` matches directories only) and is meant for its report, not for this.
fn include_globs<'a>(files: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = files.map(|f| f.split_once('/').map_or(f.to_string(), |(top, _)| format!("{top}/**/*.{{ts,tsx,mts,cts,js,jsx,mjs,cjs}}"))).collect();
    out.sort();
    out.dedup();
    out
}

/// One test file with coverage; `false` when it did not finish in time. The report goes to
/// `<dir>/coverage` (vitest empties that directory first), the log and the result next to it.
fn run_file(npx: &str, root: &Path, file: &str, include: &[String], dir: &Path) -> Result<bool> {
    let log = std::fs::File::create(dir.join("out.log"))?;
    let mut child = Command::new(npx)
        .args(["vitest", "run", file, "--coverage.enabled", "--coverage.provider=v8", "--coverage.reporter=json", "--coverage.clean=true", "--reporter=json"])
        .args(include.iter().map(|g| format!("--coverage.include={g}")))
        .arg(format!("--coverage.reportsDirectory={}", dir.join("coverage").display()))
        .arg(format!("--outputFile={}", dir.join("result.json").display()))
        .env("FORCE_COLOR", "0")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .with_context(|| format!("running `{npx} vitest run {file}`"))?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if started.elapsed() > FILE_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_with(npx: &str, root: &Path, g: &Graph) -> Result<Run> {
    let version = ready(npx, root)?;
    let started = Instant::now();
    let mut roots = vec![root.display().to_string()];
    if let Ok(c) = root.canonicalize() {
        roots.push(c.display().to_string());
    }
    // the test's symbol in the graph: a test file is its `module`
    let module_of: HashMap<&str, &str> = g
        .syms
        .iter()
        .filter(|s| s.sym.test && s.sym.kind == Kind::Module && gitnext_parse::family_of(&s.file) == Some("js"))
        .map(|s| (s.file.as_str(), s.sym.name.as_str()))
        .collect();
    let mut notes = Vec::new();
    let list = Command::new(npx).args(["vitest", "list", "--filesOnly", "--json"]).env("FORCE_COLOR", "0").current_dir(root).stdin(Stdio::null()).output().context("running `npx vitest list`")?;
    let mut files = listed_files(&String::from_utf8_lossy(&list.stdout), &roots);
    if files.is_empty() {
        // an older vitest without `list --filesOnly`: the test files the index knows
        notes.push(format!("JS/TS: `vitest list --filesOnly --json` gave no test file ({}); ran the test files of the index", last_line(&String::from_utf8_lossy(&list.stderr))));
        files = module_of.keys().map(|f| f.to_string()).collect();
        files.sort();
    }
    if files.is_empty() {
        bail!("vitest found no test file");
    }
    let owner_of = owners(g);
    let include = include_globs(owner_of.keys().copied().filter(|f| gitnext_parse::family_of(f) == Some("js")));
    if include.is_empty() {
        bail!("the index has no JavaScript / TypeScript function outside the tests");
    }
    let scratch = Scratch(root.join(".gitnext").join(format!("learn-js-{}", std::process::id())));
    std::fs::create_dir_all(&scratch.0)?;

    let mut pairs = BTreeSet::new();
    let (mut failing, mut timed_out, mut unknown, mut no_report) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (k, file) in files.iter().enumerate() {
        let Some(&name) = module_of.get(file.as_str()) else {
            unknown.push(file.clone());
            continue;
        };
        let dir = scratch.0.join(k.to_string());
        std::fs::create_dir_all(&dir)?;
        if !run_file(npx, root, file, &include, &dir)? {
            timed_out.push(file.clone());
            continue;
        }
        let result = std::fs::read_to_string(dir.join("result.json")).unwrap_or_default();
        let Ok(report) = std::fs::read_to_string(dir.join("coverage").join("coverage-final.json")) else {
            // the file did not load (a missing fixture, a config error) or vitest itself stopped
            let log = std::fs::read_to_string(dir.join("out.log")).unwrap_or_default();
            no_report.push(format!("{file} ({})", load_error(&result).unwrap_or_else(|| last_line(&log).to_string())));
            continue;
        };
        if !passed(&result) {
            failing.push(file.clone());
        }
        for (f, lines) in covered_lines(&report, &roots) {
            let Some(owners) = owner_of.get(f.as_str()) else { continue }; // not code of the index (built output, a test file)
            for l in lines {
                if let Some(Some(i)) = owners.get(l as usize) {
                    pairs.insert((*i, file.clone(), name.to_string()));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    if !failing.is_empty() {
        notes.push(format!("JS/TS: {} test files fail (what they ran is kept): {}", failing.len(), some(&failing)));
    }
    if !timed_out.is_empty() {
        notes.push(format!("JS/TS: {} test files took longer than 10 minutes and are not in the map: {}", timed_out.len(), some(&timed_out)));
    }
    if !no_report.is_empty() {
        notes.push(format!("JS/TS: {} test files wrote no coverage report and are not in the map: {}", no_report.len(), some(&no_report)));
    }
    if !unknown.is_empty() {
        notes.push(format!("JS/TS: {} test files have no symbol in the index and were not run: {}", unknown.len(), some(&unknown)));
    }
    let ran = files.len() - unknown.len();
    Ok(Run {
        source: "vitest-v8",
        command: format!("npx vitest run <file> --coverage.enabled --coverage.provider=v8 --coverage.reporter=json --coverage.include={}, each test file alone ({ran} files, vitest {version})", include.join(",")),
        seconds: started.elapsed().as_secs_f32(),
        tests_failed: !failing.is_empty() || !timed_out.is_empty() || !no_report.is_empty(),
        pairs,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report vitest 4 wrote for `tests/calc.test.ts` of the project in `tests/learn_js.rs`,
    /// the repository's path replaced by `/r/demo`.
    const REPORT: &str = include_str!("../../tests/data/learn_js/coverage-final.json");

    #[test]
    fn a_report_gives_the_lines_that_ran_by_file_of_the_repository() {
        let roots = vec!["/r/demo".to_string()];
        let got = covered_lines(REPORT, &roots);
        assert_eq!(got.keys().collect::<Vec<_>>(), ["src/calc.ts"]);
        let lines = &got["src/calc.ts"];
        // `add` and `double` ran (their bodies), `unused` did not
        assert!(lines.contains(&2) && lines.contains(&6), "{lines:?}");
        assert!(!lines.contains(&10), "{lines:?}");
        // a file outside the repository is not counted
        assert!(covered_lines(REPORT, &["/elsewhere".to_string()]).is_empty());
    }

    #[test]
    fn the_test_files_come_from_vitest_list() {
        let roots = vec!["/r/demo".to_string(), "/private/r/demo".to_string()];
        let json = r#"[{"file":"/r/demo/tests/b.test.ts"},{"file":"/private/r/demo/tests/a.ts","projectName":"x"},{"file":"/other/c.ts"}]"#;
        assert_eq!(listed_files(json, &roots), ["tests/a.ts", "tests/b.test.ts"]);
        assert!(listed_files("not json", &roots).is_empty());
    }

    #[test]
    fn the_runner_is_read_from_package_json() {
        assert_eq!(runner(r#"{"devDependencies":{"@testing-library/jest-dom":"1","vitest":"4"}}"#), Some("vitest"));
        assert_eq!(runner(r#"{"devDependencies":{"jest":"29"}}"#), Some("jest"));
        assert_eq!(runner(r#"{"scripts":{"test":"vitest run"}}"#), Some("vitest"));
        assert_eq!(runner(r#"{"devDependencies":{"@testing-library/jest-dom":"1"},"scripts":{"test":"mocha"}}"#), None);
    }

    #[test]
    fn a_failed_test_run_is_seen_in_the_result() {
        assert!(passed(r#"{"numFailedTestSuites":0,"numTotalTests":2,"numFailedTests":0,"success":true}"#));
        assert!(!passed(r#"{"numFailedTestSuites":0,"numTotalTests":2,"numFailedTests":1,"success":false}"#));
        // a file that did not load fails as a suite with no test
        assert!(!passed(r#"{"numFailedTestSuites":1,"numTotalTests":0,"numFailedTests":0,"success":false}"#));
        // not JSON for serde_json: a lone surrogate quoted from a test
        let quoted = r#"{"numFailedTestSuites":0,"numTotalTests":1,"numFailedTests":0,"testResults":[{"assertionResults":[{"title":"\ud800"}]}]}"#;
        assert!(serde_json::from_str::<serde_json::Value>(quoted).is_err() && passed(quoted));
        assert!(!passed(""));
        let broken = r#"{"success":false,"testResults":[{"status":"failed","message":"ENOENT: no such file or directory, scandir 'tests/suite/src'\n    at x"}]}"#;
        assert_eq!(load_error(broken).as_deref(), Some("ENOENT: no such file or directory, scandir 'tests/suite/src'"));
        assert_eq!(load_error(r#"{"testResults":[{"message":""}]}"#), None);
    }

    #[test]
    fn without_npx_the_message_says_what_to_install() {
        let dir = std::env::temp_dir().join(format!("gitnext-learn-js-ready-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("node_modules/vitest")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/@vitest/coverage-v8")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"devDependencies":{"vitest":"4"}}"#).unwrap();
        std::fs::write(dir.join("node_modules/vitest/package.json"), r#"{"version":"4.1.11"}"#).unwrap();
        let e = ready("/nonexistent/bin/npx", &dir).unwrap_err().to_string();
        assert!(e.contains("is not in PATH") && e.contains("nodejs.org"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_counts_the_top_directories_with_code_and_the_files_at_the_root() {
        let got = include_globs(["src/a.ts", "src/x/b.tsx", "examples/demo/c.js", "rollup.config.mjs"].into_iter());
        assert_eq!(got, ["examples/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}", "rollup.config.mjs", "src/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}"]);
    }

    #[test]
    fn a_long_list_of_names_is_cut() {
        let names: Vec<String> = (1..=7).map(|i| format!("t{i}.ts")).collect();
        assert_eq!(some(&names), "t1.ts, t2.ts, t3.ts, t4.ts, t5.ts and 2 more");
    }
}
