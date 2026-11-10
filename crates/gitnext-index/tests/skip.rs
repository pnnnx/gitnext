//! Build output and caches are not indexed even without a .gitignore.

use gitnext_index::Index;

#[test]
fn build_copies_and_caches_are_skipped() {
    let dir = std::env::temp_dir().join(format!("gitnext-skip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["build/lib", "pkg.egg-info", "__pycache__", ".venv/lib", "src"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    let code = "def main():\n    pass\n";
    std::fs::write(dir.join("app.py"), code).unwrap();
    std::fs::write(dir.join("src/util.py"), "def helper():\n    pass\n").unwrap();
    std::fs::write(dir.join("build/lib/app.py"), code).unwrap();
    std::fs::write(dir.join("pkg.egg-info/PKG-INFO"), "Name: pkg\n").unwrap();
    std::fs::write(dir.join(".venv/lib/x.py"), code).unwrap();
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let mut files: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).collect();
    files.sort();
    assert_eq!(files, vec!["app.py", "src/util.py"]);
    assert_eq!(g.find("main").len(), 1, "one main, not a copy from build/");

    // `.gitnext/config` can keep a folder
    std::fs::write(dir.join(".gitnext/config"), "index_keep = build\n").unwrap();
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    assert!(idx.graph().unwrap().files.iter().any(|f| f.path == "build/lib/app.py"));
    std::fs::remove_dir_all(&dir).ok();
}
