//! Rust: the tests built with `-C instrument-coverage` in a target of their own, each test run
//! alone with its own profile, `llvm-profdata` + `llvm-cov export` (rustup's `llvm-tools-preview`)
//! turn the profile into the lines it ran. Measured on semver and indexmap: the map finds
//! 100% of the tests that fail when a function breaks (the call graph: 59%); the first
//! instrumented build takes 12–21 s and about 350 MB, a later map 2–5× a plain test run.
//! Doc-tests are not run.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use gitnext_index::Graph;

use super::{owners_at, Run};

/// One test may run this long; then it is stopped and counted as failed.
const TEST_TIMEOUT: Duration = Duration::from_secs(300);

/// A test binary `cargo test --no-run` built.
#[derive(Debug, PartialEq)]
struct Binary {
    exe: PathBuf,
    /// the package's directory: tests run there, as under `cargo test`
    package: PathBuf,
    /// the target's root file (`src/lib.rs`, `tests/foo.rs`)
    src: PathBuf,
    /// `lib`, `bin`, `test`, `bench`, `example`
    kind: String,
}

/// `llvm-profdata` and `llvm-cov` of the toolchain whose sysroot is given.
fn llvm_tools(sysroot: &Path) -> Result<(PathBuf, PathBuf)> {
    let find = |name: &str| -> Option<PathBuf> {
        let dirs = std::fs::read_dir(sysroot.join("lib").join("rustlib")).ok()?;
        let mut hits: Vec<PathBuf> = dirs.flatten().map(|d| d.path().join("bin").join(name)).filter(|p| p.is_file()).collect();
        hits.sort();
        hits.into_iter().next()
    };
    match (find("llvm-profdata"), find("llvm-cov")) {
        (Some(p), Some(c)) => Ok((p, c)),
        _ => bail!("llvm-profdata and llvm-cov are not installed for the toolchain in {}: rustup component add llvm-tools-preview", sysroot.display()),
    }
}

/// The test binaries in the JSON messages of `cargo test --no-run --message-format=json`.
fn test_binaries(messages: &str) -> Vec<Binary> {
    let mut out = Vec::new();
    for line in messages.lines() {
        let Ok(m) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if m["reason"] != "compiler-artifact" || m["profile"]["test"] != true {
            continue;
        }
        let (Some(exe), Some(manifest), Some(src)) = (m["executable"].as_str(), m["manifest_path"].as_str(), m["target"]["src_path"].as_str()) else { continue };
        let package = Path::new(manifest).parent().map(Path::to_path_buf).unwrap_or_default();
        let kind = m["target"]["kind"][0].as_str().unwrap_or("lib").to_string();
        out.push(Binary { exe: exe.into(), package, src: src.into(), kind });
    }
    out
}

/// The compiler's errors among the same messages, for when the build fails.
fn build_errors(messages: &str) -> String {
    let errors: Vec<String> = messages
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-message" && m["message"]["level"] == "error")
        .filter_map(|m| m["message"]["rendered"].as_str().map(str::to_string))
        .take(3)
        .collect();
    errors.join("")
}

/// `a::b: test` lines of `--list --format terse` → `a::b` (benchmarks dropped).
fn listed(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.strip_suffix(": test")).map(str::to_string).collect()
}

/// lcov → for each file under `prefix` (an absolute directory with a trailing `/`), its path
/// relative to it and the lines that ran.
fn lcov_lines(text: &str, prefix: &str) -> HashMap<String, Vec<u32>> {
    let mut out: HashMap<String, Vec<u32>> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("SF:") {
            cur = p.strip_prefix(prefix).map(str::to_string);
        } else if let (Some(da), Some(file)) = (line.strip_prefix("DA:"), &cur) {
            let mut f = da.split(',');
            if let (Some(Ok(n)), Some(c)) = (f.next().map(str::parse::<u32>), f.next()) {
                if c != "0" {
                    out.entry(file.clone()).or_default().push(n);
                }
            }
        } else if line == "end_of_record" {
            cur = None;
        }
    }
    out
}

/// Of the graph's tests with the name the test ends with — (file, qualified name), already only
/// those of the binary's files — the one that is this test: the most trailing parts of cargo's path
/// (`display::tests::test_align`) agree, then the fewest parts left over (`crate::set::tests::x`
/// before `crate::rayon::set::tests::x`), then the first file.
fn pick<'a>(candidates: &[(&'a str, &'a str)], cargo_name: &str) -> Option<&'a str> {
    let want: Vec<&str> = cargo_name.rsplit("::").collect();
    let score = |q: &str| {
        let n = q.rsplit("::").zip(&want).take_while(|(a, b)| a == *b).count();
        (n, std::cmp::Reverse(q.split("::").count() - n))
    };
    candidates.iter().map(|&(f, q)| (score(q), std::cmp::Reverse(f), f)).filter(|(s, _, _)| s.0 > 0).max().map(|(_, _, f)| f)
}

