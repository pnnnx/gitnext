//! Language tables for the generic extractor (`generic.rs`): TypeScript /
//! JavaScript, Go, Java. Each is what the engine cannot guess — which nodes
//! are definitions, how imports bind names, the module path, what is public,
//! what is a test.

use crate::generic::{join_relative, path_module, Ctx, Def, Imports, Spec};
use crate::{CrateHint, EdgeKind, Kind, Lang};
use tree_sitter::Node;

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`')
}

// ------------------------------------------------------------------ TypeScript / JavaScript

pub(crate) static TYPESCRIPT: Spec = Spec { lang: Lang::TypeScript, language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), ..JS_BASE };
pub(crate) static TSX: Spec = Spec { lang: Lang::Tsx, language: || tree_sitter_typescript::LANGUAGE_TSX.into(), ..JS_BASE };
pub(crate) static JAVASCRIPT: Spec = Spec { lang: Lang::JavaScript, language: || tree_sitter_javascript::LANGUAGE.into(), ..JS_BASE };

const JS_BASE: Spec = Spec {
    lang: Lang::JavaScript,
    language: || tree_sitter_javascript::LANGUAGE.into(),
    classify: js_classify,
    module: |rel, _, _, _| path_module(rel),
    imports: js_imports,
    public: |n, _, _| n.parent().is_some_and(|p| p.kind() == "export_statement") || n.parent().and_then(|p| p.parent()).is_some_and(|p| p.kind() == "export_statement") || n.kind() == "method_definition" && !n.child(0).is_some_and(|c| c.kind() == "accessibility_modifier"),
    is_test: |_, _, _| false,
    calls: &[("call_expression", "function")],
    member: &[("member_expression", "object", "property")],
    new: &[("new_expression", "constructor"), ("jsx_opening_element", "name"), ("jsx_self_closing_element", "name")],
    this: &["this", "super"],
    bare_is_self: false,
    wrappers: &["export_statement", "lexical_declaration", "variable_declaration"],
    heritage: &[("extends_clause", EdgeKind::Inherits), ("implements_clause", EdgeKind::Implements), ("extends_type_clause", EdgeKind::Inherits)],
    fields: &["public_field_definition", "field_definition", "property_signature"],
    builtin_types: &["string", "number", "boolean", "void", "any", "unknown", "never", "object", "Promise", "Array", "Record", "Partial", "Readonly", "Map", "Set", "Date", "Error", "T", "K", "V"],
};

fn js_classify<'t>(n: Node<'t>, _src: &[u8], _ctx: &Ctx) -> Option<Def<'t>> {
    let def = |kind, name: Option<Node<'t>>| name.map(|name| Def { kind, name, owner: None, func: None });
    match n.kind() {
        "function_declaration" | "generator_function_declaration" | "function_signature" => def(Kind::Function, n.child_by_field_name("name")),
        "method_definition" | "method_signature" | "abstract_method_signature" => def(Kind::Function, n.child_by_field_name("name")),
        "class_declaration" | "abstract_class_declaration" | "class" => def(Kind::Class, n.child_by_field_name("name")),
        "interface_declaration" => def(Kind::Trait, n.child_by_field_name("name")),
        "type_alias_declaration" => def(Kind::TypeAlias, n.child_by_field_name("name")),
        "enum_declaration" => def(Kind::Enum, n.child_by_field_name("name")),
        // const f = (…) => …; const f = function (…) {…}
        "variable_declarator" | "public_field_definition" | "field_definition" => {
            let v = js_function_value(n.child_by_field_name("value")?, _src)?;
            let name = n.child_by_field_name("name").or_else(|| n.child_by_field_name("property"))?;
            if !matches!(name.kind(), "identifier" | "property_identifier") {
                return None;
            }
            Some(Def { kind: Kind::Function, name, owner: None, func: Some(v) })
        }
        _ => None,
    }
}

/// The function behind a value: `(…) => …`, and through wrappers —
/// `(… ) as T`, `x satisfies T`, `memo(() => …)`, `forwardRef((p, ref) => …)`.
fn js_function_value<'t>(v: Node<'t>, src: &[u8]) -> Option<Node<'t>> {
    match v.kind() {
        "arrow_function" | "function_expression" | "function" | "generator_function" => Some(v),
        "as_expression" | "satisfies_expression" | "parenthesized_expression" | "non_null_expression" | "type_assertion" => js_function_value(v.named_child(0)?, src),
        // only wrappers that return the component / callback itself, not any call taking a lambda
        "call_expression" => {
            let callee = v.child_by_field_name("function")?;
            let name = text(callee.child_by_field_name("property").unwrap_or(callee), src);
            if !matches!(name, "memo" | "forwardRef" | "observer" | "useCallback" | "defineComponent" | "lazy" | "styled") {
                return None;
            }
            let args = v.child_by_field_name("arguments")?;
            let mut c = args.walk();
            let f = args.named_children(&mut c).find(|a| matches!(a.kind(), "arrow_function" | "function_expression" | "function"));
            f
        }
        _ => None,
    }
}

fn js_imports(root: Node, src: &[u8], rel: &str, _module: &str, pkg: Option<&CrateHint>) -> Imports {
    let own = pkg.map(|h| h.name.as_str()).filter(|n| !n.is_empty());
    let mut out = Imports::default();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            match ch.kind() {
                "import_statement" => js_import(ch, src, rel, own, &mut out),
                // const x = require("y"); const { a } = require("y")
                "variable_declarator" => {
                    let Some(v) = ch.child_by_field_name("value") else { continue };
                    if v.kind() != "call_expression" || v.child_by_field_name("function").map(|f| text(f, src)) != Some("require") {
                        stack.push(ch);
                        continue;
                    }
                    let Some(arg) = v.child_by_field_name("arguments").and_then(|a| a.named_child(0)) else { continue };
                    let target = js_target(rel, unquote(text(arg, src)));
                    out.edges.push((target.clone(), line(ch)));
                    if let Some(name) = ch.child_by_field_name("name") {
                        if name.kind() == "identifier" {
                            out.aliases.insert(text(name, src).into(), target);
                        } else {
                            let mut c2 = name.walk();
                            for p in name.named_children(&mut c2) {
                                let local = p.child_by_field_name("value").unwrap_or(p);
                                let key = p.child_by_field_name("key").unwrap_or(p);
                                out.aliases.insert(text(local, src).into(), format!("{target}::{}", text(key, src)));
                            }
                        }
                    }
                }
                "class_body" | "statement_block" | "function_declaration" | "arrow_function" | "class_declaration" | "method_definition" => {}
                _ => stack.push(ch),
            }
        }
    }
    out
}

fn js_import(n: Node, src: &[u8], rel: &str, own: Option<&str>, out: &mut Imports) {
    let Some(source) = n.child_by_field_name("source") else { return };
    let spec = unquote(text(source, src));
    // `import { create } from "zustand"` inside zustand itself: its own code, found by name
    let own_pkg = own.is_some_and(|o| spec == o || spec.starts_with(&format!("{o}/")));
    let target = js_target(rel, spec);
    if !own_pkg {
        out.edges.push((target.clone(), line(n)));
    }
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        let mut c = x.walk();
        for ch in x.named_children(&mut c) {
            match ch.kind() {
                // import x from "m" → x stands for the module's default export
                "identifier" if x.kind() == "import_clause" && !own_pkg => {
                    out.aliases.insert(text(ch, src).into(), target.clone());
                }
                "namespace_import" if !own_pkg => {
                    if let Some(id) = ch.named_child(0) {
                        out.aliases.insert(text(id, src).into(), target.clone());
                    }
                }
                "import_specifier" => {
                    let name = ch.child_by_field_name("name").map(|x| text(x, src)).unwrap_or("");
                    let local = ch.child_by_field_name("alias").map(|x| text(x, src)).unwrap_or(name);
                    out.aliases.insert(local.into(), if own_pkg { name.to_string() } else { format!("{target}::{name}") });
                }
                "string" => {}
                _ => stack.push(ch),
            }
        }
    }
}

/// `./util/fmt` from `src/store.ts` → `src::util::fmt`; a package keeps its name: `react`, `@tanstack/react-query`, `node:fs`.
fn js_target(rel: &str, spec: &str) -> String {
    if spec.starts_with('.') {
        join_relative(rel, spec)
    } else {
        spec.to_string()
    }
}

// ------------------------------------------------------------------ Go

pub(crate) static GO: Spec = Spec {
    lang: Lang::Go,
    language: || tree_sitter_go::LANGUAGE.into(),
    classify: go_classify,
    module: |rel, _, _, _| {
        // a package is a directory: every file in it shares the module path
        let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if dir.is_empty() { "main".into() } else { dir.split('/').map(|s| s.replace(['-', '.'], "_")).collect::<Vec<_>>().join("::") }
    },
    imports: go_imports,
    public: |_, name, _| name.chars().next().is_some_and(char::is_uppercase),
    is_test: |n, name, _| n.kind() == "function_declaration" && (name.starts_with("Test") || name.starts_with("Benchmark") || name.starts_with("Fuzz") || name.starts_with("Example")),
    calls: &[("call_expression", "function")],
    member: &[("selector_expression", "operand", "field")],
    new: &[],
    this: &[],
    bare_is_self: false,
    wrappers: &[],
    heritage: &[],
    fields: &["field_declaration"],
    builtin_types: &["string", "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64", "float32", "float64", "bool", "byte", "rune", "error", "any", "uintptr", "complex64", "complex128"],
};

fn go_classify<'t>(n: Node<'t>, src: &[u8], ctx: &Ctx) -> Option<Def<'t>> {
    match n.kind() {
        "function_declaration" => Some(Def { kind: Kind::Function, name: n.child_by_field_name("name")?, owner: None, func: None }),
        "method_declaration" => {
            // func (s *Store) Save() → Store::Save
            let recv = n.child_by_field_name("receiver")?.named_child(0)?.child_by_field_name("type")?;
            let ty = text(recv, src).trim_start_matches('*');
            let ty = ty.split('[').next().unwrap_or(ty);
            Some(Def { kind: Kind::Method, name: n.child_by_field_name("name")?, owner: Some(format!("{}::{ty}", ctx.module)), func: None })
        }
        "type_spec" | "type_alias" => {
            let kind = match n.child_by_field_name("type").map(|t| t.kind()) {
                Some("struct_type") if n.kind() == "type_spec" => Kind::Struct,
                Some("interface_type") if n.kind() == "type_spec" => Kind::Trait,
                _ => Kind::TypeAlias,
            };
            Some(Def { kind, name: n.child_by_field_name("name")?, owner: None, func: None })
        }
        "method_elem" if ctx.owner.is_some() => Some(Def { kind: Kind::Function, name: n.child_by_field_name("name")?, owner: None, func: None }),
        _ => None,
    }
}

fn go_imports(root: Node, src: &[u8], _rel: &str, _module: &str, pkg: Option<&CrateHint>) -> Imports {
    let mut out = Imports::default();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            if ch.kind() != "import_spec" {
                if matches!(ch.kind(), "import_declaration" | "import_spec_list") {
                    stack.push(ch);
                }
                continue;
            }
            let Some(p) = ch.child_by_field_name("path") else { continue };
            let path = unquote(text(p, src));
            // in-repo package: module path from go.mod stripped → repo directory
            let target = match pkg.filter(|h| !h.name.is_empty()).and_then(|h| path.strip_prefix(&h.name).map(|rest| (h, rest))) {
                Some((h, rest)) => {
                    let dir = format!("{}/{}", h.dir, rest.trim_start_matches('/'));
                    dir.split('/').filter(|s| !s.is_empty()).map(|s| s.replace(['-', '.'], "_")).collect::<Vec<_>>().join("::")
                }
                None => path.to_string(),
            };
            out.edges.push((target.clone(), line(ch)));
            let local = ch.child_by_field_name("name").map(|n| text(n, src).to_string()).unwrap_or_else(|| {
                // the package name is (almost always) the last path element, without a version suffix
                let mut segs = path.rsplit('/');
                let last = segs.next().unwrap_or(path);
                if last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit()) && last.len() > 1 { segs.next().unwrap_or(last).to_string() } else { last.replace('-', "_") }
            });
            if local != "_" && local != "." {
                out.aliases.insert(local, target);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ Java

pub(crate) static JAVA: Spec = Spec {
    lang: Lang::Java,
    language: || tree_sitter_java::LANGUAGE.into(),
    classify: java_classify,
    module: |rel, root, src, _| {
        let mut c = root.walk();
        let pkg = root.named_children(&mut c).find(|n| n.kind() == "package_declaration").and_then(|p| p.named_child(0)).map(|n| text(n, src).replace('.', "::"));
        pkg.unwrap_or_else(|| {
            let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let dir = dir.split_once("/java/").map(|(_, r)| r).unwrap_or(dir);
            dir.replace('/', "::")
        })
    },
    imports: java_imports,
    public: |n, _, src| n.child(0).is_some_and(|m| m.kind() == "modifiers" && text(m, src).split_whitespace().any(|w| w == "public")),
    is_test: |n, name, src| {
        (n.kind() == "class_declaration" && (name.ends_with("Test") || name.ends_with("Tests") || name.ends_with("IT")))
            || n.child(0).is_some_and(|m| m.kind() == "modifiers" && text(m, src).contains("@Test"))
    },
    calls: &[("method_invocation", "name")],
    member: &[("field_access", "object", "field")],
    new: &[("object_creation_expression", "type")],
    this: &["this", "super"],
    bare_is_self: true,
    wrappers: &[],
    heritage: &[("superclass", EdgeKind::Inherits), ("super_interfaces", EdgeKind::Implements), ("extends_interfaces", EdgeKind::Inherits)],
    fields: &["field_declaration", "constant_declaration", "formal_parameter"],
    builtin_types: &["String", "Object", "Integer", "Long", "Double", "Boolean", "List", "Map", "Set", "Optional", "T", "K", "V", "E"],
};

fn java_classify<'t>(n: Node<'t>, _src: &[u8], _ctx: &Ctx) -> Option<Def<'t>> {
    let kind = match n.kind() {
        "method_declaration" | "constructor_declaration" | "compact_constructor_declaration" => Kind::Function,
        "class_declaration" | "record_declaration" => Kind::Class,
        "interface_declaration" | "annotation_type_declaration" => Kind::Trait,
        "enum_declaration" => Kind::Enum,
        _ => return None,
    };
    Some(Def { kind, name: n.child_by_field_name("name")?, owner: None, func: None })
}

fn java_imports(root: Node, src: &[u8], _rel: &str, _module: &str, _pkg: Option<&CrateHint>) -> Imports {
    let mut out = Imports::default();
    let mut c = root.walk();
    for n in root.named_children(&mut c).filter(|n| n.kind() == "import_declaration") {
        let t = text(n, src);
        let wildcard = t.contains(".*");
        let Some(path) = n.named_children(&mut n.walk()).find(|x| matches!(x.kind(), "scoped_identifier" | "identifier")).map(|x| text(x, src).replace('.', "::")) else { continue };
        out.edges.push((path.clone(), line(n)));
        if !wildcard {
            let local = path.rsplit("::").next().unwrap_or(&path).to_string();
            out.aliases.insert(local, path);
        }
    }
    out
}
