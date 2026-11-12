//! `gitnext learn-tests` on a small Rust project: the tests are built with coverage and run one by
//! one, and the map knows which test ran which function. Needs rustup's `llvm-tools-preview`; without
//! it (as in CI) the test passes without doing anything.

use gitnext_ops::gitnext_index::{Index, TestMap};
use std::path::{Path, PathBuf};
use std::process::Command;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-learn-rust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn llvm_tools() -> bool {
    let Ok(o) = Command::new("rustc").args(["--print", "sysroot"]).output() else { return false };
    let sysroot = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
    let Ok(dirs) = std::fs::read_dir(sysroot.join("lib/rustlib")) else { return false };
    dirs.flatten().any(|d| d.path().join("bin/llvm-cov").is_file() && d.path().join("bin/llvm-profdata").is_file())
}

const LIB: &str = "pub mod display;\n\npub fn version() -> String {\n    display::pad(\"1.0\", 5)\n}\n";
const DISPLAY: &str = "pub fn pad(s: &str, width: usize) -> String {\n    let mut out = s.to_string();\n    while out.len() < width {\n        out.push(' ');\n    }\n    out\n}\n\npub fn unused() -> u32 {\n    let x = 7;\n    x + 1\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_align() {\n        assert_eq!(pad(\"a\", 3), \"a  \");\n    }\n\n    #[test]\n    #[ignore]\n    fn test_slow() {\n        assert_eq!(unused(), 8);\n    }\n}\n";
const IT: &str = "#[test]\nfn test_version() {\n    assert_eq!(demo::version(), \"1.0  \");\n}\n\nmacro_rules! made {\n    ($n:ident) => {\n        #[test]\n        fn $n() {\n            assert_eq!(demo::version().len(), 5);\n        }\n    };\n}\nmade!(test_made);\n";

#[test]
fn each_test_is_mapped_to_the_functions_it_ran() {
    if !llvm_tools() {
        return;
    }
    let dir = project(&[
        ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n"),
        ("src/lib.rs", LIB),
        ("src/display.rs", DISPLAY),
        ("tests/it.rs", IT),
    ]);
    // its own target, so this test does not share one with anything else
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let learned = gitnext_ops::learn::learn(&dir, &g).unwrap();
    let map = TestMap::open_existing(&dir).unwrap();
    let ran = |file: &str, name: &str| map.ran_by(file, name).into_iter().map(|r| r.0).collect::<Vec<_>>();

    assert_eq!(ran("src/display.rs", "test_align"), ["demo::display::pad"]);
    assert_eq!(ran("tests/it.rs", "test_version"), ["demo::display::pad", "demo::version"]);
    // the ignored test is not run, the one made by a macro has no symbol to be found by
    assert!(ran("src/display.rs", "test_slow").is_empty() && ran("tests/it.rs", "test_made").is_empty());
    assert_eq!((learned.tests, learned.functions, learned.pairs, learned.never_run), (2, 2, 3, 1));
    assert!(!learned.tests_failed);
    let notes = learned.notes.join("\n");
    assert!(notes.contains("gitnext-cov") && notes.contains(" MB)"), "{notes}");
    assert!(notes.contains("1 of 3 tests are not functions the graph knows") && notes.contains("test_made"), "{notes}");
    assert!(notes.contains("1 #[ignore] tests were not run"), "{notes}");
    // the profiles are gone, the build stays
    let cov = PathBuf::from(notes.split("kept in ").nth(1).and_then(|r| r.split(" (").next()).unwrap());
    assert!(cov.ends_with("gitnext-cov") && cov.is_dir(), "{}", cov.display());
    assert!(!std::fs::read_dir(&cov).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("profiles-")));
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}
