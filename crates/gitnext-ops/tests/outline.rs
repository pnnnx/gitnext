//! `gitnext outline`: the skeleton of a file or a folder.

use gitnext_ops::gitnext_index::Index;
use gitnext_ops::render::{self, Opts};

#[test]
fn a_folder_with_nothing_public_is_not_answered_with_silence() {
    let dir = std::env::temp_dir().join(format!("gitnext-outline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in [
        ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs", "pub fn open() -> u32 {\n    helper()\n}\n\nfn helper() -> u32 {\n    7\n}\n"),
        ("tests/it.rs", "fn fixture() -> u32 {\n    1\n}\n\n#[test]\nfn test_open() {\n    assert_eq!(demo::open(), 7 * fixture());\n}\n"),
    ] {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    // a folder with a public function: the function, as before
    let src = render::outline(&g, &dir, "src", Opts::default());
    assert!(src.contains("open") && !src.contains("nothing public"), "{src}");
    // a folder of tests: what is there and how to see it
    let tests = render::outline(&g, &dir, "tests", Opts::default());
    assert!(tests.starts_with("1 file at `tests`, nothing public in them"), "{tests}");
    assert!(tests.contains("tests/it.rs · 2 symbols, 2 of them test code") && tests.contains("--all") && tests.contains("--tests"), "{tests}");
    // and with --all --tests the helper and the test are listed
    let all = render::outline(&g, &dir, "tests", Opts { all: true, tests: true, ..Opts::default() });
    assert!(all.contains("fixture") && !all.contains("nothing public"), "{all}");
    let _ = std::fs::remove_dir_all(&dir);
}
