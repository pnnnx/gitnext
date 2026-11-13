//! `gitnext learn-tests` in a JavaScript / TypeScript project on vitest: the map names the test
//! files that really ran each function. What is missing is said without running node; the run
//! itself installs vitest from npm and is skipped unless `GITNEXT_TEST_NPM=1` (the parsing of a
//! saved coverage report is tested without node in `learn/js.rs`).

use gitnext_ops::gitnext_index::{Graph, Index, TestMap};
use gitnext_ops::{learn, locate};
use std::path::{Path, PathBuf};
use std::process::Command;

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-learn-js-{name}-{}", std::process::id()));
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

const PACKAGE: &str = "{ \"name\": \"demo\", \"private\": true, \"type\": \"module\", \"scripts\": { \"test\": \"vitest run\" },\n  \"devDependencies\": { \"vitest\": \"4.1.11\", \"@vitest/coverage-v8\": \"4.1.11\" } }\n";
const CALC: &str = "export function add(a: number, b: number): number {\n  return a + b\n}\n\nexport function double(x: number): number {\n  return add(x, x)\n}\n\nexport function unused(): number {\n  return 7\n}\n\nconst table = { twice: double }\n\nexport function byName(name: \"twice\", x: number): number {\n  return table[name](x)\n}\n";
const CALC_TEST: &str = "import { expect, it } from 'vitest'\nimport { double } from '../src/calc'\n\nit('doubles', () => {\n  expect(double(2)).toBe(4)\n})\n";
const BY_NAME_TEST: &str = "import { expect, it } from 'vitest'\nimport { byName } from '../src/calc'\n\nit('looks up', () => {\n  expect(byName('twice', 3)).toBe(6)\n})\n";

fn learn_error(dir: &Path) -> String {
    let g = graph(dir);
    let e = learn::learn(dir, &g).err().map(|e| format!("{e:#}")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(dir);
    e
}

#[test]
fn a_project_on_jest_is_told_so_in_one_line() {
    let dir = project("jest", &[("package.json", "{ \"devDependencies\": { \"jest\": \"29.7.0\" } }\n"), ("src/calc.ts", CALC), ("tests/calc.test.ts", CALC_TEST)]);
    let e = learn_error(&dir);
    assert_eq!(e, "JavaScript/TypeScript: the tests run with jest; learn-tests knows vitest only");
}

#[test]
fn what_is_not_installed_is_named_with_the_command_that_installs_it() {
    let files = [("package.json", PACKAGE), ("src/calc.ts", CALC), ("tests/calc.test.ts", CALC_TEST)];
    let e = learn_error(&project("no-modules", &files));
    assert!(e.contains("vitest is not installed") && e.contains("npm install"), "{e}");
    // vitest is there, its v8 coverage is not: the version to install is vitest's own
    let dir = project("no-coverage", &files);
    std::fs::create_dir_all(dir.join("node_modules/vitest")).unwrap();
    std::fs::write(dir.join("node_modules/vitest/package.json"), "{ \"name\": \"vitest\", \"version\": \"4.1.11\" }\n").unwrap();
    let e = learn_error(&dir);
    assert!(e.ends_with("vitest's v8 coverage is not installed: npm i -D @vitest/coverage-v8@4.1.11"), "{e}");
    let e = learn_error(&project("no-package", &[("src/calc.ts", CALC)]));
    assert!(e.contains("no package.json at the root"), "{e}");
}

#[test]
fn a_vitest_project_learns_which_test_file_ran_which_function() {
    if std::env::var("GITNEXT_TEST_NPM").as_deref() != Ok("1") {
        eprintln!("skipped: set GITNEXT_TEST_NPM=1 to install vitest from npm and run it");
        return;
    }
    let dir = project("vitest", &[("package.json", PACKAGE), ("src/calc.ts", CALC), ("tests/calc.test.ts", CALC_TEST), ("tests/by-name.test.ts", BY_NAME_TEST)]);
    // npm 10 stops on the peer dependencies of vitest 4 without --legacy-peer-deps
    let installed = Command::new("npm").args(["install", "--no-audit", "--no-fund", "--loglevel=error", "--legacy-peer-deps"]).current_dir(&dir).status();
    assert!(installed.is_ok_and(|s| s.success()), "npm install failed in {}", dir.display());
    let g = graph(&dir);
    let l = learn::learn(&dir, &g).unwrap();
    assert!(!l.tests_failed && l.tests == 2, "{}", learn::render(&l));
    assert!(l.command.contains("2 files"), "{}", l.command);
    let files_of = |name: &str| {
        let q = g.unique_name(g.find(name)[0]);
        let mut t: Vec<String> = TestMap::open_existing(&dir).unwrap().tests_of(&q).into_iter().map(|c| c.file).collect();
        t.sort();
        t
    };
    assert_eq!(files_of("double"), ["tests/by-name.test.ts", "tests/calc.test.ts"]);
    // reached through an object: the call graph does not see it, the coverage run does
    assert_eq!(files_of("add"), ["tests/by-name.test.ts", "tests/calc.test.ts"]);
    assert_eq!(files_of("byName"), ["tests/by-name.test.ts"]);
    assert!(files_of("unused").is_empty());
    let (t, source) = locate::tests_for_at(&g, &dir, g.find("byName")[0], 4);
    assert_eq!(source, locate::Source::Learned { stale: false });
    assert_eq!(t.iter().map(|x| g.syms[x.symbol].file.as_str()).collect::<Vec<_>>(), ["tests/by-name.test.ts"]);
    // the temporary reports are gone
    let left: Vec<_> = std::fs::read_dir(dir.join(".gitnext")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("learn-js")).collect();
    assert!(left.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
