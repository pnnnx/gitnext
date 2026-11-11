//! Regressions from a real agent session on loca (Python):
//! - `rename` missed `__all__`, function-level imports in tests and `module.func` access;
//! - `impact` followed every `x.run()` with an unknown receiver.

use gitnext_ops::edit;
use gitnext_ops::gitnext_index::Index;
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn graph(dir: &Path) -> gitnext_ops::gitnext_index::Graph {
    let mut idx = Index::open(dir).unwrap();
    idx.refresh().unwrap();
    idx.graph().unwrap()
}

#[test]
fn rename_reaches_tests_all_and_module_access() {
    let dir = project(
        "rename",
        &[
            ("pkg/__init__.py", ""),
            ("pkg/tools/__init__.py", "from .runner import detect\n\n__all__ = [\"other\",\n           \"detect\"]\n"),
            ("pkg/tools/runner.py", "def detect(root):\n    return root\n\n\ndef build(ctx):\n    return detect(ctx)\n"),
            ("pkg/cli.py", "from .tools.runner import detect\n\n\ndef main():\n    print(detect('.'))\n"),
            (
                "tests/test_runner.py",
                "import pkg.tools.runner as tt\n\n\ndef test_detect():\n    from pkg.tools.runner import detect\n    assert detect(1) == 1\n\n\ndef test_patch():\n    real = tt.detect\n    tt.detect = lambda r: 2\n    tt.detect = real\n",
            ),
        ],
    );
    let g = graph(&dir);
    let i = g.find("detect")[0];
    let r = edit::rename(&dir, &g, i, "find", false).unwrap();
    let left: Vec<String> = ["pkg/tools/__init__.py", "pkg/tools/runner.py", "pkg/cli.py", "tests/test_runner.py"]
        .iter()
        .flat_map(|f| std::fs::read_to_string(dir.join(f)).unwrap().lines().filter(|l| l.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "detect")).map(|l| format!("{f}: {l}")).collect::<Vec<_>>())
        .collect();
    assert!(left.is_empty(), "old name left:\n{}\nreport: {r:?}", left.join("\n"));
    assert!(std::fs::read_to_string(dir.join("pkg/tools/__init__.py")).unwrap().contains("\"find\"]"));
    assert!(r.leftover.is_empty(), "{:?}", r.leftover);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn impact_does_not_follow_unknown_receivers() {
    let dir = project(
        "impact",
        &[
            ("app/__init__.py", ""),
            (
                "app/agent.py",
                "class Agent:\n    def execute(self, call):\n        return call\n\n    def run(self, calls):\n        return [self.execute(c) for c in calls]\n",
            ),
            ("app/tool.py", "class Tool:\n    def run(self, args):\n        return args\n"),
            ("app/cli.py", "def ask(agent):\n    return agent.run([])\n\n\ndef shell(proc):\n    return proc.run()\n"),
        ],
    );
    let g = graph(&dir);
    let i = g.find("Agent::execute")[0];
    let nodes = g.impact(i, 3);
    let sure: Vec<&str> = nodes.iter().filter(|n| n.certain).map(|n| n.symbol.short.as_str()).collect();
    assert!(sure.iter().any(|s| s.ends_with("Agent::run")), "{sure:?}");
    // `agent.run([])` / `proc.run()` could be Agent.run or Tool.run: shown as maybe, never followed
    assert!(!sure.iter().any(|s| *s == "ask" || *s == "shell"), "{sure:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_text_also_fixes_comments_and_strings_but_not_longer_words() {
    let dir = project(
        "rename-text",
        &[
            ("app/__init__.py", ""),
            ("app/core.py", "# detect is used by run; detector is another thing\ndef detect(x):\n    return x\n\n\ndef run():\n    return detect(1), 'detect', 'detector'\n"),
        ],
    );
    let g = graph(&dir);
    let i = g.find("detect")[0];
    let mut r = edit::rename(&dir, &g, i, "find", false).unwrap();
    assert!(!r.leftover.is_empty());
    edit::rename_leftover_text(&dir, &mut r).unwrap();
    let text = std::fs::read_to_string(dir.join("app/core.py")).unwrap();
    assert_eq!(text, "# find is used by run; detector is another thing\ndef find(x):\n    return x\n\n\ndef run():\n    return find(1), 'find', 'detector'\n");
    assert!(r.leftover.is_empty(), "{:?}", r.leftover);
    std::fs::remove_dir_all(&dir).ok();
}
