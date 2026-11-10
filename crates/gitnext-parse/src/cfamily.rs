//! C / C++ extractor (one grammar: tree-sitter-cpp also parses C).
//!
//! Emits functions and prototypes, structs / classes / unions / enums (fields,
//! base classes → `inherits`), methods (inline, declared in the class, or defined
//! out of line as `Foo::bar`), namespaces, typedefs / `using`, macros,
//! `#include` (→ `imports`), calls (incl. `new T(...)`) and type uses.
//!
//! Qualified names follow C++: `ns::Class::method`. C functions are global, so
//! their qualified name is just the function name; the file is kept in the index.

use crate::rust::collapse;
use crate::{name_pos, first_sentence, hash_hex, is_test_path, Edge, EdgeKind, FileAnalysis, Kind, Param, Symbol};
use std::path::Path;
use tree_sitter::{Node, Parser};

pub fn analyze(rel: &Path, src: &str) -> anyhow::Result<FileAnalysis> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_cpp::LANGUAGE.into())?;
    // Export/visibility macros confuse the grammar; blank them out (same length,
    // so lines and byte offsets stay valid).
    let clean = sanitize(src);
    let src: &str = &clean;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter failed on {}", rel.display()))?;
    let root = tree.root_node();
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let in_test = is_test_path(&rel_s);
    let mut ex = Ex { src: src.as_bytes(), out: FileAnalysis::default(), file: rel_s.clone() };
    ex.out.module_doc = file_doc(root, ex.src);
    ex.out.symbols.push(Symbol {
        kind: Kind::Module,
        name: rel_s.rsplit('/').next().unwrap_or(&rel_s).to_string(),
        qualified: rel_s.clone(),
        parent: None,
        trait_impl: None,
        start_line: 1,
        end_line: root.end_position().row as u32 + 1,
        signature: format!("file {rel_s}"),
        params: vec![],
        returns: None,
        summary: ex.out.module_doc.as_deref().and_then(first_sentence),
        doc: ex.out.module_doc.clone(),
        visibility: Some("pub".into()),
        test: in_test,
        decl: false,
        body_hash: hash_hex(src.as_bytes()),
        inferred: false,
    });
    let ctx = Ctx { ns: String::new(), owner: None, access: Some("pub".into()), in_test, template: None };
    ex.items(root, &ctx);
    Ok(ex.out)
}

#[derive(Clone)]
struct Ctx {
    /// Enclosing namespace path ("" = global).
    ns: String,
    /// Qualified name of the enclosing class/struct.
    owner: Option<String>,
    /// Current access level inside a class body (None = private).
    access: Option<String>,
    in_test: bool,
    /// `template<...>` prefix for the next item.
    template: Option<String>,
}

impl Ctx {
    fn qualify(&self, name: &str) -> String {
        match (&self.owner, self.ns.is_empty()) {
            (Some(o), _) => format!("{o}::{name}"),
            (None, true) => name.to_string(),
            (None, false) => format!("{}::{name}", self.ns),
        }
    }
    fn parent(&self) -> Option<String> {
        self.owner.clone().or_else(|| (!self.ns.is_empty()).then(|| self.ns.clone()))
    }
}

/// What a declarator chain resolves to.
struct DeclInfo<'t> {
    name: String,
    /// `*`, `&`, `**` collected from pointer/reference declarators.
    ptr: String,
    params: Option<Node<'t>>,
}

struct Ex<'a> {
    src: &'a [u8],
    out: FileAnalysis,
    file: String,
}

const STD_TYPES: &[&str] = &[
    "string", "string_view", "vector", "map", "unordered_map", "set", "unordered_set", "list", "deque",
    "array", "pair", "tuple", "optional", "variant", "shared_ptr", "unique_ptr", "weak_ptr", "function",
    "size_t", "ssize_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
    "uint64_t", "uintptr_t", "intptr_t", "ptrdiff_t", "FILE", "va_list", "T", "U", "K", "V",
];

const STORAGE_WORDS: &[&str] = &["static", "inline", "extern", "virtual", "explicit", "constexpr", "friend", "__inline", "consteval"];

