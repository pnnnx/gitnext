//! `why <test>` proves which change since the tests last passed breaks the test: each change is
//! undone on its own and the test run again. Here a real project (Python, pytest) gets a bug and a
//! meant change after its map was learned; the bug is found, and every file is as it was after.

use gitnext_ops::gitnext_index::{Graph, Index, TestMap};
use gitnext_ops::prove::{self, Hunk};
use gitnext_ops::why;
use std::path::{Path, PathBuf};
use std::process::Command;

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-prove-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn graph(dir: &Path) -> Graph {
    let mut idx = Index::open(dir).unwrap();
    idx.refresh().unwrap();
    idx.graph().unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

const CALC: &str = "def bigger(a, b):\n    return a > b\n\n\ndef pick(a, b):\n    if bigger(a, b):\n        return a\n    return b\n\n\ndef double(x):\n    return x * 2\n";
const TESTS: &str = "from pkg.calc import pick, double\n\n\ndef test_pick():\n    assert pick(3, 2) == 3\n    assert pick(1, 5) == 5\n\n\ndef test_double():\n    assert double(4) == 8\n";

#[test]
fn hunks_of_a_diff_are_undone_one_at_a_time() {
    let diff = "diff --git a/src/a.py b/src/a.py\n--- a/src/a.py\n+++ b/src/a.py\n@@ -2 +2 @@ def f\n-    return 1\n+    return 2\n@@ -5,0 +6,2 @@ def g\n+    x = 1\n+    y = 2\n@@ -9,2 +10,0 @@ def h\n-    a\n-    b\ndiff --git a/gone.py b/gone.py\n--- a/gone.py\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
    let h = prove::parse_hunks(diff);
    assert_eq!(h.len(), 3, "{h:?}");
    assert_eq!(h[0], Hunk { file: "src/a.py".into(), new: 2, count: 1, minus: vec!["    return 1".into()], plus: vec!["    return 2".into()] });
    let now = "l1\n    return 2\nl3\nl4\nl5\n    x = 1\n    y = 2\nl8\nl9\nl10\n";
    // a changed line, added lines, removed lines: each put back alone
    assert_eq!(prove::undone(now, &h[0]), now.replace("return 2", "return 1"));
    assert_eq!(prove::undone(now, &h[1]), "l1\n    return 2\nl3\nl4\nl5\nl8\nl9\nl10\n");
    assert_eq!(prove::undone(now, &h[2]), "l1\n    return 2\nl3\nl4\nl5\n    x = 1\n    y = 2\nl8\nl9\nl10\n    a\n    b\n");
}

#[test]
fn the_change_that_breaks_the_test_is_proven_and_the_rest_is_left_alone() {
    let dir = project("e2e", &[("pkg/__init__.py", ""), ("pkg/calc.py", CALC), ("tests/__init__.py", ""), ("tests/test_calc.py", TESTS)]);
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "tests pass"]);
    let commit = git(&dir, &["rev-parse", "HEAD"]);
    // the map as `learn-tests` leaves it: test_pick ran bigger and pick
    let g = graph(&dir);
    let ran: Vec<(String, String, String, String)> = ["bigger", "pick"]
        .iter()
        .map(|n| {
            let i = (0..g.syms.len()).find(|&i| g.syms[i].sym.name == *n).unwrap();
            (g.unique_name(i), g.syms[i].sym.body_hash.clone(), "tests/test_calc.py".to_string(), "test_pick".to_string())
        })
        .collect();
    TestMap::write(&dir, "test", &ran).unwrap();
    TestMap::open_existing(&dir).unwrap().set_meta("commit", commit.trim()).unwrap();
    // since then: the bug, and a change that is meant (it keeps test_double passing)
    let changed = CALC.replace("return a > b", "return a < b").replace("return x * 2", "return 2 * x");
    std::fs::write(dir.join("pkg/calc.py"), &changed).unwrap();
    let g = graph(&dir);
    let t = why::find_tests(&g, "test_pick");
    let w = why::why(&dir, &g, &t, std::time::Duration::from_secs(120));
    assert!(w.failed(), "the test must fail: {}", why::render(&g, &dir, &w));
    let text = why::render(&g, &dir, &w);
    let p = w.proof.as_ref().unwrap_or_else(|| panic!("no proof: {text}"));
    let cause = p.cause.as_ref().unwrap_or_else(|| panic!("no cause: {text}"));
    assert_eq!((cause.file.as_str(), cause.minus.clone(), cause.plus.clone()), ("pkg/calc.py", vec!["    return a > b".to_string()], vec!["    return a < b".to_string()]));
    // bigger is the most suspect: the first change tried is the cause
    assert_eq!((p.changes, p.tried), (2, 1), "{text}");
    assert!(text.contains("Undoing the change in `bigger` (pkg/calc.py:2) makes the test pass"), "{text}");
    assert!(text.contains("  - return a > b\n  + return a < b\n"), "{text}");
    assert!(text.contains("the other change since then is not the cause of this failure — keep it."), "{text}");
    // one line that stands in one place: it is put back by itself
    assert!(text.contains("next: `gitnext edit replace \"return a < b\"` with \"return a > b\" as the code"), "{text}");
    assert_eq!(p.fixed.as_deref(), Some("def bigger(a, b):\n    return a > b"));
    // the files are as they were, and nothing is left behind
    assert_eq!(std::fs::read_to_string(dir.join("pkg/calc.py")).unwrap(), changed);
    assert!(!std::fs::read_dir(dir.join(".gitnext")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("prove-")));
    // the tools' wording of the next step
    let tools = why::Next { replace: "replace(symbol=\"{}\", code=<the whole corrected function>)", check: "check() with no arguments", line: "replace(symbol={line}, code={was})" };
    assert!(why::render_for(&g, &dir, &w, &tools).contains("next: replace(symbol=\"return a < b\", code=\"return a > b\") — puts the line back as it was; then check() with no arguments."));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_change_left_undone_by_a_killed_why_is_put_back() {
    let dir = project("recover", &[("a.py", "x = 1\n")]);
    // what a `why` killed while its test ran leaves: the file as it wrote it, and the note
    let note = dir.join(".gitnext").join("prove-4194303");
    std::fs::create_dir_all(&note).unwrap();
    std::fs::write(note.join("file"), "a.py").unwrap();
    std::fs::write(note.join("was"), "x = 2\n").unwrap();
    std::fs::write(note.join("wrote"), "x = 1\n").unwrap();
    assert_eq!(prove::recover(&dir), ["a.py"]);
    assert_eq!(std::fs::read_to_string(dir.join("a.py")).unwrap(), "x = 2\n");
    assert!(!note.exists());
    // a file somebody wrote since is theirs: left as it is
    std::fs::create_dir_all(&note).unwrap();
    std::fs::write(note.join("file"), "a.py").unwrap();
    std::fs::write(note.join("was"), "x = 3\n").unwrap();
    std::fs::write(note.join("wrote"), "x = 9\n").unwrap();
    assert!(prove::recover(&dir).is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("a.py")).unwrap(), "x = 2\n");
    let _ = std::fs::remove_dir_all(&dir);
}
