//! `gitnext look`: a place in a file, written the way tracebacks, test runners and models write it.

use gitnext_ops::gitnext_index::Index;
use gitnext_ops::look::look;
use gitnext_ops::Workspace;

const EVAL: &str = "pub fn matches(a: u32, b: u32) -> bool {\n    matches_greater(a, b) || a == b\n}\n\nfn matches_greater(a: u32, b: u32) -> bool {\n    if a == b {\n        return false;\n    }\n    a > b\n}\n\npub const LIMIT: u32 = 9;\n";
const DISPLAY: &str = "pub fn pad(s: &str) -> String {\n    format!(\"{s:>8}\")\n}\n\npub fn show(n: u32) -> String {\n    pad(&n.to_string())\n}\n";
const TESTS: &str = "#[test]\nfn test_greater_than() {\n    assert!(demo::eval::matches(2, 1));\n}\n\n#[test]\nfn test_equal() {\n    assert!(demo::eval::matches(1, 1));\n}\n";

fn project(name: &str) -> (Workspace, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gitnext-look-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in [
        ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs", "pub mod display;\npub mod eval;\n"),
        ("src/eval.rs", EVAL),
        ("src/display.rs", DISPLAY),
        ("tests/test_req.rs", TESTS),
    ] {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    (Workspace::open(&dir).unwrap(), dir)
}

#[test]
fn a_name_in_a_file_is_found_however_it_is_written() {
    let (ws, dir) = project("name");
    let mut idx = Index::open(&ws.root).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let by_name = look(&ws, &g, "matches_greater", false);
    assert!(by_name.starts_with("src/eval.rs:5-10 matches_greater"), "{by_name}");
    for what in [
        "src/eval.rs::matches_greater",
        "eval.rs::matches_greater",
        "src/eval.rs:matches_greater",
        "src/eval.rs matches_greater",
        "matches_greater src/eval.rs",
        "matches_greater() eval.rs",
        "src/eval.rs:7",
        "src/eval.rs:L7",
        "./src/eval.rs:5",
    ] {
        assert_eq!(look(&ws, &g, what, false), by_name, "`{what}`");
    }
    // a test id of a runner: the file, the test, its case
    let t = look(&ws, &g, "tests/test_req.rs::test_greater_than[case-1]", false);
    assert!(t.starts_with("tests/test_req.rs:1-4 test_greater_than"), "{t}");
    // not in that file: said so, with what the file has
    let miss = look(&ws, &g, "src/display.rs::matches_greater", false);
    assert!(miss.starts_with("no `matches_greater` in src/display.rs — what is there:") && miss.contains("pad") && miss.contains("show"), "{miss}");
    // a file followed by a shell pipe is the file
    let piped = look(&ws, &g, "src/display.rs | grep -n pad", false);
    assert_eq!(piped, look(&ws, &g, "src/display.rs", false));
    // names with `::` that are not files are names as before
    let q = look(&ws, &g, "eval::matches_greater", false);
    assert_eq!(q, by_name);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lines_of_a_file() {
    let (ws, dir) = project("lines");
    let mut idx = Index::open(&ws.root).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let r = look(&ws, &g, "src/eval.rs:5-7", false);
    assert!(r.starts_with("src/eval.rs:5-7 (of 12 lines)\n```rust\nfn matches_greater(a: u32, b: u32) -> bool {\n    if a == b {\n        return false;\n```\n"), "{r}");
    assert!(r.contains("these lines are in: matches_greater (5-10)"), "{r}");
    // the same written with a space, and past the end of the file
    assert_eq!(look(&ws, &g, "eval.rs 5-7", false), r);
    let tail = look(&ws, &g, "src/eval.rs:9-400", false);
    assert!(tail.starts_with("src/eval.rs:9-12 (of 12 lines)") && tail.contains("LIMIT"), "{tail}");
    assert_eq!(look(&ws, &g, "src/eval.rs:300-400", false), "src/eval.rs has 12 lines\n");
    // two functions in the range: both named; the small-model wording says how to change one
    let both = look(&ws, &g, "src/eval.rs:1-10", true);
    assert!(both.contains("these lines are in: matches (1-3), matches_greater (5-10)") && both.contains("replace(symbol="), "{both}");
    // a line outside any function: the lines around it
    let around = look(&ws, &g, "src/eval.rs:12", false);
    assert!(around.starts_with("src/eval.rs:2-12 (of 12 lines)") && around.contains("LIMIT"), "{around}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn words_that_are_names() {
    let (ws, dir) = project("words");
    let mut idx = Index::open(&ws.root).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    // no line has all of the words; two of them are functions
    let r = look(&ws, &g, "pad formatter width align show", false);
    assert!(r.starts_with("nothing has all these words; the ones that are names"), "{r}");
    assert!(r.contains("pad (function) src/display.rs:1") && r.contains("show (function) src/display.rs:5"), "{r}");
    let none = look(&ws, &g, "zzzz qqqq", false);
    assert!(none.starts_with("nothing found for `zzzz qqqq`") && none.contains("src/app.py:120-160"), "{none}");
    let _ = std::fs::remove_dir_all(&dir);
}
