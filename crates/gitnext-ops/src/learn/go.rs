//! Go: one test binary per package, built with coverage of the whole module
//! (`go test -c -cover -coverpkg=<module>/...`), then every test run alone with its own profile.
//! Measured on cobra and chi: the map finds 100% of the tests that fail when a function
//! breaks (the call graph: 78%), for 1.2–5× a plain test run.
//!
//! A block of the profile belongs to the innermost function around its first line. In Go the first
//! block of a function starts on the line of its signature (at the `{`), so that line counts.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use gitnext_index::Graph;

use super::{owners_at, Run};

/// A package's test binary that takes longer than this to build is given up.
const BUILD_LIMIT: Duration = Duration::from_secs(600);
/// One test: the binary stops itself at `-test.timeout`, this is the last resort.
const TEST_TIMEOUT: &str = "5m";
const TEST_LIMIT: Duration = Duration::from_secs(360);
/// How many test names a note lists before it says "and N more".
const NAMES: usize = 5;

pub(super) fn run(root: &Path, g: &Graph) -> Result<Run> {
    run_with("go", root, g)
}

/// The temporary binaries and profiles; removed however `run` ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a command printed (stdout and stderr together) and how it ended.
struct Ran {
    ok: bool,
    timed_out: bool,
    text: String,
}

fn timed(cmd: &mut Command, log: &Path, limit: Duration) -> Result<Ran> {
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    let mut child = cmd.stdin(Stdio::null()).stdout(out).stderr(err).spawn()?;
    let started = Instant::now();
    let (ok, timed_out) = loop {
        match child.try_wait()? {
            Some(status) => break (status.success(), false),
            None if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                break (false, true);
            }
        }
    };
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    Ok(Ran { ok, timed_out, text })
}

fn last_line(text: &str) -> &str {
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

/// Learning can start: a module at the root of the repository and a `go` that runs.
fn ready(go: &str, root: &Path) -> Result<()> {
    if !root.join("go.mod").exists() {
        bail!("no go.mod at the root of the repository (modules in subdirectories are not learned yet)");
    }
    match Command::new(go).arg("version").stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => bail!("`{go} version` failed: {}", last_line(&String::from_utf8_lossy(&o.stderr))),
        Err(_) => bail!("`{go}` is not in PATH: install Go (https://go.dev/dl/) to learn which test runs which function"),
    }
}

/// One package with tests: its import path and directory relative to the repository (`""` at the root).
struct Package {
    import: String,
    dir: String,
    tests: bool,
}

/// `go list -f '{{.ImportPath}}\t{{.Dir}}\t{{len .TestGoFiles}}\t{{len .XTestGoFiles}}' ./...`
fn packages(listing: &str, roots: &[String]) -> Vec<Package> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [import, dir, t, x] = f[..] else { continue };
        let Some(rel) = roots.iter().find_map(|r| if dir == r { Some("") } else { dir.strip_prefix(&format!("{r}/")) }) else { continue };
        out.push(Package { import: import.into(), dir: rel.into(), tests: t != "0" || x != "0" });
    }
    out
}

/// A profile → the first line of every block that ran, by file relative to the repository.
/// `example.com/mod/pkg/file.go:12.30,14.2 1 3`; the file is named by its package's import path.
fn profile_lines(text: &str, dir_of: &HashMap<&str, &str>) -> BTreeSet<(String, u32)> {
    let mut out = BTreeSet::new();
    for line in text.lines().skip_while(|l| l.starts_with("mode:")) {
        let mut parts = line.rsplitn(3, ' ');
        let (Some(count), Some(_statements), Some(place)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if count == "0" {
            continue;
        }
        let Some((file, range)) = place.rsplit_once(':') else { continue };
        let Some(Ok(start)) = range.split('.').next().map(str::parse::<u32>) else { continue };
        let Some((import, name)) = file.rsplit_once('/') else { continue };
        let Some(dir) = dir_of.get(import) else { continue }; // a file outside the module
        out.insert((if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }, start));
    }
    out
}

/// Top-level tests that failed in a `-test.v` run (`--- FAIL: TestX (0.00s)`; subtests are indented).
fn failed_in(verbose: &str) -> BTreeSet<String> {
    verbose.lines().filter_map(|l| l.strip_prefix("--- FAIL: ")).filter_map(|r| r.split_whitespace().next()).map(String::from).collect()
}

