//! `tests-for` names the tests most likely to fail first: direct calls (those named after
//! the function before the others), tests named after it, then the ones that only reach it
//! through other functions — and gives one command for the top and one for whole files.

use gitnext_ops::gitnext_index::Index;
use gitnext_ops::locate;

#[test]
fn most_likely_tests_first() {
    let dir = std::env::temp_dir().join(format!("gitnext-testsfor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("shop.py"),
        "def total_price(items):\n    return sum(items)\n\n\ndef checkout(cart):\n    return total_price(cart)\n\n\ndef run(cart):\n    return checkout(cart)\n",
    )
    .unwrap();
    // 12 tests that only reach it through `run`, in the file that would be listed first by path
    let mut far = String::from("from shop import run\n\n\ndef helper(cart):\n    return run(cart)\n");
    for i in 0..12 {
        far.push_str(&format!("\n\ndef test_flow_{i}():\n    assert helper([{i}]) == {i}\n"));
    }
    std::fs::write(dir.join("tests/test_a_flows.py"), far).unwrap();
    std::fs::write(
        dir.join("tests/test_shop.py"),
        "from shop import checkout, total_price\n\n\ndef test_rounding():\n    assert total_price([1, 2]) == 3\n\n\ndef test_total_price_empty():\n    assert total_price([]) == 0\n\n\ndef test_checkout():\n    assert checkout([1]) == 1\n\n\ndef test_total_price_documented():\n    pass\n",
    )
    .unwrap();
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let target = g.find("total_price")[0];
    let t = locate::tests_for(&g, target, 4);
    let names: Vec<&str> = t.iter().map(|x| g.syms[x.symbol].sym.name.as_str()).collect();
    // direct and named, direct, named only, one hop away, then the far ones; no helpers
    assert_eq!(&names[..4], ["test_total_price_empty", "test_rounding", "test_total_price_documented", "test_checkout"], "{names:?}");
    assert_eq!(names.len(), 16, "{names:?}");
    assert!(!names.contains(&"helper"));
    assert_eq!(t[0].how, "direct, named after it");
    assert_eq!(t[2].how, "named after it");

    let text = locate::render_tests_for(&g, &dir, target, 4, false);
    assert!(text.contains("(16), the 10 most likely to fail first"), "{text}");
    assert_eq!(text.matches("test_flow_").count(), 2 * 6, "6 of the 12 far tests fit the top 10 (listed and in the command):\n{text}");
    assert!(text.contains("pytest tests/test_shop.py::test_total_price_empty"), "{text}");
    assert!(text.contains("  pytest tests/test_a_flows.py tests/test_shop.py") || text.contains("  pytest tests/test_shop.py tests/test_a_flows.py"), "{text}");
    let all = locate::render_tests_for(&g, &dir, target, 4, true);
    assert_eq!(all.matches("test_flow_").count(), 2 * 12, "{all}");
    let _ = std::fs::remove_dir_all(&dir);
}
