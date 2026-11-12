//! Renames that broke the code on the eval's large sample (run 6): each test is one of the causes.

use gitnext_ops::edit;
use gitnext_ops::gitnext_index::{Graph, Index};
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-rename-more-{name}-{}", std::process::id()));
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

fn read(dir: &Path, f: &str) -> String {
    std::fs::read_to_string(dir.join(f)).unwrap()
}

fn callers(g: &Graph, name: &str) -> Vec<String> {
    let i = g.find(name)[0];
    let mut v: Vec<String> = g.callers(i).iter().map(|&(c, _)| g.syms[c].sym.name.clone()).collect();
    v.sort();
    v.dedup();
    v
}

const PY: &[(&str, &str)] = &[
    ("pkg/__init__.py", "from .core import Ctx as Ctx\nfrom .core import helper as helper\n"),
    (
        "pkg/core.py",
        "class Ctx:\n    def find_object(self, t):\n        return t\n\n\ndef helper(x):\n    return x\n\n\ndef get_ctx():\n    return Ctx()\n\n\ndef decorator(object_type):\n    def wrap(f):\n        def new_func(*args, **kwargs):\n            ctx = get_ctx()\n            obj = ctx.find_object(object_type)\n            return f(obj, helper(1))\n        return new_func\n    return wrap\n",
    ),
    ("tests/test_x.py", "import pkg\n\n\ndef test_helper():\n    assert pkg.helper(1) == 1\n"),
];

/// click: `click.style(...)` in tests and examples — the package's `__init__` re-exports the function.
#[test]
fn python_call_through_the_package_is_a_caller_and_is_renamed() {
    let dir = project("py-package", PY);
    let g = graph(&dir);
    assert_eq!(callers(&g, "helper"), ["decorator", "test_helper"]);
    let i = g.find("helper")[0];
    edit::rename(&dir, &g, i, "assist", false).unwrap();
    assert!(read(&dir, "tests/test_x.py").contains("pkg.assist(1)"));
    assert!(read(&dir, "pkg/__init__.py").contains("import assist as assist"));
    assert!(read(&dir, "pkg/core.py").contains("return f(obj, assist(1))"));
}

/// click decorators: the call sits in a function nested two levels deep.
#[test]
fn python_calls_inside_nested_functions_belong_to_the_outer_function() {
    let dir = project("py-nested", PY);
    let g = graph(&dir);
    assert_eq!(callers(&g, "get_ctx"), ["decorator"]);
    assert_eq!(callers(&g, "find_object"), ["decorator"]);
    let i = g.find("find_object")[0];
    edit::rename(&dir, &g, i, "lookup", false).unwrap();
    assert!(read(&dir, "pkg/core.py").contains("obj = ctx.lookup(object_type)"));
}

/// indexmap: `use crate::util::{third, slice_eq};` kept the old name, the calls got the new one.
#[test]
fn rust_name_inside_a_use_group_is_renamed() {
    let dir = project(
        "rs-group",
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/lib.rs", "mod map;\nmod util;\n"),
            ("src/util.rs", "pub(crate) fn third<A, B, C>(t: (A, B, C)) -> C {\n    t.2\n}\n\npub(crate) fn first<A, B>(t: (A, B)) -> A {\n    t.0\n}\n"),
            (
                "src/map.rs",
                "use crate::util::{first, third as last};\nuse crate::util::{\n    self,\n    third,\n};\nuse std::cmp::{max, min as third_min};\n\npub fn pick(t: (u8, u8, u8)) -> u8 {\n    max(third(t), last(t)) + first((1, 2)) + util::third(t) + third_min(1, 2)\n}\n",
            ),
        ],
    );
    let g = graph(&dir);
    let i = g.find("third")[0];
    edit::rename(&dir, &g, i, "third_of", false).unwrap();
    let map = read(&dir, "src/map.rs");
    assert!(map.contains("use crate::util::{first, third_of as last};"), "{map}");
    assert!(map.contains("    third_of,\n"), "{map}");
    assert!(map.contains("max(third_of(t), last(t))"), "{map}");
    assert!(map.contains("use std::cmp::{max, min as third_min};"), "{map}");
    assert!(read(&dir, "src/util.rs").contains("fn third_of<A, B, C>"));
}