fn is_test_name(n: &str) -> bool {
    ["Test", "Example", "Fuzz"].iter().any(|p| n.starts_with(p)) && n != "TestMain" && n.chars().all(|c| c.is_alphanumeric() || c == '_')
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

fn run_with(go: &str, root: &Path, g: &Graph) -> Result<Run> {
    ready(go, root)?;
    let started = Instant::now();
    let module_out = Command::new(go).args(["list", "-m"]).current_dir(root).stdin(Stdio::null()).output().context("running `go list -m`")?;
    let module = String::from_utf8_lossy(&module_out.stdout).lines().next().unwrap_or("").trim().to_string();
    if !module_out.status.success() || module.is_empty() {
        bail!("`go list -m` failed: {}", last_line(&String::from_utf8_lossy(&module_out.stderr)));
    }
    let listing = Command::new(go)
        .args(["list", "-e", "-f", "{{.ImportPath}}\t{{.Dir}}\t{{len .TestGoFiles}}\t{{len .XTestGoFiles}}", "./..."])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .context("running `go list ./...`")?;
    let mut roots = vec![root.display().to_string()];
    if let Ok(c) = root.canonicalize() {
        roots.push(c.display().to_string());
    }
    let pkgs = packages(&String::from_utf8_lossy(&listing.stdout), &roots);
    if pkgs.is_empty() {
        bail!("`go list ./...` found no package: {}", last_line(&String::from_utf8_lossy(&listing.stderr)));
    }
    let dir_of: HashMap<&str, &str> = pkgs.iter().map(|p| (p.import.as_str(), p.dir.as_str())).collect();

    // the test's symbol in the graph: (directory, name) → its `_test.go` file
    let mut test_file: HashMap<(&str, &str), &str> = HashMap::new();
    for s in &g.syms {
        if s.sym.test && s.sym.kind.is_callable() && s.file.ends_with("_test.go") {
            let dir = s.file.rsplit_once('/').map_or("", |(d, _)| d);
            test_file.entry((dir, s.sym.name.as_str())).or_insert(s.file.as_str());
        }
    }
    let owner_of = owners_at(g, true);

    let scratch = Scratch(root.join(".gitnext").join(format!("learn-go-{}", std::process::id())));
    std::fs::create_dir_all(&scratch.0)?;
    let log = scratch.0.join("out.log");
    let prof = scratch.0.join("cover.out");
    let coverpkg = format!("-coverpkg={module}/...");

    let mut pairs = BTreeSet::new();
    let (mut notes, mut not_built, mut with_main, mut unknown, mut alone_only, mut failing) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut ran = 0usize;
    for (k, p) in pkgs.iter().enumerate().filter(|(_, p)| p.tests) {
        let cwd = root.join(&p.dir);
        let shown = if p.dir.is_empty() { "." } else { p.dir.as_str() };
        let out = scratch.0.join(k.to_string());
        std::fs::create_dir_all(&out)?;
        let built = timed(Command::new(go).args(["test", "-c", "-cover", &coverpkg, "-o"]).arg(format!("{}/", out.display())).arg(".").current_dir(&cwd), &log, BUILD_LIMIT)?;
        let binary = std::fs::read_dir(&out).ok().and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "test")));
        let Some(binary) = binary.filter(|_| built.ok) else {
            not_built.push(format!("{shown} ({})", if built.timed_out { "the build took longer than 10 minutes".to_string() } else { last_line(&built.text).to_string() }));
            continue;
        };
        if test_file.contains_key(&(p.dir.as_str(), "TestMain")) {
            with_main.push(shown.to_string());
        }
        let listed = timed(Command::new(&binary).args(["-test.list", "^(Test|Example|Fuzz)"]).current_dir(&cwd), &log, TEST_LIMIT)?;
        let mut failed_alone = Vec::new();
        for name in listed.text.lines().map(str::trim).filter(|n| is_test_name(n)) {
            let Some(&file) = test_file.get(&(p.dir.as_str(), name)) else {
                unknown.push(format!("{shown}: {name}"));
                continue;
            };
            let _ = std::fs::remove_file(&prof);
            let one = timed(
                Command::new(&binary).arg(format!("-test.run=^{name}$")).args(["-test.count=1", &format!("-test.timeout={TEST_TIMEOUT}")]).arg(format!("-test.coverprofile={}", prof.display())).current_dir(&cwd),
                &log,
                TEST_LIMIT,
            )?;
            ran += 1;
            if !one.ok {
                failed_alone.push(name.to_string());
            }
            let text = std::fs::read_to_string(&prof).unwrap_or_default();
            let _ = std::fs::remove_file(&prof);
            for (f, line) in profile_lines(&text, &dir_of) {
                if let Some(Some(i)) = owner_of.get(f.as_str()).and_then(|o| o.get(line as usize)) {
                    pairs.insert((*i, file.to_string(), name.to_string()));
                }
            }
        }
        if !failed_alone.is_empty() {
            // failing alone and passing with the rest of the package: depends on another test
            let all = timed(Command::new(&binary).args(["-test.v", "-test.count=1", "-test.timeout=10m"]).current_dir(&cwd), &log, BUILD_LIMIT)?;
            let failed = failed_in(&all.text);
            for n in failed_alone {
                let passed = all.text.lines().any(|l| l.strip_prefix("--- PASS: ").and_then(|r| r.split_whitespace().next()) == Some(n.as_str()));
                if passed && !failed.contains(&n) {
                    alone_only.push(format!("{shown}: {n}"));
                } else {
                    failing.push(format!("{shown}: {n}"));
                }
            }
        }
    }
    let _ = std::fs::remove_file(&log);
    if !not_built.is_empty() {
        notes.push(format!("Go: {} packages' test binaries did not build, their tests are not in the map: {}", not_built.len(), some(&not_built)));
    }
    if !with_main.is_empty() {
        notes.push(format!("Go: {} packages have TestMain, it ran once for every test: {}", with_main.len(), some(&with_main)));
    }
    if !alone_only.is_empty() {
        notes.push(format!("Go: {} tests fail when run alone but pass with their package (they depend on another test; what they ran alone is kept): {}", alone_only.len(), some(&alone_only)));
    }
    if !failing.is_empty() {
        notes.push(format!("Go: {} tests fail: {}", failing.len(), some(&failing)));
    }
    if !unknown.is_empty() {
        notes.push(format!("Go: {} tests have no symbol in the index and were not run: {}", unknown.len(), some(&unknown)));
    }
    Ok(Run {
        source: "go test -cover",
        command: format!("go test -c -cover {coverpkg} per package, then each test alone: <pkg>.test -test.run '^Name$' -test.coverprofile=… ({ran} tests)"),
        seconds: started.elapsed().as_secs_f32(),
        tests_failed: !alone_only.is_empty() || !failing.is_empty(),
        pairs,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_gives_the_first_line_of_the_blocks_that_ran_by_file_of_the_repository() {
        let dir_of: HashMap<&str, &str> = [("example.com/demo", ""), ("example.com/demo/calc", "calc")].into_iter().collect();
        let text = "mode: set\n\
                    example.com/demo/calc/calc.go:5.24,5.40 1 1\n\
                    example.com/demo/calc/calc.go:7.24,9.2 1 0\n\
                    example.com/demo/main.go:3.13,5.2 2 1\n\
                    golang.org/x/other/x.go:1.1,2.2 1 1\n";
        let got: Vec<(String, u32)> = profile_lines(text, &dir_of).into_iter().collect();
        assert_eq!(got, vec![("calc/calc.go".to_string(), 5), ("main.go".to_string(), 3)]);
    }

    #[test]
    fn packages_are_named_by_their_directory_relative_to_the_repository() {
        let roots = vec!["/r/demo".to_string(), "/private/r/demo".to_string()];
        let listing = "example.com/demo\t/r/demo\t0\t0\nexample.com/demo/calc\t/private/r/demo/calc\t2\t1\nexample.com/demo/x\t/elsewhere/x\t1\t0\n";
        let got: Vec<(String, String, bool)> = packages(listing, &roots).into_iter().map(|p| (p.import, p.dir, p.tests)).collect();
        assert_eq!(got, vec![("example.com/demo".into(), "".into(), false), ("example.com/demo/calc".into(), "calc".into(), true)]);
    }

    #[test]
    fn only_top_level_failures_of_a_verbose_run_are_counted() {
        let v = "=== RUN   TestA\n--- FAIL: TestA (0.00s)\n    --- FAIL: TestA/sub (0.00s)\n--- PASS: TestB (0.01s)\nFAIL\n";
        assert_eq!(failed_in(v).into_iter().collect::<Vec<_>>(), vec!["TestA".to_string()]);
    }

    #[test]
    fn tests_examples_and_fuzz_targets_are_run_but_not_test_main() {
        assert!(is_test_name("TestDouble") && is_test_name("ExampleCommand_Execute") && is_test_name("FuzzParse"));
        assert!(!is_test_name("TestMain") && !is_test_name("BenchmarkX") && !is_test_name("ok  \texample.com/demo"));
    }

    #[test]
    fn without_go_mod_or_go_the_message_says_what_is_missing() {
        let dir = std::env::temp_dir().join(format!("gitnext-learn-go-ready-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let e = ready("go", &dir).unwrap_err().to_string();
        assert!(e.contains("no go.mod"), "{e}");
        std::fs::write(dir.join("go.mod"), "module example.com/demo\n").unwrap();
        let e = ready("/nonexistent/bin/go", &dir).unwrap_err().to_string();
        assert!(e.contains("is not in PATH") && e.contains("go.dev"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_list_of_names_is_cut() {
        let names: Vec<String> = (1..=7).map(|i| format!("T{i}")).collect();
        assert_eq!(some(&names), "T1, T2, T3, T4, T5 and 2 more");
    }
}