impl<'a> Ex<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn items(&mut self, n: Node, ctx: &Ctx) {
        let mut cur = n.walk();
        let children: Vec<Node> = n.named_children(&mut cur).collect();
        for c in children {
            self.item(c, ctx);
        }
    }

    fn item(&mut self, n: Node, ctx: &Ctx) {
        match n.kind() {
            "function_definition" => self.function(n, ctx, false),
            "declaration" | "field_declaration" => self.declaration(n, ctx),
            "type_definition" => self.typedef(n, ctx),
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
                self.record(n, ctx, None);
            }
            "alias_declaration" => {
                if let Some(name) = n.child_by_field_name("name") {
                    let nm = self.text(name).to_string();
                    let mut s = self.base(n, ctx, Kind::TypeAlias, &nm);
                    s.signature = self.with_template(ctx, collapse(self.text(n).trim_end_matches(';')));
                    s.returns = n.child_by_field_name("type").map(|t| collapse(self.text(t)));
                    self.out.symbols.push(s);
                }
            }
            "namespace_definition" => {
                let name = n.child_by_field_name("name").map(|x| self.text(x).to_string());
                let ns = match (name, ctx.ns.is_empty()) {
                    (None, _) => ctx.ns.clone(), // anonymous namespace: file-local
                    (Some(nm), true) => nm,
                    (Some(nm), false) => format!("{}::{nm}", ctx.ns),
                };
                if let Some(b) = n.child_by_field_name("body") {
                    let inner = Ctx { ns, owner: None, access: Some("pub".into()), in_test: ctx.in_test, template: None };
                    self.items(b, &inner);
                }
            }
            "linkage_specification" => {
                if let Some(b) = n.child_by_field_name("body") {
                    if b.kind() == "declaration_list" {
                        self.items(b, ctx);
                    } else {
                        self.item(b, ctx);
                    }
                }
            }
            "template_declaration" => {
                let params = n.child_by_field_name("parameters").map(|p| collapse(self.text(p))).unwrap_or_default();
                let mut inner = ctx.clone();
                inner.template = Some(format!("template{params}"));
                let mut cur = n.walk();
                let kids: Vec<Node> = n.named_children(&mut cur).filter(|k| k.kind() != "template_parameter_list").collect();
                for k in kids {
                    self.item(k, &inner);
                }
            }
            "preproc_include" => {
                if let Some(p) = n.child_by_field_name("path") {
                    let path = self.text(p).trim_matches(|c| c == '"' || c == '<' || c == '>').to_string();
                    self.out.edges.push(Edge { from: self.file.clone(), to: path, kind: EdgeKind::Imports, line: line(n), col: 0 });
                }
            }
            "preproc_def" | "preproc_function_def" if ctx.owner.is_none() => {
                let Some(name) = n.child_by_field_name("name") else { return };
                let nm = self.text(name).to_string();
                if nm.ends_with("_H") || nm.ends_with("_H_") || nm.ends_with("_HPP") || nm.ends_with("_INCLUDED") {
                    return; // include guard
                }
                let kind = if n.kind() == "preproc_def" { Kind::Const } else { Kind::Macro };
                let mut s = self.base(n, ctx, kind, &nm);
                s.qualified = nm.clone(); // macros are global
                s.parent = None;
                let params = n.child_by_field_name("parameters").map(|p| self.text(p)).unwrap_or("");
                let value = n.child_by_field_name("value").map(|v| collapse(self.text(v))).unwrap_or_default();
                let value: String = if value.chars().count() > 60 { value.chars().take(57).collect::<String>() + "…" } else { value };
                s.signature = collapse(&format!("#define {nm}{params} {value}"));
                self.out.symbols.push(s);
            }
            "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif" | "preproc_elifdef" => self.items(n, ctx),
            _ => {}
        }
    }

    fn with_template(&self, ctx: &Ctx, sig: String) -> String {
        match &ctx.template {
            Some(t) => format!("{t} {sig}"),
            None => sig,
        }
    }

    fn base(&self, n: Node, ctx: &Ctx, kind: Kind, name: &str) -> Symbol {
        let doc = doc_above(n, self.src);
        Symbol {
            kind,
            name: name.to_string(),
            qualified: ctx.qualify(name),
            parent: ctx.parent(),
            trait_impl: None,
            start_line: line(n),
            // `#define` nodes end at column 0 of the next line
            end_line: if n.end_position().column == 0 && n.end_position().row > n.start_position().row { n.end_position().row as u32 } else { n.end_position().row as u32 + 1 },
            signature: String::new(),
            params: vec![],
            returns: None,
            summary: doc.as_deref().and_then(first_sentence),
            doc,
            visibility: ctx.access.clone(),
            test: ctx.in_test,
            decl: false,
            body_hash: hash_hex(self.text(n).as_bytes()),
            inferred: false,
        }
    }

    /// Walk a declarator chain down to the declared name.
    fn decl_info<'t>(&self, mut d: Node<'t>) -> Option<DeclInfo<'t>> {
        let mut ptr = String::new();
        loop {
            match d.kind() {
                "function_declarator" => {
                    let name = self.declarator_name(d.child_by_field_name("declarator")?)?;
                    return Some(DeclInfo { name, ptr, params: d.child_by_field_name("parameters") });
                }
                "pointer_declarator" => {
                    ptr.push('*');
                    d = d.child_by_field_name("declarator")?;
                }
                "reference_declarator" => {
                    ptr.push('&');
                    d = d.named_child(0)?;
                }
                "init_declarator" | "attributed_declarator" => d = d.child_by_field_name("declarator").or_else(|| d.named_child(0))?,
                "parenthesized_declarator" => d = d.named_child(0)?,
                "array_declarator" => {
                    ptr.push_str("[]");
                    d = d.child_by_field_name("declarator")?;
                }
                _ => {
                    let name = self.declarator_name(d)?;
                    return Some(DeclInfo { name, ptr, params: None });
                }
            }
        }
    }

    fn declarator_name(&self, d: Node) -> Option<String> {
        match d.kind() {
            "identifier" | "field_identifier" | "type_identifier" | "destructor_name" | "operator_name" | "primitive_type" => {
                Some(self.text(d).to_string())
            }
            "qualified_identifier" => Some(strip_templates(&collapse(self.text(d)))),
            "template_function" | "template_type" => d.child_by_field_name("name").map(|x| self.text(x).to_string()),
            "pointer_declarator" | "reference_declarator" | "function_declarator" | "parenthesized_declarator"
            | "init_declarator" | "array_declarator" => self.decl_info(d).map(|i| i.name),
            _ => None,
        }
    }

    fn function(&mut self, n: Node, ctx: &Ctx, decl: bool) {
        let Some(d) = n.child_by_field_name("declarator") else { return };
        let Some(info) = self.decl_info(d) else { return };
        if info.params.is_none() {
            return;
        }
        // Out-of-line definition `void Foo::bar()` → method of Foo.
        let mut c = ctx.clone();
        let mut name = info.name.clone();
        if let Some((scope, last)) = info.name.rsplit_once("::") {
            name = last.to_string();
            let owner = if ctx.ns.is_empty() || scope.starts_with(&ctx.ns) { scope.to_string() } else { format!("{}::{scope}", ctx.ns) };
            c.owner = Some(owner);
            c.access = Some("pub".into());
        }
        let kind = if c.owner.is_some() { Kind::Method } else { Kind::Function };
        let mut sym = self.base(n, &c, kind, &name);
        let body = n.child_by_field_name("body");
        sym.decl = decl || body.is_none();
        // signature ends before the body or the constructor's `: a(x), b(y)` list
        let init_list = {
            let mut cur = n.walk();
            let x = n.children(&mut cur).find(|c| c.kind() == "field_initializer_list").map(|c| c.start_byte());
            x
        };
        let head_end = init_list.or(body.map(|b| b.start_byte())).unwrap_or(n.end_byte());
        let head = std::str::from_utf8(&self.src[n.start_byte()..head_end]).unwrap_or("");
        sym.signature = self.with_template(ctx, collapse(head.trim_end().trim_end_matches(';').trim_end_matches('{')));
        // static free functions are file-local
        let storage = self.storage(n);
        if c.owner.is_none() {
            sym.visibility = (!storage.contains(&"static")).then(|| "pub".to_string());
        }
        if matches!(name.as_str(), "TEST" | "TEST_F" | "TEST_P" | "TEST_CASE") || name.starts_with("test_") {
            sym.test = true;
        }
        // return type = text before the declarator, minus storage words, plus pointer marks
        let ret_raw = std::str::from_utf8(&self.src[n.start_byte()..d.start_byte()]).unwrap_or("");
        let ret: Vec<&str> = ret_raw.split_whitespace().filter(|w| !STORAGE_WORDS.contains(w) && !w.starts_with("__attribute__")).collect();
        let ret = format!("{}{}", ret.join(" "), info.ptr).trim().to_string();
        if !ret.is_empty() && ret != "void" {
            sym.returns = Some(ret);
        }
        if let Some(t) = n.child_by_field_name("type") {
            self.type_uses(&sym.qualified, t);
        }
        if let Some(params) = info.params {
            let mut cur = params.walk();
            for p in params.named_children(&mut cur) {
                if !matches!(p.kind(), "parameter_declaration" | "optional_parameter_declaration" | "variadic_parameter_declaration" | "variadic_parameter") {
                    continue;
                }
                let full = collapse(self.text(p));
                if full == "void" {
                    continue;
                }
                if p.kind() == "variadic_parameter" {
                    sym.params.push(Param { name: "...".into(), ty: String::new() });
                    continue;
                }
                let pname = p.child_by_field_name("declarator").and_then(|x| self.declarator_name(x)).unwrap_or_default();
                let ty = if pname.is_empty() { full.clone() } else { remove_last_word(&full, &pname) };
                if let Some(t) = p.child_by_field_name("type") {
                    self.type_uses(&sym.qualified, t);
                }
                sym.params.push(Param { name: pname, ty });
            }
        }
        if let Some(b) = body {
            // local declarations `PriorityQueue q;`, casts, `new T`
            self.type_uses(&sym.qualified, b);
            let mut calls = Vec::new();
            self.collect_calls(b, &mut calls);
            let locals = crate::local_bindings(n, self.src);
            calls.retain(|(to, _, _)| to.contains("::") || to.starts_with('.') || !locals.contains(to.as_str()));
            // inside a method, a bare `sound()` is `this->sound()` when the class has such a member
            if sym.kind == Kind::Method {
                if let Some(owner) = sym.parent.clone() {
                    for (to, _, _) in calls.iter_mut() {
                        let member = !to.contains("::") && !to.starts_with('.')
                            && self.out.symbols.iter().any(|m| m.kind == Kind::Method && m.parent.as_deref() == Some(owner.as_str()) && &m.name == to);
                        if member {
                            *to = format!("Self::{to}");
                        }
                    }
                }
            }
            for (to, l, col) in calls {
                self.out.edges.push(Edge { from: sym.qualified.clone(), to, kind: EdgeKind::Calls, line: l, col });
            }
        }
        self.out.symbols.push(sym);
    }

    fn storage(&self, n: Node) -> Vec<&'a str> {
        let mut cur = n.walk();
        n.children(&mut cur)
            .filter(|c| matches!(c.kind(), "storage_class_specifier" | "virtual" | "explicit_function_specifier"))
            .map(|c| self.text(c))
            .collect()
    }

    /// `int f(int);` prototypes, `struct X {...} x;`, member declarations, globals.
    fn declaration(&mut self, n: Node, ctx: &Ctx) {
        // A type with a body declared inline: `struct X { ... };`
        if let Some(t) = n.child_by_field_name("type") {
            if t.child_by_field_name("body").is_some() && matches!(t.kind(), "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier") {
                self.record(t, ctx, None);
            }
        }
        let mut cur = n.walk();
        let decls: Vec<Node> = n.children_by_field_name("declarator", &mut cur).collect();
        for d in decls {
            let is_fn = self.decl_info(d).is_some_and(|i| i.params.is_some());
            if is_fn {
                // Function prototype / method declaration: reuse the function path with decl=true.
                self.function_decl(n, d, ctx);
            }
        }
    }

    fn function_decl(&mut self, n: Node, d: Node, ctx: &Ctx) {
        let Some(info) = self.decl_info(d) else { return };
        let mut c = ctx.clone();
        let mut name = info.name.clone();
        if let Some((scope, last)) = info.name.rsplit_once("::") {
            name = last.to_string();
            c.owner = Some(scope.to_string());
        }
        let kind = if c.owner.is_some() { Kind::Method } else { Kind::Function };
        let mut sym = self.base(n, &c, kind, &name);
        sym.decl = true;
        sym.signature = self.with_template(ctx, collapse(self.text(n).trim_end().trim_end_matches(';')));
        if c.owner.is_none() {
            sym.visibility = (!self.storage(n).contains(&"static")).then(|| "pub".to_string());
        }
        let ret_raw = std::str::from_utf8(&self.src[n.start_byte()..d.start_byte()]).unwrap_or("");
        let ret: Vec<&str> = ret_raw.split_whitespace().filter(|w| !STORAGE_WORDS.contains(w)).collect();
        let ret = format!("{}{}", ret.join(" "), info.ptr).trim().to_string();
        if !ret.is_empty() && ret != "void" {
            sym.returns = Some(ret);
        }
        if let Some(params) = info.params {
            let mut cur = params.walk();
            for p in params.named_children(&mut cur) {
                if !matches!(p.kind(), "parameter_declaration" | "optional_parameter_declaration" | "variadic_parameter_declaration") {
                    continue;
                }
                let full = collapse(self.text(p));
                if full == "void" {
                    continue;
                }
                let pname = p.child_by_field_name("declarator").and_then(|x| self.declarator_name(x)).unwrap_or_default();
                let ty = if pname.is_empty() { full.clone() } else { remove_last_word(&full, &pname) };
                if let Some(t) = p.child_by_field_name("type") {
                    self.type_uses(&sym.qualified, t);
                }
                sym.params.push(Param { name: pname, ty });
            }
        }
        if let Some(t) = n.child_by_field_name("type") {
            self.type_uses(&sym.qualified, t);
        }
        self.out.symbols.push(sym);
    }

    fn typedef(&mut self, n: Node, ctx: &Ctx) {
        let alias = {
            let mut cur = n.walk();
            let d = n.children_by_field_name("declarator", &mut cur).next();
            d.and_then(|d| self.declarator_name(d))
        };
        if let Some(t) = n.child_by_field_name("type") {
            if t.child_by_field_name("body").is_some() {
                let own_name = t.child_by_field_name("name").map(|x| self.text(x).to_string());
                self.record(t, ctx, alias.clone());
                if own_name.is_none() || own_name == alias {
                    return; // `typedef struct {..} X;` — X is the record itself
                }
            }
        }
        if let Some(a) = alias {
            let mut s = self.base(n, ctx, Kind::TypeAlias, &a);
            s.signature = collapse(self.text(n).trim_end_matches(';'));
            // `typedef struct buffer { … } buffer_t` → out: `struct buffer`, not the whole body
            s.returns = n.child_by_field_name("type").map(|t| match (t.child_by_field_name("body"), t.child_by_field_name("name")) {
                (Some(_), Some(nm)) => format!("{} {}", t.kind().trim_end_matches("_specifier"), self.text(nm)),
                _ => collapse(self.text(t)),
            });
            self.out.symbols.push(s);
        }
    }

    /// struct / class / union / enum with a body.
    fn record(&mut self, n: Node, ctx: &Ctx, name_override: Option<String>) {
        let Some(body) = n.child_by_field_name("body") else { return };
        let name = n
            .child_by_field_name("name")
            .and_then(|x| self.declarator_name(x))
            .or(name_override);
        let Some(name) = name else { return }; // anonymous
        let kind = match n.kind() {
            "class_specifier" => Kind::Class,
            "union_specifier" => Kind::Union,
            "enum_specifier" => Kind::Enum,
            _ => Kind::Struct,
        };
        let name = name.rsplit("::").next().unwrap_or(&name).to_string();
        let mut sym = self.base(n, ctx, kind, &name);
        sym.signature = self.with_template(ctx, collapse(std::str::from_utf8(&self.src[n.start_byte()..body.start_byte()]).unwrap_or("").trim_end()));
        if sym.visibility.is_none() && ctx.owner.is_none() {
            sym.visibility = Some("pub".into());
        }
        let q = sym.qualified.clone();
        // base classes
        let mut cur = n.walk();
        let bases: Vec<Node> = n.named_children(&mut cur).filter(|c| c.kind() == "base_class_clause").collect();
        for bc in bases {
            let mut c2 = bc.walk();
            for b in bc.named_children(&mut c2) {
                let bn = match b.kind() {
                    "type_identifier" => self.text(b).to_string(),
                    "qualified_identifier" => strip_templates(&collapse(self.text(b))),
                    "template_type" => b.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
                    _ => continue,
                };
                if !bn.is_empty() {
                    self.out.edges.push(Edge { from: q.clone(), to: bn, kind: EdgeKind::Inherits, line: line(b), col: name_pos(b).1 });
                }
            }
        }
        let idx = self.out.symbols.len();
        self.out.symbols.push(sym);

        if kind == Kind::Enum {
            let mut cur = body.walk();
            let vs: Vec<Param> = body
                .named_children(&mut cur)
                .filter(|e| e.kind() == "enumerator")
                .map(|e| Param {
                    name: e.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
                    ty: e.child_by_field_name("value").map(|v| format!("= {}", collapse(self.text(v)))).unwrap_or_default(),
                })
                .collect();
            self.out.symbols[idx].params = vs;
            return;
        }

        let mut inner = Ctx {
            ns: ctx.ns.clone(),
            owner: Some(q.clone()),
            access: if kind == Kind::Class { None } else { Some("pub".into()) },
            in_test: ctx.in_test,
            template: None,
        };
        let mut fields = Vec::new();
        let mut cur = body.walk();
        let members: Vec<Node> = body.named_children(&mut cur).collect();
        for m in members {
            match m.kind() {
                "access_specifier" => {
                    inner.access = match self.text(m).trim_end_matches(':').trim() {
                        "public" => Some("pub".into()),
                        "protected" => Some("protected".into()),
                        _ => None,
                    }
                }
                "field_declaration" | "declaration" => {
                    let mut c3 = m.walk();
                    let decls: Vec<Node> = m.children_by_field_name("declarator", &mut c3).collect();
                    let ty = m.child_by_field_name("type");
                    if let Some(t) = ty {
                        if t.child_by_field_name("body").is_some() {
                            self.record(t, &inner, None);
                        }
                    }
                    for d in decls {
                        match self.decl_info(d) {
                            Some(i) if i.params.is_some() => self.function_decl(m, d, &inner),
                            Some(i) => {
                                let tt = ty.map(|t| collapse(self.text(t))).unwrap_or_default();
                                if let Some(t) = ty {
                                    self.type_uses(&q, t);
                                }
                                fields.push(Param { name: i.name, ty: format!("{tt}{}", i.ptr) });
                            }
                            None => {}
                        }
                    }
                }
                "function_definition" => self.function(m, &inner, false),
                "template_declaration" | "alias_declaration" | "type_definition" | "enum_specifier" | "class_specifier" | "struct_specifier" | "union_specifier" => {
                    self.item(m, &inner)
                }
                _ => {}
            }
        }
        self.out.symbols[idx].params = fields;
    }

    fn type_uses(&mut self, from: &str, ty: Node) {
        let mut names = Vec::new();
        collect_types(ty, self.src, &mut names);
        for (name, l, col) in names {
            if !STD_TYPES.contains(&name.as_str()) && !self.out.edges.iter().any(|e| e.from == from && e.to == name && e.kind == EdgeKind::Uses) {
                self.out.edges.push(Edge { from: from.to_string(), to: name, kind: EdgeKind::Uses, line: l, col });
            }
        }
    }

    fn collect_calls(&self, n: Node, out: &mut Vec<(String, u32, u32)>) {
        match n.kind() {
            "call_expression" => {
                if let Some(f) = n.child_by_field_name("function") {
                    let name = match f.kind() {
                        "identifier" => Some(self.text(f).to_string()),
                        "qualified_identifier" => Some(strip_templates(&collapse(self.text(f)))),
                        "template_function" => f.child_by_field_name("name").map(|x| self.text(x).to_string()),
                        "field_expression" => {
                            let field = f.child_by_field_name("field").map(|x| strip_templates(self.text(x))).unwrap_or_default();
                            let arg = f.child_by_field_name("argument").map(|a| self.text(a)).unwrap_or("");
                            Some(if arg == "this" { format!("Self::{field}") } else { format!(".{field}") })
                        }
                        _ => None,
                    };
                    if let Some(nm) = name.filter(|s| !s.is_empty()) {
                        let (l, c) = name_pos(f);
                        out.push((nm, l, c));
                    }
                }
            }
            "new_expression" => {
                if let Some(t) = n.child_by_field_name("type") {
                    let nm = match t.kind() {
                        "template_type" => t.child_by_field_name("name").map(|x| self.text(x).to_string()),
                        _ => Some(strip_templates(&collapse(self.text(t)))),
                    };
                    if let Some(nm) = nm {
                        let (l, c) = name_pos(t);
                        out.push((nm, l, c));
                    }
                }
            }
            "argument_list" => {
                // function pointers passed as arguments: qsort(a, n, sz, cmp)
                let mut cur = n.walk();
                for a in n.named_children(&mut cur) {
                    if a.kind() == "identifier" {
                        let (l, c) = name_pos(a);
                        out.push((self.text(a).to_string(), l, c));
                    }
                }
            }
            _ => {}
        }
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            self.collect_calls(c, out);
        }
    }
}

