//! Rust methods whose name several types share: the receiver's type picks the right one —
//! a field of a struct in the same file (`self.core.drain()`), `Self` in an `impl` that sits in
//! another module than the struct, a `&Self` parameter, and what a `macro_rules!` expands to.

use gitnext_index::{Graph, Index};

fn graph(name: &str, files: &[(&str, &str)]) -> Graph {
    let dir = std::env::temp_dir().join(format!("gitnext-rust-recv-{name}-{}", std::process::id()));
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

/// (sure callers, guessed callers) of `target`, by name.
fn callers(g: &Graph, target: &str) -> (Vec<String>, Vec<String>) {
    let found = g.find(target);
    assert_eq!(found.len(), 1, "{target}: {found:?}");
    let (mut sure, mut guess) = (Vec::new(), Vec::new());
    for (c, _) in g.callers(found[0]) {
        let name = g.syms[c].sym.name.clone();
        if g.is_guess(c, found[0]) { guess.push(name) } else { sure.push(name) }
    }
    sure.sort();
    sure.dedup();
    guess.sort();
    guess.dedup();
    (sure, guess)
}

const CARGO: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

#[test]
fn fields_self_and_macros() {
    let g = graph(
        "fields",
        &[
            ("Cargo.toml", CARGO),
            (
                "src/lib.rs",
                r#"pub mod core;
pub mod display;
pub mod other;

use crate::core::Core;

pub struct Map {
    core: Core,
}

impl Map {
    pub fn drain_all(&mut self) {
        self.core.drain_items();
    }
    pub fn label(&self) -> &str {
        "map"
    }
    pub fn same(&self, rhs: &Self) -> bool {
        rhs.label() == "map"
    }
}

pub struct View<'a> {
    map: &'a Map,
}

impl<'a> View<'a> {
    pub fn peek(&self) {
        self.map.core.drain_items();
    }
}

#[macro_export]
macro_rules! make_map {
    () => {
        $crate::Map::build()
    };
}

impl Map {
    pub fn build() -> Map {
        Map { core: Core {} }
    }
}
"#,
            ),
            (
                "src/core.rs",
                r#"pub struct Core {}

impl Core {
    pub fn drain_items(&mut self) {}
}
"#,
            ),
            (
                "src/other.rs",
                r#"pub struct Other {}

impl Other {
    pub fn drain_items(&mut self) {}
    pub fn label(&self) -> &str {
        "other"
    }
}
"#,
            ),
            (
                "src/display.rs",
                r#"use crate::Map;
use std::fmt;

impl fmt::Display for Map {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.label())
    }
}
"#,
            ),
        ],
    );
    // a field of a struct in the same file: `self.core.drain_items()` is `Core::drain_items`
    let (sure, _) = callers(&g, "Core::drain_items");
    assert!(sure.contains(&"drain_all".to_string()), "{sure:?}");
    let (sure, guess) = callers(&g, "Other::drain_items");
    assert!(!sure.contains(&"drain_all".to_string()), "{sure:?}");
    // a field chain leaving the file (`map: &'a Map`, but `Map.core` is known here too — same file)
    let (sure, _) = callers(&g, "Core::drain_items");
    assert!(sure.contains(&"peek".to_string()), "{sure:?} / {guess:?}");
    // `Self` in an `impl` of another module, and a `&Self` parameter
    let (sure, _) = callers(&g, "Map::label");
    assert_eq!(sure, ["fmt", "same"]);
    let (sure, _) = callers(&g, "Other::label");
    assert!(sure.is_empty(), "{sure:?}");
    // what a macro expands to
    let (sure, _) = callers(&g, "Map::build");
    assert_eq!(sure, ["make_map"]);
}

#[test]
fn two_types_of_one_name_stay_unknown() {
    // `impl a::Slice` written in module `c`: the owner reads `c::Slice`; with `a::Slice` and
    // `b::Slice` both in the crate, `self.span()` must not become a sure call of either
    let g = graph(
        "slices",
        &[
            ("Cargo.toml", CARGO),
            ("src/lib.rs", "pub mod a;\npub mod b;\npub mod c;\n"),
            ("src/a.rs", "pub struct Slice {}\n\nimpl Slice {\n    pub fn span(&self) -> usize {\n        1\n    }\n}\n"),
            ("src/b.rs", "pub struct Slice {}\n\nimpl Slice {\n    pub fn span(&self) -> usize {\n        2\n    }\n}\n"),
            ("src/c.rs", "use crate::a::Slice;\n\nimpl Slice {\n    pub fn twice(&self) -> usize {\n        self.span() * 2\n    }\n}\n"),
        ],
    );
    for t in ["a::Slice::span", "b::Slice::span"] {
        let (sure, _) = callers(&g, t);
        assert!(!sure.contains(&"twice".to_string()), "{t}: {sure:?}");
    }
}
