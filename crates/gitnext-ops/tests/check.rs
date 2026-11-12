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
