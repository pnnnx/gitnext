//! `gitnext dead`: only what nothing refers to — a call, or the name written anywhere else.

use gitnext_ops::gitnext_index::Index;
use gitnext_ops::insight::dead;

const CORE: &str = "\
import typing as t


class _Meta(type):
    pass


class _Base(metaclass=_Meta):
    pass


class _Vars(t.TypedDict):
    name: str


def _really_dead(x):
    return x + 1


def _only_in_a_comment(x):
    return x


def _callback(x):
    return x * 2


def _value():
    return 1


def source_vars() -> _Vars:
    # _only_in_a_comment is mentioned here and nowhere else
    return {\"name\": str(list(map(_callback, [1])))}


def __getattr__(name):
    if name == \"Base\":
        return _Base
    return _value
";

#[test]
fn a_name_used_as_a_value_or_a_type_is_not_dead() {
    let dir = std::env::temp_dir().join(format!("gitnext-dead-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in [("pkg/__init__.py", "from .core import source_vars\n"), ("pkg/core.py", CORE)] {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let d = dead(&g, &dir);
    let all = d.likely.join("\n") + &d.public_unused.join("\n");
    // never called, but referred to: a metaclass, a base returned as a value, a return type, a callback
    for used in ["_Meta", "_Base", "_Vars", "_callback", "_value"] {
        assert!(!all.contains(used), "`{used}` is used, not dead:\n{all}");
    }
    // nothing names these outside themselves (a comment does not count)
    for unused in ["_really_dead", "_only_in_a_comment"] {
        assert!(d.likely.iter().any(|l| l.contains(unused)), "`{unused}` is dead:\n{all}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn dead_in(tag: &str, files: &[(&str, &str)]) -> gitnext_ops::insight::Dead {
    let dir = std::env::temp_dir().join(format!("gitnext-dead-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let d = dead(&idx.graph().unwrap(), &dir);
    let _ = std::fs::remove_dir_all(&dir);
    d
}

const FILTERS: &str = "\
class Reindent:
    def process(self, tlist):
        func_name = f'_process_{type(tlist).__name__}'
        func = getattr(self, func_name.lower(), self.process)
        return func(tlist)

    def _process_where(self, tlist):
        return tlist

    def visit(self, kind):
        return getattr(self, '_visit_' + kind)()

    def _visit_call(self):
        return 1

    def handle(self, kind):
        return getattr(self, '_handle_%s' % kind)()

    def _handle_join(self):
        return 2

    def click(self, kind):
        return getattr(self, kind + '_handler')()

    def _click_handler(self):
        return 3

    def _orphan_thing(self):
        return 4

    def _really_dead(self):
        return 5


def __getattr__(name):
    return globals()[f\"_{name}\"]


def describe(x):
    return '_orphan_' + x
";

#[test]
fn a_name_built_next_to_reflection_is_not_dead() {
    let d = dead_in("py", &[("pkg/__init__.py", "from .filters import Reindent, describe\n"), ("pkg/filters.py", FILTERS)]);
    let all = d.likely.join("\n") + "\n" + &d.public_unused.join("\n");
    // f-string + .lower(), concatenation, `%`, a suffix: each next to getattr
    for used in ["_process_where", "_visit_call", "_handle_join", "_click_handler"] {
        assert!(!all.contains(used), "`{used}` is reached through getattr, not dead:\n{all}");
    }
    // `f"_{name}"` is too short to be a piece of a name; `'_orphan_' + x` has no reflection near it
    for unused in ["_really_dead", "_orphan_thing"] {
        assert!(d.likely.iter().any(|l| l.contains(unused)), "`{unused}` is dead:\n{all}");
    }
}

const HANDLERS: &str = "\
package org.example;

public class Handlers {
    public Object dispatch(String kind) throws Exception {
        return getClass().getDeclaredMethod(\"handle\" + kind).invoke(this);
    }

    private Object handleSave() {
        return null;
    }

    private Object neverCalled() {
        return null;
    }

    protected char previous() {
        return 'a';
    }
}
";

#[test]
fn java_reflection_and_protected() {
    let d = dead_in("java", &[("src/main/java/org/example/Handlers.java", HANDLERS)]);
    let all = d.likely.join("\n") + "\n" + &d.public_unused.join("\n");
    assert!(!all.contains("handleSave"), "reached through getDeclaredMethod(\"handle\" + kind):\n{all}");
    assert!(d.likely.iter().any(|l| l.contains("neverCalled")), "private and unreferenced:\n{all}");
    // protected: API for subclasses, not something to delete
    assert!(!d.likely.iter().any(|l| l.contains("previous")), "protected is not likely dead:\n{all}");
    assert!(d.public_unused.iter().any(|l| l.contains("previous")), "protected is public but unused:\n{all}");
}

const BENCH: &str = "\
#![feature(test)]
extern crate test;

use test::Bencher;

fn helper() -> u32 {
    1
}

#[bench]
fn insert_10_000(b: &mut Bencher) {
    b.iter(helper);
}

#[no_mangle]
extern \"C\" fn exported_hook() {}

fn unused_in_bench() {}
";

#[test]
fn rust_functions_run_by_an_attribute_are_not_dead() {
    let d = dead_in("rs", &[("Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.1.0\"\n"), ("src/lib.rs", "pub fn f() {}\n"), ("benches/bench.rs", BENCH)]);
    let all = d.likely.join("\n") + "\n" + &d.public_unused.join("\n");
    for used in ["insert_10_000", "exported_hook", "helper"] {
        assert!(!all.contains(used), "`{used}` is run by the bench harness or the linker:\n{all}");
    }
    assert!(d.likely.iter().any(|l| l.contains("unused_in_bench")), "a plain function nothing calls:\n{all}");
}
