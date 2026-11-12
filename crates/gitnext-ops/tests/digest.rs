//! The digest of real build / test output: every tool's failure becomes name, place, message.
//! The fixtures in `tests/digest/` are outputs of the eval repos with one seeded bug (cargo, pytest,
//! go, vitest, tsc) and the usual shapes of maven, jest, gcc and a Python traceback.

use gitnext_ops::digest::{digest, render, Digest};

fn of(name: &str) -> Digest {
    digest(&std::fs::read_to_string(format!("{}/tests/digest/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap())
}

fn first(d: &Digest) -> (&str, Option<&str>, Option<u32>, String) {
    let f = &d.failures[0];
    (f.what.as_str(), f.file.as_deref(), f.line, f.message.join(" | "))
}

#[test]
fn cargo_test_panic_without_the_backtrace() {
    let d = of("cargo-test.txt");
    assert_eq!(d.kind, "tests");
    assert_eq!(first(&d), ("test_greater_than", Some("tests/test_version_req.rs"), Some(82), "did not match 2.0.0".into()));
    assert_eq!(d.summary.as_deref(), Some("FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.11s"));
    let text = render(&d, 5);
    assert!(!text.contains("rust_begin_unwind") && text.lines().count() == 3, "{text}");
}

#[test]
fn rustc_error_with_its_place() {
    let d = of("cargo-build.txt");
    assert_eq!(d.kind, "build");
    assert_eq!(d.failures.len(), 1, "{:?}", d.failures);
    // the message, the line of code, and what the compiler wrote under it
    assert_eq!(first(&d), ("E0609", Some("src/eval.rs"), Some(63), "no field `majr` on type `&Comparator` | > if ver.major != cmp.majr { | unknown field".into()));
}

#[test]
fn pytest_assertion_with_the_failing_line() {
    let d = of("pytest.txt");
    assert_eq!(d.kind, "tests");
    let (what, file, line, msg) = first(&d);
    assert_eq!((what, file, line), ("test_filename_formatting", Some("tests/test_utils/test_format_filename.py"), Some(6)));
    assert!(msg.starts_with("> assert click.format_filename(b\"/x/foo.txt\") == \"/x/foo.txt\" | AssertionError: assert 'foo.txt' == '/x/foo.txt'"), "{msg}");
    assert_eq!(d.summary.as_deref(), Some("1 failed, 68 passed in 0.16s"));
}

#[test]
fn pytest_cases_of_one_parametrized_test_are_one_failure() {
    let d = of("pytest-param.txt");
    assert_eq!(d.failures.len(), 1, "{:?}", d.failures);
    let f = &d.failures[0];
    assert_eq!((f.what.as_str(), f.file.as_deref(), f.line, f.count), ("test_param_sum", Some("tests/test_zz_param.py"), Some(6), 2));
    assert_eq!(f.message[..2], ["> assert a == b, message", "AssertionError: Error: Invalid value for '-a': Value must be an iterable."]);
    let text = render(&d, 5);
    assert!(text.starts_with("tests: 2 failed — 2 failed, 1 passed in") && text.contains("✗ test_param_sum  tests/test_zz_param.py:6  (2 cases)"), "{text}");
}

#[test]
fn pytest_that_cannot_import_the_project() {
    let d = of("pytest-syntax.txt");
    let (what, file, line, msg) = first(&d);
    assert!(what.starts_with("ImportError while loading conftest"), "{what}");
    assert!(file.unwrap().ends_with("src/click/utils.py") && line == Some(457), "{file:?} {line:?}");
    assert!(msg.ends_with("SyntaxError: expected ':'"), "{msg}");
}

#[test]
fn go_panic_points_at_the_first_frame_of_the_project() {
    let d = of("go-test.txt");
    let (what, file, line, msg) = first(&d);
    assert_eq!(what, "TestActiveHelpAlone");
    assert!(file.unwrap().ends_with("cobra/command.go") && line == Some(1678), "{file:?} {line:?}");
    assert_eq!(msg, "runtime error: invalid memory address or nil pointer dereference");
    assert_eq!(d.summary.as_deref(), Some("FAIL github.com/spf13/cobra"));
    assert!(render(&d, 5).lines().count() <= 4);
}

#[test]
fn go_assertions_and_go_build() {
    let d = of("go-assert.txt");
    assert_eq!(first(&d), ("TestCommandAlias", Some("command_test.go"), Some(120), "Unexpected error: unknown command \"say\" for \"print\" | expected \"echo\", got \"print\"".into()));
    let d = of("go-build.txt");
    assert_eq!(d.kind, "build");
    assert_eq!(first(&d), ("error", Some("./command.go"), Some(1678), "c.parnt undefined (type *Command has no field or method parnt)".into()));
}

#[test]
fn vitest_843_lines_become_a_dozen() {
    let d = of("vitest.txt");
    assert_eq!(d.lines, 843);
    assert_eq!(d.failures.len(), 35);
    let (what, file, line, msg) = first(&d);
    assert_eq!((what, file, line), ("OMap > set", Some("tests/collection-access.ts"), Some(254)));
    assert!(msg.starts_with("AssertionError: expected '!!omap []\\n' to be"), "{msg}");
    assert_eq!(d.summary.as_deref(), Some("35 failed | 240 passed (275)"));
    let text = render(&d, 5);
    assert!(text.lines().count() <= 12 && text.contains("… and 30 more:"), "{text}");
    assert!(text.len() < 1500, "{}", text.len());
}

#[test]
fn tsc_maven_jest_gcc_traceback() {
    let d = of("tsc.txt");
    assert_eq!(first(&d), ("TS2551", Some("src/nodes/YAMLSeq.ts"), Some(328), "Property 'lengthh' does not exist on type 'string[]'. Did you mean 'length'?".into()));

    let d = of("maven-test.txt");
    assert_eq!(d.kind, "tests");
    assert_eq!(d.failures.len(), 1, "{:?}", d.failures);
    assert_eq!(first(&d), ("TextNodeTest.testSplitText", Some("TextNodeTest.java"), Some(88), "org.opentest4j.AssertionFailedError: expected: <Hello > but was: <Hello>".into()));
    assert_eq!(d.summary.as_deref(), Some("Tests run: 1543, Failures: 1, Errors: 0, Skipped: 12"));

    let d = of("maven-build.txt");
    assert_eq!(d.kind, "build");
    // the same error in two files, each printed twice by maven: one line with a count
    assert_eq!(d.failures.len(), 1, "{:?}", d.failures);
    assert!(d.failures[0].count >= 2 && d.failures[0].file.as_deref().unwrap().ends_with("nodes/Element.java") && d.failures[0].line == Some(1642));
    assert_eq!(d.failures[0].message[0], "cannot find symbol");

    let d = of("jest.txt");
    assert_eq!(first(&d), ("vanilla store › can set the store", Some("tests/vanilla/basic.test.ts"), Some(42), "expect(received).toEqual(expected) // deep equality | Expected: 2".into()));
    assert_eq!(d.summary.as_deref(), Some("1 failed, 62 passed, 63 total"));

    let d = of("gcc.txt");
    assert_eq!(d.failures.len(), 1, "{:?}", d.failures);
    assert_eq!(first(&d), ("error", Some("src/parse.c"), Some(42), "'cursor' undeclared (first use in this function)".into()));

    let d = of("traceback.txt");
    assert_eq!(first(&d), ("KeyError", Some("/home/user/proj/tools/config.py"), Some(17), "'build'".into()));
}

#[test]
fn unknown_output_keeps_its_last_lines() {
    let d = digest("starting\nstep 1 done\nsomething odd happened here\n");
    assert!(d.failures.is_empty());
    let text = render(&d, 5);
    assert!(text.contains("not recognised") && text.contains("something odd happened here"), "{text}");
    assert_eq!(render(&digest(""), 5), "no output\n");
}