/// The source files cargo compiled into a binary, from the dep-info file it writes next to it
/// (`deps/demo-1a2b.d`), relative to the repository (`prefix`: its absolute path with a trailing
/// `/`). rustc writes the paths cargo gave it: absolute, or relative to the workspace root, which is
/// the repository's root here; spaces are escaped. `None` when there is no such file.
fn compiled_files(dep_info: &str, prefix: &str) -> Option<std::collections::HashSet<String>> {
    let mut out = std::collections::HashSet::new();
    for line in dep_info.lines() {
        let Some((_, deps)) = line.split_once(": ") else { continue };
        let deps = deps.replace("\\ ", "\u{0}");
        for d in deps.split_whitespace().map(|d| d.replace('\u{0}', " ")).filter(|d| d.ends_with(".rs")) {
            match d.strip_prefix(prefix) {
                Some(rel) => out.insert(rel.to_string()),
                None if !d.starts_with('/') => out.insert(d.trim_start_matches("./").to_string()),
                None => false,
            };
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The files a test binary is made of: an integration test is its file, the files of its
/// directory (`tests/foo/…`) and the shared ones in `tests/`; a library's or program's unit tests are
/// the package's other files. Paths are relative to the repository; `packages` are the repository's
/// package directories, so a file of a nested package is not taken for one of the outer.
fn belongs(file: &str, bin: &Binary, root: &str, packages: &[String]) -> bool {
    let rel = |p: &Path| p.to_string_lossy().strip_prefix(root).unwrap_or_default().trim_start_matches('/').to_string();
    let pkg = rel(&bin.package);
    let owner = packages.iter().filter(|p| p.is_empty() || file.starts_with(&format!("{p}/"))).max_by_key(|p| p.len());
    if owner != Some(&pkg) {
        return false;
    }
    let tests = if pkg.is_empty() { "tests/".to_string() } else { format!("{pkg}/tests/") };
    if bin.kind == "test" {
        let src = rel(&bin.src);
        // `tests/it.rs`, `tests/it/…`, and modules shared through a directory (`tests/common/mod.rs`)
        file == src || file.starts_with(&format!("{}/", src.trim_end_matches(".rs"))) || file.strip_prefix(&tests).is_some_and(|r| r.contains('/'))
    } else {
        !file.starts_with(&tests)
    }
}

/// Size of a directory tree in bytes; cargo hard-links its binaries into `debug/`, they are counted once.
fn du(dir: &Path, seen: &mut std::collections::HashSet<(u64, u64)>) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => n += du(&e.path(), seen),
            Ok(t) if t.is_file() => {
                let Ok(m) = e.metadata() else { continue };
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if !seen.insert((m.dev(), m.ino())) {
                        continue;
                    }
                }
                n += m.len();
            }
            _ => {}
        }
    }
    n
}

/// The profiles' directory goes away whatever happens.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn output(cmd: &mut Command) -> Option<String> {
    cmd.output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Runs one test alone; `Some(passed)`, `None` when it was stopped after `TEST_TIMEOUT`.
fn run_one(bin: &Binary, name: &str, profile: &Path) -> Result<Option<bool>> {
    let mut child = Command::new(&bin.exe)
        .args(["--exact", name, "--test-threads=1", "-q"])
        .env("LLVM_PROFILE_FILE", profile)
        .env("CARGO_MANIFEST_DIR", &bin.package)
        .current_dir(&bin.package)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("running {}", bin.exe.display()))?;
    let started = Instant::now();
    let mut nap = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status.success()));
        }
        if started.elapsed() > TEST_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(nap);
        nap = (nap * 2).min(Duration::from_millis(50));
    }
}