/// yaml: `#stringifyFlowMap` was refused with the `#`, and without it became a public member.
#[test]
fn typescript_private_member_stays_private() {
    let src = "export class Box {\n  #count = 0\n\n  #bump(n: number) {\n    return this.#count + n\n  }\n\n  next() {\n    return this.#bump(1)\n  }\n}\n";
    for (case, new) in [("with", "#raise"), ("without", "raise")] {
        let dir = project(&format!("ts-private-{case}"), &[("src/box.ts", src)]);
        let g = graph(&dir);
        let i = g.find("#bump")[0];
        edit::rename(&dir, &g, i, new, false).unwrap();
        let text = read(&dir, "src/box.ts");
        assert!(text.contains("  #raise(n: number) {") && text.contains("return this.#raise(1)"), "{text}");
    }
    let dir = project("ts-private-bad", &[("src/box.ts", src)]);
    let g = graph(&dir);
    assert!(edit::rename(&dir, &g, g.find("#bump")[0], "#1x", false).is_err());
}

/// click, four more shapes of a use that no call edge reached.
#[test]
fn python_uses_that_are_not_plain_calls() {
    let dir = project(
        "py-shapes",
        &[
            ("pkg/__init__.py", "from . import compat\nfrom .core import command as command\n"),
            ("pkg/compat.py", "def strip_ansi(s):\n    return s\n\n\ndef hidden(p):\n    return p\n\n\ndef visible(p):\n    return p\n\n\ndef ask(hide):\n    f = hidden if hide else visible\n    return f('?')\n"),
            (
                "pkg/core.py",
                "class Ctx:\n    def call_on_close(self, f):\n        return f\n\n    def short_help(self):\n        return ''\n\n\ndef command(f):\n    return f\n\n\ndef listing(pairs):\n    from .core import command\n    return [command.short_help() for name, command in pairs]\n",
            ),
            (
                "tests/test_y.py",
                "import pkg\nimport pkg.compat\n\n\ndef test_chain():\n    assert pkg.compat.strip_ansi('x') == 'x'\n    table = [(pkg.compat, 'strip_ansi', pkg.compat.strip_ansi)]\n    assert table\n\n\ndef test_decorator(ctx):\n    @ctx.call_on_close\n    def done():\n        pass\n",
            ),
        ],
    );
    let g = graph(&dir);
    // a dotted module path, called and passed as a value
    assert_eq!(callers(&g, "strip_ansi"), ["test_chain"]);
    // a bare name used as a value
    assert_eq!(callers(&g, "hidden"), ["ask"]);
    // a decorator on a nested function, through an unknown receiver
    assert_eq!(callers(&g, "call_on_close"), ["test_decorator"]);
    // the loop variable `command` is not the imported function `command`
    assert_eq!(callers(&g, "short_help"), ["listing"]);
    for (old, new, file, want) in [
        ("strip_ansi", "plain", "tests/test_y.py", "pkg.compat.plain('x')"),
        ("hidden", "masked", "pkg/compat.py", "f = masked if hide else visible"),
        ("call_on_close", "at_exit", "tests/test_y.py", "@ctx.at_exit"),
        ("short_help", "brief", "pkg/core.py", "command.brief() for name, command"),
    ] {
        let g = graph(&dir);
        edit::rename(&dir, &g, g.find(old)[0], new, false).unwrap();
        let text = read(&dir, file);
        assert!(text.contains(want), "{old}: {text}");
    }
    assert!(read(&dir, "tests/test_y.py").contains("'strip_ansi', pkg.compat.plain)"));
}
