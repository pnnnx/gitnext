//! Rust tests declared inside a macro call (`quickcheck! { fn prop(..) -> bool { .. } }`, indexmap's
//! `quickcheck_limit!`): they are functions of the graph, marked as tests, and the library functions they call
//! have them as callers.

use gitnext_index::{Graph, Index};

fn graph(name: &str, files: &[(&str, &str)]) -> Graph {
    let dir = std::env::temp_dir().join(format!("gitnext-rust-macro-tests-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, code) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, code).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    g
}

const CARGO: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

const LIB: &str = r#"pub struct Bag {
    items: Vec<u32>,
}

impl Bag {
    pub fn new() -> Self {
        Bag { items: Vec::new() }
    }
    pub fn insert(&mut self, x: u32) {
        self.items.push(x);
    }
    pub fn holds(&self, x: u32) -> bool {
        self.items.contains(&x)
    }
}
"#;

const QUICK: &str = r#"use demo::Bag;

macro_rules! quickcheck_limit {
    { $( fn $f:ident($($a:ident : $t:ty),*) -> $r:ty { $($c:tt)* } )* } => {
        quickcheck::quickcheck! { $( #[test] fn $f() {} )* }
    };
}

fn filled(xs: &[u32]) -> Bag {
    let mut bag = Bag::new();
    for &x in xs {
        bag.insert(x);
    }
    bag
}

quickcheck_limit! {
    fn inserted_are_contained(xs: Vec<u32>) -> bool {
        let bag = filled(&xs);
        xs.iter().all(|&x| bag.holds(x))
    }

    fn empty_contains_nothing(x: u32) -> bool {
        !Bag::new().holds(x)
    }
}
"#;

#[test]
fn tests_in_a_macro_are_tests_of_the_graph() {
    let g = graph("quick", &[("Cargo.toml", CARGO), ("src/lib.rs", LIB), ("tests/quick.rs", QUICK)]);
    for t in ["inserted_are_contained", "empty_contains_nothing"] {
        let found = g.find(t);
        assert_eq!(found.len(), 1, "{t}: {found:?}");
        let s = &g.syms[found[0]].sym;
        assert!(s.test, "{t} is a test");
        assert!(s.params.is_empty(), "{t}: what cargo runs is `fn {t}()`");
        assert_eq!(s.start_line, if t == "inserted_are_contained" { 18 } else { 23 });
    }
    let callers_of = |target: &str| -> Vec<String> {
        let found = g.find(target);
        assert_eq!(found.len(), 1, "{target}: {found:?}");
        let mut names: Vec<String> = g.callers(found[0]).into_iter().map(|(c, _)| g.syms[c].sym.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    };
    assert_eq!(callers_of("filled"), vec!["inserted_are_contained"]);
    assert!(callers_of("Bag::new").contains(&"empty_contains_nothing".to_string()));
    let contains = callers_of("Bag::holds");
    assert!(contains.contains(&"inserted_are_contained".to_string()) && contains.contains(&"empty_contains_nothing".to_string()), "{contains:?}");
}
