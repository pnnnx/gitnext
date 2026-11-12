//! `gitnext why`: from a failing test to the code to look at. The test run itself is the project's
//! own command (cargo, pytest, go…); here its output is given, and what is checked is what is made of it.

use gitnext_ops::check::Step;
use gitnext_ops::gitnext_index::{Graph, Index};
use gitnext_ops::{digest, locate, why};
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-why-{name}-{}", std::process::id()));
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

fn failed(g: &Graph, dir: &Path, output: &str) -> Step {
    Step { what: "tests", command: "the test command".into(), ok: false, timed_out: false, seconds: 0.2, output: output.into(), digest: digest::digest(output), frames: locate::frames(g, dir, output) }
}

const EVAL: &str = "pub fn matches(req: u32, v: u32) -> bool {\n    matches_exact(req, v) || matches_greater(req, v)\n}\n\nfn matches_exact(req: u32, v: u32) -> bool {\n    req == v\n}\n\nfn matches_greater(req: u32, v: u32) -> bool {\n    v < req\n}\n\npub fn unrelated() -> u32 {\n    7\n}\n";
const TESTS: &str = "use demo::matches;\n\nfn assert_all(req: u32, vs: &[u32]) {\n    for v in vs {\n        assert!(matches(req, *v), \"did not match {v}\");\n    }\n}\n\n#[test]\nfn test_greater_than() {\n    assert_all(1, &[2, 3]);\n}\n\n#[test]\nfn test_exact() {\n    assert_all(1, &[1]);\n}\n";

fn rust_project(name: &str) -> PathBuf {
    project(
        name,
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/lib.rs", "mod eval;\npub use eval::matches;\n"),
            ("src/eval.rs", EVAL),
            ("tests/test_req.rs", TESTS),
        ],
    )
}

#[test]
fn the_test_is_found_by_the_words_of_its_name() {
    let dir = rust_project("find");
    let g = graph(&dir);
    let name = |i: &usize| g.syms[*i].sym.name.clone();
    assert_eq!(why::find_tests(&g, "test_greater_than").iter().map(name).collect::<Vec<_>>(), ["test_greater_than"]);
    // what a small model asks for when told that `test_greater_than` fails
    assert_eq!(why::find_tests(&g, "greater_than").iter().map(name).collect::<Vec<_>>(), ["test_greater_than"]);
    assert_eq!(why::find_tests(&g, "GreaterThan").iter().map(name).collect::<Vec<_>>(), ["test_greater_than"]);
    // a helper of the tests is not a test, and code is not a test
    assert!(why::find_tests(&g, "assert_all").is_empty() && why::find_tests(&g, "matches_greater").is_empty());
    assert_eq!(why::similar_tests(&g, "greater").iter().map(name).collect::<Vec<_>>(), ["test_greater_than"]);
}

#[test]
fn the_function_named_like_the_test_comes_first_with_its_code() {
    let dir = rust_project("rank");
    let g = graph(&dir);
    let test = why::find_tests(&g, "greater_than");
    let out = "running 1 test\ntest test_greater_than ... FAILED\n\nfailures:\n\n---- test_greater_than stdout ----\n\nthread 'test_greater_than' (7) panicked at tests/test_req.rs:5:9:\ndid not match 2\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    test_greater_than\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n";
    let w = why::explain(&dir, &g, &test, vec![failed(&g, &dir, out)]);
    assert!(w.failed());
    let names: Vec<&str> = w.suspects.iter().map(|s| g.syms[s.symbol].sym.name.as_str()).collect();
    // through the helper `assert_all`, which is test code and is not listed; `unrelated` is not reached
    assert_eq!(names, ["matches_greater", "matches", "matches_exact"]);
    let text = why::render(&g, &dir, &w);
    assert!(text.starts_with("why: test_greater_than — FAILS"), "{text}");
    assert!(text.contains("✗ test_greater_than  tests/test_req.rs:5\n    did not match 2\n"), "{text}");
    // the source line of the failure, and the code of the first function only
    assert!(text.contains("tests/test_req.rs:5: assert!(matches(req, *v), \"did not match {v}\");"), "{text}");
    assert!(text.contains(" 1. matches_greater  src/eval.rs:9") && text.contains("    v < req\n"), "{text}");
    assert!(!text.contains("req == v") && !text.contains("test result:"), "{text}");
    assert!(text.len() < 900, "{}", text.len());
}

#[test]
fn a_function_the_failure_names_comes_before_the_rest() {
    let dir = project(
        "frames",
        &[
            ("go.mod", "module example.com/demo\n\ngo 1.21\n"),
            ("command.go", "package demo\n\ntype Command struct {\n\tparent *Command\n}\n\nfunc (c *Command) HasParent() bool {\n\treturn c.parent == nil\n}\n\nfunc (c *Command) Root() *Command {\n\tif c.HasParent() {\n\t\treturn c.parent.Root()\n\t}\n\treturn c\n}\n\nfunc (c *Command) Name() string {\n\treturn \"x\"\n}\n"),
            ("command_test.go", "package demo\n\nimport \"testing\"\n\nfunc TestRoot(t *testing.T) {\n\tc := &Command{}\n\tif c.Root().Name() != \"x\" {\n\t\tt.Fatal(\"no root\")\n\t}\n}\n"),
        ],
    );
    let g = graph(&dir);
    let test = why::find_tests(&g, "TestRoot");
    let out = format!(
        "--- FAIL: TestRoot (0.00s)\npanic: runtime error: invalid memory address or nil pointer dereference [recovered]\n\tpanic: runtime error: invalid memory address or nil pointer dereference\n\ngoroutine 20 [running]:\ntesting.tRunner.func1.2({{0x6ede00, 0x9bc2d0}})\n\t/usr/local/go/src/testing/testing.go:1734 +0x21c\nexample.com/demo.(*Command).Root(...)\n\t{0}/command.go:13\nexample.com/demo.TestRoot(0xc0001048c0)\n\t{0}/command_test.go:7 +0x99\nFAIL\texample.com/demo\t0.006s\nFAIL\n",
        dir.display()
    );
    let mut step = failed(&g, &dir, &out);
    // `check::run` gives the paths as the project has them
    for f in &mut step.digest.failures {
        f.file = f.file.as_deref().and_then(|p| p.strip_prefix(&format!("{}/", dir.display())).map(str::to_string));
    }
    let w = why::explain(&dir, &g, &test, vec![step]);
    let first = &w.suspects[0];
    assert_eq!((g.syms[first.symbol].sym.name.as_str(), first.in_failure), ("Root", Some(0)));
    let text = why::render(&g, &dir, &w);
    assert!(text.contains("✗ TestRoot  command.go:13\n    runtime error: invalid memory address or nil pointer dereference\n    command.go:13: return c.parent.Root()"), "{text}");
    assert!(text.contains(" 1. Root  command.go:11  (named in the failure)"), "{text}");
    assert!(!text.contains("/usr/local/go") && !text.contains("goroutine"), "{text}");
}

#[test]
fn a_passing_test_needs_no_explanation() {
    let dir = rust_project("pass");
    let g = graph(&dir);
    let test = why::find_tests(&g, "test_exact");
    let ok = Step { what: "tests", command: "cargo test".into(), ok: true, timed_out: false, seconds: 0.1, output: String::new(), digest: Default::default(), frames: vec![] };
    let w = why::explain(&dir, &g, &test, vec![ok]);
    assert!(!w.failed() && w.suspects.is_empty());
    assert!(why::render(&g, &dir, &w).starts_with("why: test_exact — passes"));
}