/// The lines one profile ran, the profile removed.
fn export(tools: &(PathBuf, PathBuf), exe: &Path, raw: &Path, prefix: &str) -> HashMap<String, Vec<u32>> {
    let data = raw.with_extension("profdata");
    let merged = Command::new(&tools.0).arg("merge").arg("-sparse").arg(raw).arg("-o").arg(&data).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    let lines = if merged {
        output(Command::new(&tools.1).arg("export").arg("-format=lcov").arg(format!("-instr-profile={}", data.display())).arg(exe).arg("--ignore-filename-regex=/.cargo/|/rustc/").stderr(Stdio::null()))
            .map(|t| lcov_lines(&t, prefix))
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let _ = std::fs::remove_file(raw);
    let _ = std::fs::remove_file(&data);
    lines
}

fn mb(bytes: u64) -> u64 {
    (bytes + 500_000) / 1_000_000
}

fn some(names: &[String]) -> String {
    let mut s = names.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 5 {
        s.push_str(", …");
    }
    s
}

pub(super) fn run(root: &Path, g: &Graph) -> Result<Run> {
    if !root.join("Cargo.toml").is_file() {
        bail!("no Cargo.toml at the root of the repository");
    }
    if output(Command::new("cargo").arg("--version")).is_none() {
        bail!("cargo is not installed: https://rustup.rs");
    }
    let sysroot = output(Command::new("rustc").args(["--print", "sysroot"]).current_dir(root)).context("`rustc --print sysroot` failed")?;
    let tools = llvm_tools(Path::new(sysroot.trim()))?;
    let started = Instant::now();

    let meta: serde_json::Value = output(Command::new("cargo").args(["metadata", "--format-version", "1", "--no-deps"]).current_dir(root).stderr(Stdio::null()))
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let target = meta["target_directory"].as_str().map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let cov = target.join("gitnext-cov");
    let scratch = Scratch(cov.join(format!("profiles-{}", std::process::id())));
    std::fs::create_dir_all(&scratch.0)?;

    let flags = format!("{} -C instrument-coverage", std::env::var("RUSTFLAGS").unwrap_or_default()).trim().to_string();
    let command = format!("RUSTFLAGS='{flags}' CARGO_TARGET_DIR={} cargo test --workspace --no-run; each test alone: <binary> --exact <test>", cov.display());
    let built = Command::new("cargo")
        .args(["test", "--workspace", "--no-run", "--message-format=json"])
        .env("RUSTFLAGS", &flags)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", &cov)
        .env("LLVM_PROFILE_FILE", scratch.0.join("build-%p.profraw")) // build scripts are instrumented too
        .current_dir(root)
        .stderr(Stdio::piped())
        .output()
        .context("running cargo test --no-run")?;
    let messages = String::from_utf8_lossy(&built.stdout);
    let bins = test_binaries(&messages);
    if !built.status.success() || bins.is_empty() {
        let why = build_errors(&messages);
        let why = if why.is_empty() { String::from_utf8_lossy(&built.stderr).lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n") } else { why };
        bail!("the tests did not build with coverage (`{command}`):\n{}", why.trim_end());
    }

    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_s = canonical.to_string_lossy().to_string();
    let prefix = format!("{root_s}/");
    let rel = |p: &str| p.strip_prefix(&prefix).unwrap_or(if p == root_s { "" } else { p }).to_string();
    let packages: Vec<String> = meta["packages"].as_array().map(|ps| ps.iter().filter_map(|p| p["manifest_path"].as_str()).map(|m| rel(Path::new(m).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default().as_str())).collect()).unwrap_or_else(|| vec![String::new()]);
    let bins: Vec<Binary> = bins.into_iter().map(|b| Binary { package: b.package.canonicalize().unwrap_or(b.package), src: b.src.canonicalize().unwrap_or(b.src), ..b }).collect();

    // the graph's tests by name
    let mut tests_named: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for s in &g.syms {
        if s.sym.test && s.sym.kind.is_callable() {
            tests_named.entry(s.sym.name.as_str()).or_default().push((s.file.as_str(), s.sym.qualified.as_str()));
        }
    }

    // run the tests one by one; their profiles are exported by the other threads meanwhile
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4);
    let (to_export, jobs) = mpsc::channel::<(usize, PathBuf, PathBuf)>();
    let jobs = Mutex::new(jobs);
    let mut runs: Vec<(String, String, Option<bool>)> = Vec::new(); // (file, name, passed)
    let (mut unknown, mut ignored, mut timed_out) = (Vec::new(), 0usize, Vec::new());
    let lines: Vec<(usize, HashMap<String, Vec<u32>>)> = std::thread::scope(|s| -> Result<_> {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut got = Vec::new();
                    loop {
                        let job = jobs.lock().map(|j| j.recv());
                        let Ok(Ok((k, exe, raw))) = job else { break };
                        got.push((k, if raw.exists() { export(&tools, &exe, &raw, &prefix) } else { HashMap::new() }));
                    }
                    got
                })
            })
            .collect();
        for (b, bin) in bins.iter().enumerate() {
            let compiled = std::fs::read_to_string(bin.exe.with_extension("d")).ok().and_then(|t| compiled_files(&t, &prefix));
            let of_binary = |f: &str| compiled.as_ref().map_or_else(|| belongs(f, bin, &root_s, &packages), |c| c.contains(f));
            let list =|extra: &[&str]| output(Command::new(&bin.exe).args(["--list", "--format", "terse"]).args(extra).current_dir(&bin.package).stderr(Stdio::null())).map(|t| listed(&t)).unwrap_or_default();
            let skip: BTreeSet<String> = list(&["--ignored"]).into_iter().collect();
            for (t, name) in list(&[]).into_iter().enumerate() {
                if skip.contains(&name) {
                    ignored += 1;
                    continue;
                }
                let short = name.rsplit("::").next().unwrap_or(&name).to_string();
                let cands: Vec<(&str, &str)> = tests_named.get(short.as_str()).map(|c| c.iter().copied().filter(|(f, _)| of_binary(f)).collect()).unwrap_or_default();
                let Some(file) = pick(&cands, &name) else {
                    unknown.push(name);
                    continue;
                };
                let raw = scratch.0.join(format!("{b}-{t}.profraw"));
                let passed = run_one(bin, &name, &raw)?;
                if passed.is_none() {
                    timed_out.push(name.clone());
                }
                let _ = to_export.send((runs.len(), bin.exe.clone(), raw));
                runs.push((file.to_string(), short, passed));
            }
        }
        drop(to_export);
        Ok(handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect())
    })?;
    drop(scratch);

    let owner_of = owners_at(g, true);
    let mut pairs: BTreeSet<(usize, String, String)> = BTreeSet::new();
    let mut silent = Vec::new();
    for (k, files) in &lines {
        let (tfile, tname, _) = &runs[*k];
        if files.is_empty() {
            silent.push(tname.clone());
        }
        for (file, ls) in files {
            let Some(owners) = owner_of.get(file.as_str()) else { continue };
            for &l in ls {
                if let Some(Some(i)) = owners.get(l as usize) {
                    pairs.insert((*i, tfile.clone(), tname.clone()));
                }
            }
        }
    }
    silent.sort();

    let mut notes = vec![format!("the build with coverage is kept in {} ({} MB) to make the next map faster; `rm -rf {}` frees the space", cov.display(), mb(du(&cov, &mut Default::default())), cov.display())];
    if !unknown.is_empty() {
        notes.push(format!("{} of {} tests are not functions the graph knows (made by a macro?) and were not run: {}", unknown.len(), unknown.len() + runs.len(), some(&unknown)));
    }
    if !silent.is_empty() {
        notes.push(format!("{} tests wrote no coverage (the process aborted?): {}", silent.len(), some(&silent)));
    }
    if !timed_out.is_empty() {
        notes.push(format!("{} tests were stopped after {} s: {}", timed_out.len(), TEST_TIMEOUT.as_secs(), some(&timed_out)));
    }
    if ignored > 0 {
        notes.push(format!("{ignored} #[ignore] tests were not run; doc-tests are not run either"));
    }
    Ok(Run { source: "llvm-cov", command, seconds: started.elapsed().as_secs_f32(), tests_failed: runs.iter().any(|r| r.2 != Some(true)), pairs, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcov_keeps_the_lines_that_ran_in_files_of_the_repository() {
        let text = "TN:\nSF:/repo/src/lib.rs\nFN:3,f\nDA:3,1\nDA:4,0\nDA:5,12\nend_of_record\nSF:/home/u/.cargo/registry/x.rs\nDA:1,1\nend_of_record\nSF:/repo/tests/it.rs\nDA:7,1\nend_of_record\nDA:9,1\n";
        let got = lcov_lines(text, "/repo/");
        assert_eq!(got.len(), 2);
        assert_eq!(got["src/lib.rs"], [3, 5]);
        assert_eq!(got["tests/it.rs"], [7]);
    }

    #[test]
    fn the_test_binaries_are_read_from_cargos_messages() {
        let msgs = [
            r#"{"reason":"compiler-artifact","manifest_path":"/r/Cargo.toml","target":{"kind":["lib"],"src_path":"/r/src/lib.rs"},"profile":{"test":true},"executable":"/r/target/debug/deps/demo-1"}"#,
            r#"{"reason":"compiler-artifact","manifest_path":"/r/Cargo.toml","target":{"kind":["lib"],"src_path":"/r/src/lib.rs"},"profile":{"test":false},"executable":null}"#,
            r#"{"reason":"compiler-artifact","manifest_path":"/r/b/Cargo.toml","target":{"kind":["test"],"src_path":"/r/b/tests/it.rs"},"profile":{"test":true},"executable":"/r/target/debug/deps/it-2"}"#,
            "   Compiling demo v0.1.0",
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        let got = test_binaries(&msgs);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], Binary { exe: "/r/target/debug/deps/demo-1".into(), package: "/r".into(), src: "/r/src/lib.rs".into(), kind: "lib".into() });
        assert_eq!((got[1].package.to_str(), got[1].kind.as_str()), (Some("/r/b"), "test"));
        let err = r#"{"reason":"compiler-message","message":{"level":"error","rendered":"error[E0425]: cannot find value `x`\n"}}"#;
        assert_eq!(build_errors(err), "error[E0425]: cannot find value `x`\n");
    }

    #[test]
    fn the_list_names_the_tests_not_the_benchmarks() {
        assert_eq!(listed("display::tests::test_align: test\nbench_parse: benchmark\n\n"), ["display::tests::test_align"]);
    }

    #[test]
    fn without_llvm_tools_the_message_says_how_to_install_them() {
        let dir = std::env::temp_dir().join(format!("gitnext-sysroot-{}", std::process::id()));
        let bin = dir.join("lib/rustlib/x86_64-unknown-linux-gnu/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("llvm-cov"), "").unwrap();
        let err = llvm_tools(&dir).unwrap_err().to_string();
        assert!(err.contains("rustup component add llvm-tools-preview"), "{err}");
        std::fs::write(bin.join("llvm-profdata"), "").unwrap();
        assert_eq!(llvm_tools(&dir).unwrap(), (bin.join("llvm-profdata"), bin.join("llvm-cov")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_test_is_the_one_whose_path_agrees_most() {
        let c = [("src/a.rs", "demo::a::tests::test_x"), ("src/b.rs", "demo::b::tests::test_x")];
        assert_eq!(pick(&c, "b::tests::test_x"), Some("src/b.rs"));
        assert_eq!(pick(&c, "a::tests::test_x"), Some("src/a.rs"));
        // only the name agrees: the first file
        assert_eq!(pick(&c, "test_x"), Some("src/a.rs"));
        assert_eq!(pick(&c, "test_y"), None);
        assert_eq!(pick(&[], "test_x"), None);
        // indexmap: `set::tests::comparisons` is not the one of the `rayon` module
        let c = [("src/rayon/set.rs", "indexmap::rayon::set::tests::comparisons"), ("src/set/tests.rs", "indexmap::set::tests::comparisons")];
        assert_eq!(pick(&c, "set::tests::comparisons"), Some("src/set/tests.rs"));
        assert_eq!(pick(&c, "rayon::set::tests::comparisons"), Some("src/rayon/set.rs"));
    }

    #[test]
    fn the_files_of_a_binary_come_from_its_dep_info() {
        let d = "/r/target/debug/deps/demo-1: /r/src/lib.rs src/set/tests.rs /r/my\\ dir/x.rs /home/u/.cargo/registry/y.rs\n\n/r/target/debug/deps/demo-1.d: /r/src/lib.rs\n\n/r/src/lib.rs:\nsrc/set/tests.rs:\n";
        let got = compiled_files(d, "/r/").unwrap();
        let mut got: Vec<_> = got.into_iter().collect();
        got.sort();
        assert_eq!(got, ["my dir/x.rs", "src/lib.rs", "src/set/tests.rs"]);
        assert_eq!(compiled_files("", "/r/"), None);
    }

    #[test]
    fn a_binary_is_made_of_its_own_files() {
        let packages = vec![String::new(), "sub".to_string()];
        let lib = Binary { exe: "/r/t/demo".into(), package: "/r".into(), src: "/r/src/lib.rs".into(), kind: "lib".into() };
        let it = Binary { exe: "/r/t/it".into(), package: "/r".into(), src: "/r/tests/it.rs".into(), kind: "test".into() };
        assert!(belongs("src/display.rs", &lib, "/r", &packages));
        assert!(!belongs("tests/it.rs", &lib, "/r", &packages));
        assert!(!belongs("sub/src/lib.rs", &lib, "/r", &packages));
        assert!(belongs("tests/it.rs", &it, "/r", &packages));
        assert!(belongs("tests/it/helpers.rs", &it, "/r", &packages));
        assert!(belongs("tests/common/mod.rs", &it, "/r", &packages));
        assert!(!belongs("tests/other.rs", &it, "/r", &packages));
        assert!(!belongs("src/lib.rs", &it, "/r", &packages));
    }
}
