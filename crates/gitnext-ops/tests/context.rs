//! `gitnext context`: the block loca's before_prompt hook adds to a request.
//! Tasks are the ones a 4B model was benchmarked on (Russian prompts, as in loca).

use gitnext_ops::{context::context, Workspace};
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-ctx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn ctx(dir: &Path, task: &str) -> String {
    let ws = Workspace::open(dir).unwrap();
    let g = ws.graph().unwrap();
    context(&ws, &g, task, 2000)
}

const AGENT: &str = "import json\n\n\ndef brief(text, limit=40):\n    first = text.split('. ')[0]\n    return first[:limit + 1]\n\n\n# estimate_tokens is calibrated below\ndef estimate_tokens(messages):\n    return len(json.dumps(messages)) // 3\n\n\nclass Agent:\n    def size(self):\n        return estimate_tokens([]) + len(brief('a'))\n";

#[test]
fn failing_test_file_brings_the_code_it_calls() {
    let dir = project(
        "test",
        &[
            ("app/__init__.py", ""),
            ("app/agent.py", AGENT),
            ("tests/test_brief.py", "import unittest\nfrom app.agent import brief\n\n\nclass BriefTest(unittest.TestCase):\n    def test_fits(self):\n        self.assertEqual(len(brief('x' * 100, 40)), 40)\n"),
        ],
    );
    let s = ctx(&dir, "Тест tests/test_brief.py падает. Найди причину в коде и исправь (тест не меняй).");
    assert!(s.contains("def brief(text, limit=40):"), "{s}");
    assert!(s.contains("assertEqual(len(brief('x' * 100, 40)), 40)"), "the test itself shows the expectation:\n{s}");
    assert!(s.contains("tests/test_brief.py::BriefTest::test_fits"), "run command with the class:\n{s}");
    assert!(!s.contains("def estimate_tokens"), "unrelated code stays out:\n{s}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_lists_every_use_and_text_mentions() {
    let dir = project("rename", &[("app/__init__.py", ""), ("app/agent.py", AGENT)]);
    let s = ctx(&dir, "Переименуй функцию estimate_tokens в approx_tokens везде в проекте.");
    assert!(s.contains("def estimate_tokens(messages):"), "{s}");
    assert!(s.contains("used in (1): app/agent.py:16"), "{s}");
    assert!(s.contains("also as text (comments, strings): app/agent.py:9"), "{s}");
    assert!(s.contains("gitnext rename estimate_tokens approx_tokens --text"), "{s}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn nothing_named_means_nothing_added() {
    let dir = project("none", &[("app/__init__.py", ""), ("app/agent.py", AGENT)]);
    assert_eq!(ctx(&dir, "Привет! Как дела?"), "");
    // a class named in the task: its code
    assert!(ctx(&dir, "В каком файле определён класс Agent?").contains("app/agent.py:14-16 Agent"));
    std::fs::remove_dir_all(&dir).ok();
}
