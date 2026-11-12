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

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

/// A test in another language that happens to carry the same name is not a test of this function.
#[test]
fn names_do_not_cross_languages() {
    let dir = project(
        "crosslang",
        &[
            ("tool.py", "def main():\n    return run()\n\n\ndef run():\n    return 1\n"),
            ("tests/fixtures/zoo.cpp", "int helper() { return 1; }\n\nint main() {\n    return helper();\n}\n"),
            ("tests/test_tool.py", "from tool import main\n\n\ndef test_main():\n    assert main() == 1\n"),
        ],
    );
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let target = g.find("tool::main")[0];
    let files: Vec<&str> = locate::tests_for(&g, target, 4).iter().map(|x| g.syms[x.symbol].file.as_str()).collect();
    assert_eq!(files, ["tests/test_tool.py"]);
    let text = locate::render_tests_for(&g, &dir, target, 4, false);
    assert!(text.contains("note: the call graph finds ≈") && text.contains("% of the tests that would fail"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `look main` with several `main`s lists them all — also when one of them is exactly `main`.
#[test]
fn look_asks_when_a_bare_name_has_namesakes() {
    let dir = project(
        "namesakes",
        &[
            ("tool.py", "def main():\n    return 1\n\n\ndef only_here():\n    return main()\n"),
            ("native/zoo.cpp", "int helper() { return 1; }\n\nint main() {\n    return helper();\n}\n"),
        ],
    );
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let ws = gitnext_ops::Workspace::open(&dir).unwrap();
    let text = gitnext_ops::look::look(&ws, &g, "main", false);
    assert!(text.starts_with("2 things are called `main`"), "{text}");
    assert!(text.contains("tool::main") && text.contains("main@3 (function) native/zoo.cpp:3"), "{text}");
    // each label in the list leads to exactly one of them
    assert!(gitnext_ops::look::look(&ws, &g, "main@3", false).contains("int main"));
    assert!(gitnext_ops::look::look(&ws, &g, "tool::main", false).contains("def main"));
    // a name without namesakes is answered at once, with how far to trust the callers
    let one = gitnext_ops::look::look(&ws, &g, "only_here", false);
    assert!(one.contains("def only_here"), "{one}");
    // sure / maybe apart in the callers list
    let callers = gitnext_ops::render::callers(&g, g.find("tool::main")[0], Default::default());
    assert!(callers.starts_with("callers of tool::main (1 sure"), "{callers}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Text search names the function each hit is in.
#[test]
fn text_hits_name_the_enclosing_function() {
    let dir = project("texthits", &[("tool.py", "def render(x):\n    return \"()::\" + x\n\n\nLIMIT = \"()::\"\n")]);
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let hits = gitnext_ops::look::text_hits(&g, &dir, "()::", 10);
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!(hits[0].starts_with("  tool.py:2 (in render): return"), "{hits:?}");
    assert!(hits[1].starts_with("  tool.py:5: LIMIT"), "{hits:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With a learned map `tests-for` names the tests that really ran the function — also the
/// ones the call graph cannot see — and says when the function changed since.
#[test]
fn learned_map_replaces_the_call_graph() {
    let dir = project(
        "learned",
        &[
            ("app.py", "def handler(x):\n    return x + 1\n\n\nREGISTRY = {\"h\": handler}\n\n\ndef dispatch(name, x):\n    return REGISTRY[name](x)\n"),
            ("tests/test_app.py", "from app import dispatch\n\n\ndef test_dispatch():\n    assert dispatch(\"h\", 1) == 2\n\n\ndef test_other():\n    assert True\n"),
        ],
    );
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let target = g.find("app::handler")[0];
    // `handler` is only reached through a dict: without a map the answer is the whole mirror
    // test file, including a test that never runs it
    let (t, source) = locate::tests_for_at(&g, &dir, target, 4);
    assert_eq!(source, locate::Source::Graph);
    assert!(t.iter().any(|x| g.syms[x.symbol].sym.name == "test_other"), "{:?}", t.iter().map(|x| &x.how).collect::<Vec<_>>());
    let hash = g.syms[target].sym.body_hash.clone();
    let row = |h: &str| ("app::handler".to_string(), h.to_string(), "tests/test_app.py".to_string(), "test_dispatch".to_string());
    gitnext_ops::gitnext_index::TestMap::write(&dir, "pytest-cov", &[row(&hash)]).unwrap();
    let (t, source) = locate::tests_for_at(&g, &dir, target, 4);
    assert_eq!(source, locate::Source::Learned { stale: false });
    assert_eq!(t.iter().map(|x| g.syms[x.symbol].sym.name.as_str()).collect::<Vec<_>>(), ["test_dispatch"]);
    let text = locate::render_tests_for(&g, &dir, target, 4, false);
    assert!(text.starts_with("tests that ran handler (1)") && text.contains("pytest tests/test_app.py::test_dispatch") && text.contains("ran this code in the coverage run"), "{text}");
    // the function was edited after the map was learned
    gitnext_ops::gitnext_index::TestMap::write(&dir, "pytest-cov", &[row("an older hash")]).unwrap();
    assert_eq!(locate::tests_for_at(&g, &dir, target, 4).1, locate::Source::Learned { stale: true });
    assert!(locate::render_tests_for(&g, &dir, target, 4, false).contains("the code changed since that run"));
    // a function the map does not know falls back to the call graph
    assert_eq!(locate::tests_for_at(&g, &dir, g.find("app::dispatch")[0], 4).1, locate::Source::Graph);
    let _ = std::fs::remove_dir_all(&dir);
}

/// JavaScript / TypeScript: `it('…', () => …)` is not a named function — the test is the file.
#[test]
fn a_typescript_test_file_is_a_test() {
    let dir = std::env::temp_dir().join(format!("gitnext-testsfor-ts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("package.json"), "{ \"name\": \"demo\", \"devDependencies\": { \"vitest\": \"^3.0.0\" } }\n").unwrap();
    std::fs::write(dir.join("src/price.ts"), "export function totalPrice(items: number[]): number {\n  return items.reduce((a, b) => a + b, 0)\n}\n\nexport function unused(): number {\n  return 1\n}\n").unwrap();
    std::fs::write(
        dir.join("tests/cart.ts"),
        "import { describe, expect, it } from 'vitest'\nimport { totalPrice } from '../src/price'\n\ndescribe('cart', () => {\n  it('adds up', () => {\n    expect(totalPrice([1, 2])).toBe(3)\n  })\n})\n",
    )
    .unwrap();
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let t = locate::tests_for(&g, g.find("totalPrice")[0], 4);
    let files: Vec<&str> = t.iter().map(|x| g.syms[x.symbol].file.as_str()).collect();
    assert_eq!(files, ["tests/cart.ts"]);
    // the command runs that file
    let cmds = locate::run_commands(&g, &dir, &t);
    assert!(cmds.iter().any(|c| c.contains("vitest run") && c.contains("tests/cart.ts")), "{cmds:?}");
    // a function no test file calls gets none
    assert!(locate::tests_for(&g, g.find("unused")[0], 4).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
