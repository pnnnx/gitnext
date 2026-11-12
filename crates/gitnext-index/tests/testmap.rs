//! The learned test map and the coverage.py data it is built from.

use gitnext_index::{read_coverage, TestMap};
use rusqlite::Connection;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gitnext-testmap-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn coverage_lines_and_arcs_are_read_per_test() {
    let d = dir("cov");
    let data = d.join("cov.db");
    let c = Connection::open(&data).unwrap();
    c.execute_batch(
        "CREATE TABLE file(id INTEGER PRIMARY KEY, path TEXT);
         CREATE TABLE context(id INTEGER PRIMARY KEY, context TEXT);
         CREATE TABLE line_bits(file_id INTEGER, context_id INTEGER, numbits BLOB);
         CREATE TABLE arc(file_id INTEGER, context_id INTEGER, fromno INTEGER, tono INTEGER);
         INSERT INTO file VALUES (1, '/repo/src/a.py'), (2, '/repo/src/b.py');
         INSERT INTO context VALUES (1, ''), (2, 'tests/test_a.py::test_one[p1]|run'), (3, 'tests/test_a.py::TestB::test_two|run');
         INSERT INTO line_bits VALUES (1, 1, x'ff'), (1, 2, x'0601');
         INSERT INTO arc VALUES (2, 3, -4, 5), (2, 3, 5, 7), (2, 3, 7, -4), (2, 1, 1, 2);",
    )
    .unwrap();
    drop(c);
    let mut got = read_coverage(&data).unwrap();
    got.sort();
    assert_eq!(
        got,
        [
            ("/repo/src/a.py".to_string(), "tests/test_a.py::test_one[p1]|run".to_string(), vec![1, 2, 8]), // bits 1,2 of byte 0; bit 0 of byte 1
            ("/repo/src/b.py".to_string(), "tests/test_a.py::TestB::test_two|run".to_string(), vec![5, 7]),
        ],
        "code run outside tests (empty context) and entry / exit markers are left out"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn map_answers_narrowest_tests_first() {
    let d = dir("map");
    assert!(TestMap::open_existing(&d).is_none());
    let row = |sym: &str, test: &str| (sym.to_string(), "h1".to_string(), "tests/test_a.py".to_string(), test.to_string());
    // test_wide runs three functions, test_narrow one
    let pairs = vec![row("m::f", "test_wide"), row("m::g", "test_wide"), row("m::h", "test_wide"), row("m::f", "test_narrow"), row("m::f", "test_narrow")];
    let map = TestMap::write(&d, "pytest-cov", &pairs).unwrap();
    assert_eq!(map.size(), (3, 2, 4));
    let map = TestMap::open_existing(&d).unwrap();
    let names: Vec<String> = map.tests_of("m::f").into_iter().map(|c| format!("{} {}", c.name, c.breadth)).collect();
    assert_eq!(names, ["test_narrow 1", "test_wide 3"]);
    assert!(map.tests_of("m::unknown").is_empty());
    // learning again replaces the map
    TestMap::write(&d, "pytest-cov", &[row("m::g", "test_only")]).unwrap();
    assert!(TestMap::open_existing(&d).unwrap().tests_of("m::f").is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
