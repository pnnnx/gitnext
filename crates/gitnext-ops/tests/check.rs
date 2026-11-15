//! `gitnext check`: which symbols a change touched, and the build / test verdict.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gitnext_ops::check;
use gitnext_ops::gitnext_index::{Graph, Index};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-check-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    for args in [&["init", "-q"][..], &["add", "-A"], &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "start"]] {
        assert!(Command::new("git").args(args).current_dir(&dir).output().unwrap().status.success());
    }
    dir
}

fn graph(dir: &Path) -> Graph {
    let mut idx = Index::open(dir).unwrap();
    idx.refresh().unwrap();
    idx.graph().unwrap()
}

const CALC: &str = "class Calc:\n    def add(self, a, b):\n        return a + b\n\n    def sub(self, a, b):\n        return a - b\n\n\ndef helper(x):\n    return x\n";

#[test]
fn changed_symbols_are_the_innermost_ones() {
    let dir = project("changed", &[("calc.py", CALC), (".gitignore", ".gitnext/\n__pycache__/\n")]);
    assert!(check::changed_symbols(&dir, &graph(&dir)).is_empty());
    assert!(check::render(&graph(&dir), &check::check(&dir, &graph(&dir), Duration::from_secs(60))).starts_with("nothing to check"));
    // one line inside a method, and a new file
    std::fs::write(dir.join("calc.py"), CALC.replace("a - b", "a - b - 0")).unwrap();
    std::fs::write(dir.join("extra.py"), "def fresh():\n    return 1\n").unwrap();
    let g = graph(&dir);
    let mut names: Vec<&str> = check::changed_symbols(&dir, &g).into_iter().map(|i| g.syms[i].sym.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["fresh", "sub"], "the method, not its class; not the untouched `add` or `helper`");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verdict_follows_the_build() {
    let dir = project("verdict", &[("calc.py", CALC), (".gitignore", ".gitnext/\n__pycache__/\n")]);
    std::fs::write(dir.join("calc.py"), CALC.replace("a + b", "a + b + 0")).unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(60));
    assert!(c.ok(), "{}", check::render(&g, &c));
    let text = check::render(&g, &c);
    assert!(text.contains("build ok") && text.contains("verdict: builds, but no test reaches"), "{text}");
    // a syntax error: the build fails, the tests are not run, the error is shown
    std::fs::write(dir.join("calc.py"), CALC.replace("return a + b", "return a +")).unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(60));
    let text = check::render(&g, &c);
    assert!(!c.ok() && text.contains("build FAILED") && text.contains("SyntaxError") && text.contains("verdict: FAILED — build"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_commands_follow_the_languages_touched() {
    let dir = project("hang", &[("x.go", "package x\n\nfunc F() int { return 1 }\n"), ("go.mod", "module x\n")]);
    assert_eq!(check::build_commands(&dir, &["x.go"]).len(), 1);
    assert_eq!(check::build_commands(&dir, &["notes.md"]).len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

const TESTS: &str = "import os\n\nfrom calc import Calc\n\n\ndef test_add():\n    assert Calc().add(2, 3) == 5\n\n\ndef test_needs_the_other_machine():\n    assert Calc().add(1, 1) == 2\n    assert os.environ.get(\"GITNEXT_NEVER_SET\") == \"1\"\n";

fn has_pytest() -> bool {
    Command::new("pytest").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn leftovers(dir: &Path) -> Vec<String> {
    let names = std::fs::read_dir(dir.join(".gitnext")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned());
    names.filter(|n| n.starts_with("check-before-")).collect()
}

/// A test that fails on the last commit too is not the change's doing, and the check says so.
#[test]
fn a_test_that_failed_before_the_change_is_not_blamed_on_it() {
    if !has_pytest() {
        eprintln!("skipped: no pytest in PATH");
        return;
    }
    let dir = project("before", &[("calc.py", CALC), ("test_calc.py", TESTS), (".gitignore", ".gitnext/\n__pycache__/\n.pytest_cache/\n")]);
    // a change that breaks nothing: the one failing test fails without it too
    let harmless = CALC.replace("a + b", "a + b + 0");
    std::fs::write(dir.join("calc.py"), &harmless).unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(120));
    let text = check::render(&g, &c);
    assert!(c.ok(), "{text}");
    assert!(text.contains("tests AS BEFORE") && text.contains("1 test fails without your change too — not caused by it, leave it: test_needs_the_other_machine"), "{text}");
    assert!(text.ends_with("verdict: ok — builds, and your change breaks no test (1 failed before it too: test_needs_the_other_machine)\n"), "{text}");
    assert!(!text.contains("AssertionError") && !text.contains("FAILED"), "{text}");
    // the change is where it was, nothing is left behind, and what was learned is kept
    assert_eq!(std::fs::read_to_string(dir.join("calc.py")).unwrap(), harmless);
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let known = std::fs::read_to_string(dir.join(".gitnext/check-before.tsv")).unwrap();
    assert!(known.lines().any(|l| l.ends_with("\ttest_needs_the_other_machine")), "{known}");

    // a change that does break a test: that one is the change's, the other still is not
    let broken = CALC.replace("a + b", "a - b");
    std::fs::write(dir.join("calc.py"), &broken).unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(120));
    let text = check::render(&g, &c);
    assert!(!c.ok() && text.contains("tests FAILED") && text.contains("test_add"), "{text}");
    assert!(text.contains("of these, failing without your change too (not caused by it): test_needs_the_other_machine"), "{text}");
    assert!(text.contains("verdict: FAILED — tests"), "{text}");
    assert_eq!(std::fs::read_to_string(dir.join("calc.py")).unwrap(), broken);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_test_the_change_broke_is_said_to_pass_without_it() {
    if !has_pytest() {
        eprintln!("skipped: no pytest in PATH");
        return;
    }
    let only_add = TESTS.split("\n\ndef test_needs").next().unwrap().to_string() + "\n";
    let dir = project("mine", &[("calc.py", CALC), ("test_calc.py", &only_add), (".gitignore", ".gitnext/\n__pycache__/\n.pytest_cache/\n")]);
    let broken = CALC.replace("a + b", "a - b");
    std::fs::write(dir.join("calc.py"), &broken).unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(120));
    let text = check::render(&g, &c);
    assert!(!c.ok() && text.contains("tests FAILED") && text.contains("without your change this test passes — the change causes this"), "{text}");
    assert_eq!(std::fs::read_to_string(dir.join("calc.py")).unwrap(), broken);
    assert!(leftovers(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A check killed while the committed files were in place: the next one puts the changes back.
#[test]
fn changes_left_by_an_interrupted_check_come_back() {
    let dir = project("recover", &[("calc.py", CALC), (".gitignore", ".gitnext/\n__pycache__/\n")]);
    let mine = CALC.replace("a + b", "a + b + 0");
    let _ = graph(&dir); // makes .gitnext
    let left = dir.join(".gitnext/check-before-4000000");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("0"), &mine).unwrap();
    std::fs::write(left.join("files.tsv"), "0\t1\tcalc.py\n").unwrap();
    let g = graph(&dir);
    let c = check::check(&dir, &g, Duration::from_secs(60));
    assert_eq!(std::fs::read_to_string(dir.join("calc.py")).unwrap(), mine);
    assert!(check::render(&g, &c).contains("note: an earlier check was interrupted while it ran the tests on the last commit; your changes are back in calc.py"), "{}", check::render(&g, &c));
    assert!(leftovers(&dir).is_empty());
    // a file written since is left alone
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("0"), "old\n").unwrap();
    std::fs::write(left.join("files.tsv"), "0\t1\tcalc.py\n").unwrap();
    let c = check::check(&dir, &g, Duration::from_secs(60));
    assert_eq!(std::fs::read_to_string(dir.join("calc.py")).unwrap(), mine);
    assert!(c.notes.is_empty() && leftovers(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
