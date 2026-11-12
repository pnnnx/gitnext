//! `gitnext learn-tests` on a small Go module: the map names the tests that really ran each
//! function. Skipped when `go` is not installed (the profile parsing is tested without it in `learn/go.rs`).

use gitnext_ops::gitnext_index::{Graph, Index, TestMap};
use std::path::{Path, PathBuf};
use std::process::Command;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-learn-go-{}", std::process::id()));
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

const CALC: &str = "package calc\n\nvar base int\n\nfunc Add(a, b int) int { return a + b }\n\nfunc Double(x int) int {\n\treturn Add(x, x)\n}\n\nfunc SetBase(b int) { base = b }\n\nfunc Base() int {\n\tif base == 0 {\n\t\treturn -1\n\t}\n\treturn base\n}\n\nfunc Unused() int {\n\treturn 7\n}\n";
const CALC_TEST: &str = "package calc\n\nimport \"testing\"\n\nfunc TestDouble(t *testing.T) {\n\tif Double(2) != 4 {\n\t\tt.Fatal(\"bad double\")\n\t}\n}\n\nfunc TestSetBase(t *testing.T) {\n\tSetBase(3)\n}\n\n// passes only after TestSetBase\nfunc TestBaseAfterSet(t *testing.T) {\n\tif Base() != 3 {\n\t\tt.Fatal(\"base not set\")\n\t}\n}\n";
const EXAMPLE_TEST: &str = "package calc_test\n\nimport (\n\t\"fmt\"\n\n\t\"example.com/demo/calc\"\n)\n\nfunc ExampleDouble() {\n\tfmt.Println(calc.Double(3))\n\t// Output: 6\n}\n";
const GREET: &str = "package greet\n\nfunc Hello(name string) string {\n\treturn \"hello \" + name\n}\n";
const GREET_TEST: &str = "package greet\n\nimport (\n\t\"os\"\n\t\"testing\"\n)\n\nfunc TestMain(m *testing.M) {\n\tos.Exit(m.Run())\n}\n\nfunc TestHello(t *testing.T) {\n\tif Hello(\"go\") != \"hello go\" {\n\t\tt.Fatal(\"bad greeting\")\n\t}\n}\n";

fn tests_of(dir: &Path, g: &Graph, name: &str) -> Vec<(String, String)> {
    let q = &g.syms[g.find(name)[0]].sym.qualified;
    let mut t: Vec<(String, String)> = TestMap::open_existing(dir).unwrap().tests_of(q).into_iter().map(|c| (c.file, c.name)).collect();
    t.sort();
    t
}

#[test]
fn a_go_module_learns_which_test_ran_which_function() {
    if Command::new("go").arg("version").output().map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("skipped: no `go` in PATH");
        return;
    }
    let dir = project(&[
        ("go.mod", "module example.com/demo\n\ngo 1.21\n"),
        ("calc/calc.go", CALC),
        ("calc/calc_test.go", CALC_TEST),
        ("calc/example_test.go", EXAMPLE_TEST),
        ("greet/greet.go", GREET),
        ("greet/greet_test.go", GREET_TEST),
    ]);
    let g = graph(&dir);
    let learned = gitnext_ops::learn::learn(&dir, &g).unwrap();
    let pair = |f: &str, n: &str| (f.to_string(), n.to_string());

    // a one-line function and a function without branches: their only block starts on the signature line
    assert_eq!(tests_of(&dir, &g, "Add"), vec![pair("calc/calc_test.go", "TestDouble"), pair("calc/example_test.go", "ExampleDouble")]);
    assert_eq!(tests_of(&dir, &g, "SetBase"), vec![pair("calc/calc_test.go", "TestSetBase")]);
    assert_eq!(tests_of(&dir, &g, "Base"), vec![pair("calc/calc_test.go", "TestBaseAfterSet")]);
    assert_eq!(tests_of(&dir, &g, "Hello"), vec![pair("greet/greet_test.go", "TestHello")]);
    assert!(tests_of(&dir, &g, "Unused").is_empty());

    let notes = learned.notes.join("\n");
    assert!(notes.contains("TestMain") && notes.contains("greet"), "{notes}");
    assert!(notes.contains("fail when run alone") && notes.contains("calc: TestBaseAfterSet"), "{notes}");
    assert!(learned.tests_failed);
    assert_eq!((learned.tests, learned.functions), (5, 5), "{notes}");
    let left: Vec<_> = std::fs::read_dir(dir.join(".gitnext")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("learn-go-")).collect();
    assert!(left.is_empty(), "temporary binaries are left: {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
