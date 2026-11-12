//! `gitnext check`: after a change — build the project and run the tests of what changed,
//! and answer with a verdict. One step for the agent instead of three (edit → build → tests),
//! and the judgement comes from the compiler and the tests, not from the model.

use std::fmt::Write as _;
use std::path::Path;
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
}

pub struct Check {
    /// symbols whose code differs from the last commit
    pub changed: Vec<usize>,
    pub tests: usize,
    pub steps: Vec<Step>,
}

impl Check {
    pub fn ok(&self) -> bool {
        self.steps.iter().all(|s| s.ok)
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

fn run(root: &Path, what: &'static str, command: &str, timeout: Duration) -> Step {
    let log = std::env::temp_dir().join(format!("gitnext-check-{}-{}.log", std::process::id(), what));
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
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    let lines: Vec<&str> = text.lines().collect();
    let output = lines[lines.len().saturating_sub(25)..].join("\n");
    Step { what, command: command.to_string(), ok, timed_out, seconds: started.elapsed().as_secs_f32(), output }
}

/// Build, then the tests most likely to fail for every changed symbol. Stops at a failed build.
pub fn check(root: &Path, g: &Graph, timeout: Duration) -> Check {
    let changed = changed_symbols(root, g);
    let mut files: Vec<&str> = changed.iter().map(|&i| g.syms[i].file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let mut steps = Vec::new();
    for c in build_commands(root, &files) {
        steps.push(run(root, "build", &c, timeout));
        if !steps.last().is_some_and(|s| s.ok) {
            return Check { changed, tests: 0, steps };
        }
    }
    let mut tests: Vec<TestRef> = Vec::new();
    for &i in &changed {
        // a changed test is run itself; other code brings the tests most likely to fail for it
        let mine = if g.syms[i].sym.test { vec![TestRef { symbol: i, how: "changed".into() }] } else { locate::tests_for(g, i, 4).into_iter().take(locate::TOP_TESTS).collect() };
        for t in mine {
            if tests.len() < MAX_TESTS && !tests.iter().any(|x| x.symbol == t.symbol) && locate::is_runnable(g, t.symbol) {
                tests.push(t);
            }
        }
    }
    for mut c in locate::run_commands(g, root, &tests) {
        if c.starts_with('#') {
            continue; // C/C++: no single way to run tests
        }
        if c.starts_with("pytest ") && root.join(".venv/bin/pytest").exists() {
            c = format!(".venv/bin/{c}");
        }
        steps.push(run(root, "tests", &c, timeout));
    }
    Check { changed, tests: tests.len(), steps }
}

pub fn render(g: &Graph, c: &Check) -> String {
    if c.changed.is_empty() {
        return "nothing to check: no code differs from the last commit\n".to_string();
    }
    let names: Vec<&str> = c.changed.iter().take(8).map(|&i| g.short_name(i)).collect();
    let mut s = format!("check: {} changed ({}{})\n", crate::changes::plural(c.changed.len(), "symbol"), names.join(", "), if c.changed.len() > 8 { " …" } else { "" });
    for st in &c.steps {
        let state = if st.ok { "ok" } else if st.timed_out { "TIMED OUT" } else { "FAILED" };
        let _ = writeln!(s, "{:<5} {:<9} {:>5.1}s  {}", st.what, state, st.seconds, st.command);
        if !st.ok {
            for l in st.output.lines() {
                let _ = writeln!(s, "  {l}");
            }
        }
    }
    let file = &g.syms[c.changed[0]].file;
    if let Some(bad) = c.steps.iter().find(|x| !x.ok) {
        let _ = writeln!(s, "verdict: FAILED — {} ({})", bad.what, if bad.what == "build" { "the tests were not run" } else { "see the output above" });
    } else if c.steps.iter().all(|x| x.what != "tests") {
        let _ = writeln!(s, "verdict: builds, but no test reaches the changed code through the call graph — run the project's test suite");
    } else {
        let _ = writeln!(s, "verdict: ok — builds, {} pass", crate::changes::plural(c.tests, "test"));
        let _ = writeln!(s, "note: {}.", crate::trust::tests_note(file));
    }
    s
}
