//! `gitnext check`: after a change — build the project and run the tests of what changed,
//! and answer with a verdict. One step for the agent instead of three (edit → build → tests),
//! and the judgement comes from the compiler and the tests, not from the model.
//!
//! A test that fails is not always the change's doing: it may fail on the last commit too (a
//! test that needs a network, a permission, another machine). So the tests that failed are run
//! once more on the code as the last commit has it — the changed files are put back for that run
//! and the changes return when it ends — and a test that fails both ways is said to be not
//! caused by the change. Without this, models undid correct changes or went to "fix" the test.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gitnext_index::Graph;

use crate::locate::{self, TestRef};

/// At most this many tests are run for one check (the most likely to fail of each changed symbol).
const MAX_TESTS: usize = 30;

pub struct Step {
    /// `build` or `tests`
    pub what: &'static str,
    pub command: String,
    pub ok: bool,
    pub timed_out: bool,
    pub seconds: f32,
    /// the end of the command's output (stdout and stderr together)
    pub output: String,
    /// what failed, where and with what message — from the whole output, not its end
    pub digest: crate::digest::Digest,
    /// repo locations named anywhere in the output (`path:line`), in the order they came
    pub frames: Vec<crate::locate::Frame>,
    /// the tests of this step that fail on the last commit too; `None`: not looked at (the step
    /// passed, or it is the build), or it could not be told
    pub before: Option<BTreeSet<String>>,
}

impl Step {
    /// The tests that failed, by name; empty when the output is not a test run's.
    fn failed_tests(&self) -> BTreeSet<&str> {
        if self.what != "tests" || self.ok || self.timed_out || self.digest.kind != "tests" {
            return BTreeSet::new();
        }
        self.digest.failures.iter().map(|f| f.what.as_str()).collect()
    }

    /// The failures the change is not the cause of: they are there without it too.
    pub fn not_mine(&self) -> Vec<&str> {
        let Some(before) = &self.before else { return Vec::new() };
        self.failed_tests().into_iter().filter(|t| before.contains(*t)).collect()
    }

    /// Every test that fails here fails on the last commit too: the change broke nothing.
    pub fn as_before(&self) -> bool {
        let failed = self.failed_tests();
        !failed.is_empty() && self.not_mine().len() == failed.len()
    }
}

pub struct Check {
    /// symbols whose code differs from the last commit
    pub changed: Vec<usize>,
    pub tests: usize,
    pub steps: Vec<Step>,
    /// what the user should know: changes brought back after an interrupted check
    pub notes: Vec<String>,
}

