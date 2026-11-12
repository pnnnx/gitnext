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