const EXPORT_SUFFIXES: &[&str] = &["_API", "_EXPORT", "_EXPORTS", "_LIB", "_PUBLIC", "_DLL", "_EXTERN", "_CLASS", "_VISIBLE", "_IMPORT", "_DECL"];

fn is_export_macro(w: &str) -> bool {
    w.len() > 3
        && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && (EXPORT_SUFFIXES.iter().any(|s| w.ends_with(s)) || w == "EXPORT" || w == "API" || w == "DLLEXPORT")
}

/// Blank export macros: `class FOO_API X`, `FOO_PUBLIC(T) f(...)`, `__declspec(x)`,
/// `__attribute__((x))`. Output has the same byte length as the input.
pub(crate) fn sanitize(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, a: usize, z: usize| {
        for c in &mut out[a..z] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic() || b[i] == b'_') || (i > 0 && ident(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && ident(b[i]) {
            i += 1;
        }
        let word = &src[start..i];
        if word == "__declspec" || word == "__attribute__" {
            // blank through the balanced parentheses
            let mut j = i;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if j < b.len() && b[j] == b'(' {
                let mut depth = 0;
                while j < b.len() {
                    match b[j] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                blank(&mut out, start, j);
                i = j;
            }
            continue;
        }
        if is_export_macro(word) {
            if i < b.len() && b[i] == b'(' {
                // FOO_PUBLIC(cJSON *) → keep the inner type
                if let Some(close) = src[i..].find(')') {
                    blank(&mut out, start, i + 1);
                    out[i + close] = b' ';
                    i += close + 1;
                    continue;
                }
            }
            blank(&mut out, start, i);
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn strip_templates(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// `const char *s` minus `s` → `const char *`.
fn remove_last_word(full: &str, name: &str) -> String {
    match full.rfind(name) {
        Some(i) => {
            let mut t = format!("{}{}", &full[..i], &full[i + name.len()..]);
            t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            t.replace(" *", "*").replace(" &", "&").trim().to_string()
        }
        None => full.to_string(),
    }
}

fn collect_types(n: Node, src: &[u8], out: &mut Vec<(String, u32, u32)>) {
    if n.kind() == "type_identifier" {
        let t = n.utf8_text(src).unwrap_or("").to_string();
        if !out.iter().any(|(x, _, _)| *x == t) {
            out.push((t, line(n), n.start_position().column as u32));
        }
        return;
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        collect_types(c, src, out);
    }
}

fn clean_comment(text: &str) -> String {
    let t = text.trim();
    let body = if let Some(b) = t.strip_prefix("/*") {
        b.strip_suffix("*/").unwrap_or(b).trim_start_matches(['*', '!'])
    } else {
        t
    };
    body.lines()
        .map(|l| {
            let l = l.trim();
            let l = l.trim_start_matches("///").trim_start_matches("//!").trim_start_matches("//").trim_start_matches("*").trim_start_matches("!<").trim_start_matches('<');
            let l = l.trim();
            l.strip_prefix("@brief ").or_else(|| l.strip_prefix("\\brief ")).unwrap_or(l).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Comments directly above a node (no blank line between).
fn doc_above(n: Node, src: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut expect = n.start_position().row;
    let mut sib = n.prev_sibling();
    while let Some(s) = sib {
        if s.kind() != "comment" || s.end_position().row + 1 < expect {
            break;
        }
        parts.push(clean_comment(s.utf8_text(src).unwrap_or("")));
        expect = s.start_position().row;
        sib = s.prev_sibling();
    }
    parts.reverse();
    let d = parts.join("\n").trim().to_string();
    (!d.is_empty()).then_some(d)
}

/// Leading file comment, skipping license headers.
fn file_doc(root: Node, src: &[u8]) -> Option<String> {
    let mut cur = root.walk();
    let mut block: Vec<String> = Vec::new();
    let mut last_row: Option<usize> = None;
    for c in root.children(&mut cur) {
        if c.kind() != "comment" {
            break;
        }
        let text = clean_comment(c.utf8_text(src).unwrap_or(""));
        let contiguous = last_row.is_some_and(|r| c.start_position().row <= r + 1);
        if !contiguous && !block.is_empty() {
            let b = block.join("\n");
            if !is_license(&b) {
                return Some(b);
            }
            block.clear();
        }
        block.push(text);
        last_row = Some(c.end_position().row);
    }
    let b = block.join("\n").trim().to_string();
    (!b.is_empty() && !is_license(&b)).then_some(b)
}

fn is_license(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("copyright") || l.contains("license") || l.contains("spdx")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HDR: &str = r#"
/* Copyright 2024 Someone. MIT License. */

/** Geometry primitives. */
#ifndef GEO_H
#define GEO_H
#include <vector>
#include "util/math.h"

#define MAX_SHAPES 64

namespace geo {

/// Anything with an area.
class Shape {
public:
    virtual ~Shape() = default;
    /// Area in square units.
    virtual double area() const = 0;
    const char *name;
protected:
    int id_;
};

class Circle : public Shape {
public:
    explicit Circle(double r);
    double area() const override;
private:
    double r_;
};

double total_area(const std::vector<Shape*>& shapes);

}  // namespace geo

/* C-style API */
typedef struct { int x, y; } Point;
typedef int (*cmp_fn)(const void *, const void *);
enum Color { RED, GREEN = 2 };
static int helper(int a);
#endif
"#;

    const SRC: &str = r#"
#include "geo.h"

namespace geo {

Circle::Circle(double r) : r_(r) {}

double Circle::area() const { return 3.14 * r_ * r_; }

double total_area(const std::vector<Shape*>& shapes) {
    double t = 0;
    for (auto *s : shapes) t += s->area();
    Shape *c = new Circle(1.0);
    return t + c->area() + helper(1);
}

}

static int helper(int a) { return a + 1; }
"#;

    #[test]
    fn extracts_header() {
        let a = analyze(Path::new("include/geo.h"), HDR).unwrap();
        assert_eq!(a.module_doc.as_deref(), Some("Geometry primitives."));
        let get = |q: &str| a.symbols.iter().find(|s| s.qualified == q).unwrap_or_else(|| panic!("{q} missing: {:?}", a.symbols.iter().map(|s| &s.qualified).collect::<Vec<_>>()));
        let shape = get("geo::Shape");
        assert_eq!(shape.kind, Kind::Class);
        assert_eq!(shape.summary.as_deref(), Some("Anything with an area."));
        assert!(shape.params.iter().any(|p| p.name == "name" && p.ty == "char*"), "{:?}", shape.params);
        let area = get("geo::Shape::area");
        assert!(area.decl && area.kind == Kind::Method);
        assert_eq!(area.visibility.as_deref(), Some("pub"));
        assert_eq!(area.returns.as_deref(), Some("double"));
        assert!(!a.symbols.iter().any(|s| s.qualified == "geo::Shape::id_"), "fields are params, not symbols");
        assert!(get("geo::Shape").params.iter().any(|p| p.name == "id_"));
    }

    #[test]
    fn header_details() {
        let a = analyze(Path::new("include/geo.h"), HDR).unwrap();
        let find = |q: &str| a.symbols.iter().find(|s| s.qualified == q);
        assert!(find("geo::Circle::Circle").is_some(), "constructor declared");
        let total = find("geo::total_area").unwrap();
        assert!(total.decl);
        assert_eq!(total.params[0].name, "shapes");
        assert_eq!(find("Point").unwrap().kind, Kind::Struct);
        assert_eq!(find("Point").unwrap().params.len(), 2);
        assert_eq!(find("cmp_fn").unwrap().kind, Kind::TypeAlias);
        assert_eq!(find("Color").unwrap().params[1].ty, "= 2");
        assert_eq!(find("MAX_SHAPES").unwrap().kind, Kind::Const);
        assert!(find("GEO_H").is_none(), "include guard skipped");
        assert_eq!(find("helper").unwrap().visibility, None, "static = file-local");
        let has = |from: &str, to: &str, k: EdgeKind| a.edges.iter().any(|e| e.from == from && e.to == to && e.kind == k);
        assert!(has("geo::Circle", "Shape", EdgeKind::Inherits));
        assert!(has("include/geo.h", "util/math.h", EdgeKind::Imports));
        assert!(has("include/geo.h", "vector", EdgeKind::Imports));
    }

    #[test]
    fn extracts_source() {
        let a = analyze(Path::new("src/geo.cpp"), SRC).unwrap();
        let find = |q: &str| a.symbols.iter().find(|s| s.qualified == q);
        let area = find("geo::Circle::area").expect("out-of-line method");
        assert!(!area.decl && area.kind == Kind::Method);
        assert_eq!(area.parent.as_deref(), Some("geo::Circle"));
        assert_eq!(find("geo::Circle::Circle").unwrap().signature, "Circle::Circle(double r)");
        let has = |from: &str, to: &str| a.edges.iter().any(|e| e.from == from && e.to == to && e.kind == EdgeKind::Calls);
        assert!(has("geo::total_area", ".area"));
        assert!(has("geo::total_area", "Circle"), "new Circle(..)");
        assert!(has("geo::total_area", "helper"));
        assert_eq!(find("helper").unwrap().params[0].ty, "int");
    }

    #[test]
    fn export_macros() {
        let src = "class TINYXML2_LIB XMLNode : public Base {\npublic:\n  int f();\n};\nCJSON_PUBLIC(cJSON *) cJSON_Parse(const char *value) { return 0; }\nclass __declspec(dllexport) Win {};\n";
        assert_eq!(sanitize(src).len(), src.len());
        let a = analyze(Path::new("x.h"), src).unwrap();
        let find = |q: &str| a.symbols.iter().find(|s| s.qualified == q);
        assert_eq!(find("XMLNode").unwrap().kind, Kind::Class);
        assert!(a.edges.iter().any(|e| e.from == "XMLNode" && e.to == "Base"));
        assert!(find("XMLNode::f").is_some());
        assert_eq!(find("cJSON_Parse").unwrap().returns.as_deref(), Some("cJSON*"));
        assert!(find("Win").is_some());
        assert_eq!(find("cJSON_Parse").unwrap().start_line, 5);
    }

    #[test]
    fn plain_c() {
        let src = "#include <stdio.h>\n// Add two numbers.\nint add(int a, int b) { return a + b; }\nstruct node { int v; struct node *next; };\nint main(void) { printf(\"%d\", add(1, 2)); return 0; }\n";
        let a = analyze(Path::new("main.c"), src).unwrap();
        let add = a.symbols.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(add.summary.as_deref(), Some("Add two numbers."));
        assert_eq!(add.signature, "int add(int a, int b)");
        let node = a.symbols.iter().find(|s| s.name == "node").unwrap();
        assert_eq!(node.params[1].ty, "struct node*");
        assert!(a.edges.iter().any(|e| e.from == "main" && e.to == "add"));
        assert!(a.symbols.iter().find(|s| s.name == "main").unwrap().params.is_empty());
    }
}