impl Check {
    pub fn ok(&self) -> bool {
        self.steps.iter().all(|s| s.ok || s.as_before())
    }
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// The changed files as the last commit has them, for the time of one test run. What was in
/// them is kept in `.gitnext/check-before-<pid>/` with a list, and comes back when this is
/// dropped — or at the next `check`, if the process was killed in between.
struct Before {
    root: PathBuf,
    dir: PathBuf,
    /// (path in the repository, the file was there, what this wrote into it)
    files: Vec<(String, bool, Vec<u8>)>,
}

const BEFORE_DIR: &str = "check-before-";
const MANIFEST: &str = "files.tsv";
/// More changed files than this are not swapped: the check answers as it did.
const MAX_SWAPPED: usize = 300;

fn second_of(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Waits until the clock's second is past the last write of these files, and gives that second.
/// Python keeps compiled code by the source's size and the second it was written in, other tools
/// by the time alone: a file swapped within the same second would run as the old one.
fn past_the_second_of(files: &[PathBuf]) -> u64 {
    let last = files.iter().filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).map(second_of).max().unwrap_or(0);
    let limit = Instant::now() + Duration::from_secs(3); // a file dated in the future is not waited for
    // the file system's clock runs a tick behind: a write just past the second is still dated in it
    while second_of(std::time::SystemTime::now() - Duration::from_millis(60)) <= last && Instant::now() < limit {
        std::thread::sleep(Duration::from_millis(20));
    }
    last
}

/// Writes the file so that it is dated in a later second than `after`.
fn write_later(file: &Path, bytes: &[u8], after: u64) -> std::io::Result<()> {
    for _ in 0..50 {
        std::fs::write(file, bytes)?;
        if std::fs::metadata(file).and_then(|m| m.modified()).map(second_of).unwrap_or(u64::MAX) > after {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

impl Before {
    fn take(root: &Path) -> Option<Before> {
        if std::env::var("GITNEXT_CHECK_BEFORE").as_deref() == Ok("0") {
            return None;
        }
        // modified and deleted files of the commit; new ones stay where they are
        let listed = git(root, &["diff", "--name-status", "--no-renames", "--relative", "-z", "HEAD"])?;
        let listed = String::from_utf8_lossy(&listed).into_owned();
        let mut parts = listed.split('\0');
        let mut paths = Vec::new();
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            if matches!(status, "M" | "D" | "T") {
                paths.push(path.to_string());
            }
        }
        if paths.is_empty() || paths.len() > MAX_SWAPPED {
            return None;
        }
        let dir = root.join(gitnext_index::GITNEXT_DIR).join(format!("{BEFORE_DIR}{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let mut before = Before { root: root.to_path_buf(), dir, files: Vec::new() };
        let mut manifest = String::new();
        let mut committed = Vec::new();
        for (n, path) in paths.iter().enumerate() {
            let file = root.join(path);
            let meta = std::fs::symlink_metadata(&file).ok();
            if meta.as_ref().is_some_and(|m| !m.is_file()) {
                return None; // a link or a folder in a file's place: not something to write through
            }
            let was = git(root, &["show", &format!("HEAD:./{path}")])?;
            if meta.is_some() {
                std::fs::copy(&file, before.dir.join(n.to_string())).ok()?;
            }
            manifest.push_str(&format!("{n}\t{}\t{path}\n", u8::from(meta.is_some())));
            committed.push((path.clone(), meta.is_some(), was));
        }
        // the list is on disk before the first file is touched
        std::fs::write(before.dir.join(MANIFEST), manifest).ok()?;
        let after = past_the_second_of(&committed.iter().map(|(path, _, _)| root.join(path)).collect::<Vec<_>>());
        for (path, existed, was) in committed {
            let file = root.join(&path);
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if write_later(&file, &was, after).is_err() {
                return None; // what was swapped so far comes back in `drop`
            }
            before.files.push((path, existed, was));
        }
        Some(before)
    }
}

impl Drop for Before {
    fn drop(&mut self) {
        let after = past_the_second_of(&self.files.iter().map(|(path, _, _)| self.root.join(path)).collect::<Vec<_>>());
        let mut kept = false;
        for (n, (path, existed, was)) in self.files.iter().enumerate() {
            let file = self.root.join(path);
            // somebody wrote the file while the tests ran: theirs stays, ours is left in the folder
            if std::fs::read(&file).ok().as_ref() != Some(was) {
                kept |= *existed;
                continue;
            }
            let back = if *existed { std::fs::read(self.dir.join(n.to_string())).and_then(|b| write_later(&file, &b, after)) } else { std::fs::remove_file(&file) };
            kept |= back.is_err();
        }
        if !kept {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Changes a killed `check` left in `.gitnext/check-before-<pid>/` are put back.
fn recover(root: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join(gitnext_index::GITNEXT_DIR)) else { return notes };
    for dir in entries.flatten().map(|e| e.path()) {
        let Some(pid) = dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix(BEFORE_DIR)) else { continue };
        let alive = Command::new("kill").args(["-0", pid]).stdin(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if alive && pid != std::process::id().to_string() {
            continue; // another check is running its tests right now
        }
        let Ok(manifest) = std::fs::read_to_string(dir.join(MANIFEST)) else {
            let _ = std::fs::remove_dir_all(&dir); // killed before any file was touched
            continue;
        };
        let mut back = Vec::new();
        for row in manifest.lines() {
            let mut col = row.splitn(3, '\t');
            let (Some(n), Some(existed), Some(path)) = (col.next(), col.next(), col.next()) else { continue };
            let file = root.join(path);
            // only a file that still is what the last commit has: anything else was written since
            let committed = git(root, &["show", &format!("HEAD:./{path}")]);
            if committed.is_none() || std::fs::read(&file).ok() != committed {
                continue;
            }
            let done = if existed == "1" { std::fs::read(dir.join(n)).and_then(|b| std::fs::write(&file, b)) } else { std::fs::remove_file(&file) };
            if done.is_ok() {
                back.push(path.to_string());
            }
        }
        if !back.is_empty() {
            notes.push(format!("an earlier check was interrupted while it ran the tests on the last commit; your changes are back in {}", back.join(", ")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    notes
}

/// What is known about a test command on a commit: the tests that fail there. Kept in
/// `.gitnext/check-before.tsv` (commit, command, test; a row without a test: the command was run),
/// so the same failing check asks once.
fn known_before(root: &Path, commit: &str, command: &str) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(root.join(gitnext_index::GITNEXT_DIR).join("check-before.tsv")).ok()?;
    let rows: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix(commit)?.strip_prefix('\t')?.strip_prefix(command)?.strip_prefix('\t')).collect();
    (!rows.is_empty()).then(|| rows.into_iter().filter(|t| !t.is_empty()).map(String::from).collect())
}

fn remember_before(root: &Path, commit: &str, command: &str, failed: &BTreeSet<String>) {
    let file = root.join(gitnext_index::GITNEXT_DIR).join("check-before.tsv");
    // rows of other commits are of no use any more
    let mut text: String = std::fs::read_to_string(&file).unwrap_or_default().lines().filter(|l| l.starts_with(commit)).map(|l| format!("{l}\n")).collect();
    for test in std::iter::once("").chain(failed.iter().map(String::as_str)) {
        text.push_str(&format!("{commit}\t{command}\t{test}\n"));
    }
    let _ = std::fs::write(file, text);
}

/// For the test steps that failed: which of their failures are there on the last commit too.
fn look_before(root: &Path, g: &Graph, steps: &mut [Step], timeout: Duration) {
    let failing: Vec<usize> = (0..steps.len()).filter(|&i| !steps[i].failed_tests().is_empty()).collect();
    let Some(commit) = git(root, &["rev-parse", "HEAD"]).map(|b| String::from_utf8_lossy(&b).trim().to_string()).filter(|c| !c.is_empty()) else { return };
    let command_of = |s: &Step| s.command.replace(['\t', '\n'], " ");
    for &i in &failing {
        steps[i].before = known_before(root, &commit, &command_of(&steps[i]));
    }
    let unknown: Vec<usize> = failing.into_iter().filter(|&i| steps[i].before.is_none()).collect();
    if unknown.is_empty() {
        return;
    }
    let Some(swapped) = Before::take(root) else { return };
    let runs: Vec<Step> = unknown.iter().map(|&i| run(root, g, "tests", &steps[i].command, timeout)).collect();
    drop(swapped);
    for (i, was) in unknown.into_iter().zip(runs) {
        // passed, or failed as a test run does; a build that breaks or a timeout tells nothing
        let failed: Option<BTreeSet<String>> = if was.ok {
            Some(BTreeSet::new())
        } else {
            let names: BTreeSet<String> = was.failed_tests().into_iter().map(String::from).collect();
            (!names.is_empty()).then_some(names)
        };
        if let Some(failed) = failed {
            remember_before(root, &commit, &command_of(&steps[i]), &failed);
            steps[i].before = Some(failed);
        }
    }
}

/// Symbols touched by the uncommitted changes: the innermost function / method / type around
/// every changed line (`git diff HEAD`), and everything in files git does not know yet.
pub fn changed_symbols(root: &Path, g: &Graph) -> Vec<usize> {
    let git = |args: &[&str]| Command::new("git").args(args).current_dir(root).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut ranges: Vec<(String, u32, u32)> = Vec::new();
    let mut file = String::new();
    for line in git(&["diff", "HEAD", "--unified=0", "--no-color"]).lines() {
        if let Some(p) = line.strip_prefix("+++ b/") {
            file = p.to_string();
        } else if line.starts_with("+++ ") {
            file.clear(); // deleted file
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@
            let Some(new) = rest.split(' ').find(|x| x.starts_with('+')) else { continue };
            let mut it = new[1..].split(',');
            let start: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let len: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
            if !file.is_empty() && start > 0 {
                ranges.push((file.clone(), start, start + len.max(1) - 1)); // a pure deletion still marks its line
            }
        }
    }
    for f in git(&["ls-files", "--others", "--exclude-standard"]).lines() {
        ranges.push((f.to_string(), 1, u32::MAX));
    }
    let mut out: Vec<usize> = Vec::new();
    for (f, a, b) in &ranges {
        // innermost symbol that contains the start of the range, plus every symbol inside it
        let inside = |s: &gitnext_index::SymbolRow| s.sym.start_line <= *b && s.sym.end_line >= *a;
        let mut hit: Vec<usize> = (0..g.syms.len())
            .filter(|&i| g.syms[i].file == *f && inside(&g.syms[i]) && (g.syms[i].sym.kind.is_callable() || g.syms[i].sym.kind.is_type()))
            .collect();
        // a method and its class both contain the line: the method is what changed
        let all = hit.clone();
        hit.retain(|&i| !all.iter().any(|&j| j != i && g.syms[j].sym.start_line >= g.syms[i].sym.start_line && g.syms[j].sym.end_line <= g.syms[i].sym.end_line && g.syms[j].sym.end_line - g.syms[j].sym.start_line < g.syms[i].sym.end_line - g.syms[i].sym.start_line));
        for i in hit {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    out
}

/// How to check that the changed files still compile, per language present among them.
pub fn build_commands(root: &Path, files: &[&str]) -> Vec<String> {
    let has = |fam: &str| files.iter().any(|f| gitnext_parse::family_of(f) == Some(fam));
    let exists = |p: &str| root.join(p).exists();
    let mut v = Vec::new();
    if has("rust") && exists("Cargo.toml") {
        v.push("cargo check --quiet --tests".to_string());
    }
    if has("go") && exists("go.mod") {
        v.push("go build ./... && go test -count=1 -run '^$' ./... >/dev/null".to_string());
    }
    if has("python") {
        let py = if exists(".venv/bin/python") { ".venv/bin/python" } else { "python3" };
        let list: Vec<&str> = files.iter().copied().filter(|f| f.ends_with(".py")).collect();
        // -f: compileall trusts a .pyc written in the same second as the edit
        v.push(format!("{py} -m compileall -q -f {}", list.join(" ")));
    }
    if has("js") && exists("tsconfig.json") {
        v.push("npx tsc --noEmit -p .".to_string());
    }
    if has("java") {
        if exists("pom.xml") {
            v.push("mvn -q -B compile".to_string());
        } else if exists("gradlew") {
            v.push("./gradlew -q compileJava".to_string());
        }
    }
    v
}

/// The commands that run exactly these tests, as the project's own environment runs them.
pub fn test_commands(g: &Graph, root: &Path, tests: &[TestRef]) -> Vec<String> {
    locate::run_commands(g, root, tests)
        .into_iter()
        .filter(|c| !c.starts_with('#')) // C/C++: no single way to run tests
        .map(|c| if c.starts_with("pytest ") && root.join(".venv/bin/pytest").exists() { format!(".venv/bin/{c}") } else { c })
        .collect()
}

pub fn run(root: &Path, g: &Graph, what: &'static str, command: &str, timeout: Duration) -> Step {
    // one file per run: two checks of one process (a server's, a test's) must not share it
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let log = std::env::temp_dir().join(format!("gitnext-check-{}-{}-{}.log", std::process::id(), what, RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let started = Instant::now();
    let (mut ok, mut timed_out) = (false, false);
    if let Ok(out) = std::fs::File::create(&log) {
        let err = out.try_clone().ok();
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command).current_dir(root).stdin(Stdio::null()).stdout(out);
        if let Some(e) = err {
            cmd.stderr(e);
        }
        if let Ok(mut child) = cmd.spawn() {
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        ok = status.success();
                        break;
                    }
                    Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(40)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break;
                    }
                }
            }
        }
    }
    // a runaway log is cut to its first megabytes: the first failures are what matters
    let text = std::fs::read(&log).map(|b| String::from_utf8_lossy(&b[..b.len().min(8 << 20)]).into_owned()).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    let lines: Vec<&str> = text.lines().collect();
    let output = lines[lines.len().saturating_sub(25)..].join("\n");
    let (mut digest, frames) = if ok { Default::default() } else { (crate::digest::digest(&text), locate::frames(g, root, &text)) };
    // paths as the project has them, not as the tool printed them (`/abs/repo/command.go`, `./command.go`)
    for f in &mut digest.failures {
        if let Some(rel) = f.file.as_deref().and_then(|p| locate::match_file(g, root, p)) {
            f.file = Some(rel);
        }
    }
    Step { what, command: command.to_string(), ok, timed_out, seconds: started.elapsed().as_secs_f32(), output, digest, frames, before: None }
}

/// Build, then the tests most likely to fail for every changed symbol. Stops at a failed build.
pub fn check(root: &Path, g: &Graph, timeout: Duration) -> Check {
    let notes = recover(root);
    let changed = changed_symbols(root, g);
    let mut files: Vec<&str> = changed.iter().map(|&i| g.syms[i].file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let mut steps = Vec::new();
    for c in build_commands(root, &files) {
        steps.push(run(root, g, "build", &c, timeout));
        if !steps.last().is_some_and(|s| s.ok) {
            return Check { changed, tests: 0, steps, notes };
        }
    }
    let mut tests: Vec<TestRef> = Vec::new();
    for &i in &changed {
        // a changed test is run itself; other code brings the tests most likely to fail for it
        let mine = if g.syms[i].sym.test { vec![TestRef { symbol: i, how: "changed".into() }] } else { locate::tests_for_at(g, root, i, 4).0.into_iter().take(locate::TOP_TESTS).collect() };
        for t in mine {
            if tests.len() < MAX_TESTS && !tests.iter().any(|x| x.symbol == t.symbol) && locate::is_runnable(g, t.symbol) {
                tests.push(t);
            }
        }
    }
    for c in test_commands(g, root, &tests) {
        steps.push(run(root, g, "tests", &c, timeout));
    }
    look_before(root, g, &mut steps, timeout);
    Check { changed, tests: tests.len(), steps, notes }
}

pub fn render(g: &Graph, c: &Check) -> String {
    let notes: String = c.notes.iter().map(|n| format!("note: {n}\n")).collect();
    if c.changed.is_empty() {
        return format!("{notes}nothing to check: no code differs from the last commit\n");
    }
    let names: Vec<&str> = c.changed.iter().take(8).map(|&i| g.short_name(i)).collect();
    let mut s = format!("check: {} changed ({}{})\n", crate::changes::plural(c.changed.len(), "symbol"), names.join(", "), if c.changed.len() > 8 { " …" } else { "" });
    let list = |names: &[&str]| format!("{}{}", names.iter().take(5).copied().collect::<Vec<_>>().join(", "), if names.len() > 5 { " …" } else { "" });
    for st in &c.steps {
        let not_mine = st.not_mine();
        let state = if st.ok { "ok" } else if st.timed_out { "TIMED OUT" } else if st.as_before() { "AS BEFORE" } else { "FAILED" };
        let _ = writeln!(s, "{:<5} {:<9} {:>5.1}s  {}", st.what, state, st.seconds, st.command);
        if st.as_before() {
            // no details: they are not this change's to fix
            let _ = writeln!(s, "  {} without your change too — not caused by it, leave {}: {}", if not_mine.len() == 1 { "1 test fails".to_string() } else { format!("{} tests fail", not_mine.len()) }, if not_mine.len() == 1 { "it" } else { "them" }, list(&not_mine));
        } else if !st.ok {
            // what failed, where, the message — not the tool's whole output
            let text = if st.digest.failures.is_empty() { st.output.clone() } else { crate::digest::render(&st.digest, 5) };

            for l in text.lines() {
                let _ = writeln!(s, "  {l}");
            }
            if !not_mine.is_empty() {
                let _ = writeln!(s, "  of these, failing without your change too (not caused by it): {}", list(&not_mine));
            } else if st.before.is_some() && !st.failed_tests().is_empty() {
                let _ = writeln!(s, "  without your change {} — the change causes this", if st.failed_tests().len() == 1 { "this test passes" } else { "these tests pass" });
            }
        }
    }
    s.push_str(&notes);
    let file = &g.syms[c.changed[0]].file;
    let as_before: Vec<&str> = c.steps.iter().filter(|x| x.as_before()).flat_map(|x| x.not_mine()).collect();
    if let Some(bad) = c.steps.iter().find(|x| !x.ok && !x.as_before()) {
        let _ = writeln!(s, "verdict: FAILED — {} ({})", bad.what, if bad.what == "build" { "the tests were not run" } else { "see the output above" });
    } else if c.steps.iter().all(|x| x.what != "tests") {
        let _ = writeln!(s, "verdict: builds, but no test reaches the changed code through the call graph — run the project's test suite");
    } else if !as_before.is_empty() {
        let _ = writeln!(s, "verdict: ok — builds, and your change breaks no test ({} before it too: {})", if as_before.len() == 1 { "1 failed".to_string() } else { format!("{} failed", as_before.len()) }, list(&as_before));
    } else {
        let _ = writeln!(s, "verdict: ok — builds, {} pass", crate::changes::plural(c.tests, "test"));
        let _ = writeln!(s, "note: {}.", crate::trust::tests_note(file));
    }
    s
}
