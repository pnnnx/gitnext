//! Rust extractor: walks the tree-sitter CST and emits symbols and edges.

use crate::{name_pos, CrateHint, first_sentence, hash_hex, Edge, EdgeKind, FileAnalysis, Kind, Param, Symbol};
use std::collections::HashMap;
use std::path::{Component, Path};
use tree_sitter::{Node, Parser};

pub fn analyze(rel: &Path, src: &str, krate: Option<&CrateHint>) -> anyhow::Result<FileAnalysis> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter failed on {}", rel.display()))?;
    let root = tree.root_node();
    let module = match krate {
        Some(k) => module_path_in(rel, k),
        None => module_path(rel),
    };

    let mut ex = Extractor { src: src.as_bytes(), out: FileAnalysis::default(), vars: HashMap::new() };
    ex.out.module_doc = inner_doc(root, ex.src);

    // File-level module symbol: anchor for `use` edges and the file's purpose.
    let file_mod = Symbol {
        kind: Kind::Module,
        name: module.rsplit("::").next().unwrap_or(&module).to_string(),
        qualified: module.clone(),
        parent: None,
        trait_impl: None,
        start_line: 1,
        end_line: root.end_position().row as u32 + 1,
        signature: format!("mod {}", module),
        params: vec![],
        returns: None,
        summary: ex.out.module_doc.as_deref().and_then(first_sentence),
        doc: ex.out.module_doc.clone(),
        visibility: Some("pub".into()),
        test: crate::is_test_path(&rel.to_string_lossy()),
        decl: false,
        body_hash: hash_hex(src.as_bytes()),
        inferred: false,
    };
    ex.out.symbols.push(file_mod);

    let in_test = crate::is_test_path(&rel.to_string_lossy());
    let ctx = Ctx { module, owner: None, trait_impl: None, in_trait: false, in_test };
    ex.container(root, &ctx);
    Ok(ex.out)
}

/// Derive a Rust module path from a repo-relative file path.
/// `crates/foo-bar/src/a/b.rs` → `foo_bar::a::b`; `src/main.rs` → `crate`.
pub fn module_path(rel: &Path) -> String {
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(str::to_string),
            _ => None,
        })
        .collect();
    let src_idx = parts.iter().rposition(|p| p == "src");
    let (krate, rest): (String, &[String]) = match src_idx {
        Some(0) => ("crate".into(), &parts[1..]),
        Some(i) => (parts[i - 1].replace('-', "_"), &parts[i + 1..]),
        None => ("crate".into(), &parts[..]),
    };
    let mut segs = vec![krate];
    for (i, p) in rest.iter().enumerate() {
        let last = i == rest.len() - 1;
        let p = if last { p.strip_suffix(".rs").unwrap_or(p) } else { p.as_str() };
        if last && (p == "mod" || ((p == "lib" || p == "main") && i == 0)) {
            continue;
        }
        segs.push(p.replace('-', "_"));
    }
    segs.join("::")
}

/// Module path when the owning crate is known (nearest `Cargo.toml`):
/// `<crate name>::<path inside the crate, without src/>`.
pub fn module_path_in(rel: &Path, k: &CrateHint) -> String {
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let inner = if k.dir.is_empty() { rel_s.as_str() } else { rel_s.strip_prefix(&format!("{}/", k.dir)).unwrap_or(&rel_s) };
    let inner = inner.strip_prefix(&format!("{}/", k.src_dir)).unwrap_or(inner);
    let segs: Vec<&str> = inner.split('/').collect();
    let mut out = vec![k.name.replace('-', "_")];
    for (i, p) in segs.iter().enumerate() {
        let last = i == segs.len() - 1;
        let p = if last { p.strip_suffix(".rs").unwrap_or(p) } else { p };
        if last && (p == "mod" || ((p == "lib" || p == "main") && i == 0)) {
            continue;
        }
        out.push(p.replace('-', "_"));
    }
    out.join("::")
}

struct Ctx {
    module: String,
    /// Qualified name of the impl'd type or trait whose body we're in.
    owner: Option<String>,
    trait_impl: Option<String>,
    in_trait: bool,
    in_test: bool,
}

struct Extractor<'a> {
    src: &'a [u8],
    out: FileAnalysis,
    /// local variable → its type, inside the function being read (`req: &VersionReq`,
    /// `let v = Version::parse(s)?`): `v.matches()` then means `Version::matches`
    vars: HashMap<String, String>,
}

impl<'a> Extractor<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn container(&mut self, node: Node, ctx: &Ctx) {
        let mut cur = node.walk();
        let children: Vec<Node> = node.named_children(&mut cur).collect();
        for child in children {
            self.item(child, ctx);
        }
    }

    fn item(&mut self, n: Node, ctx: &Ctx) {
        match n.kind() {
            "function_item" | "function_signature_item" => self.function(n, ctx),
            "struct_item" => self.adt(n, ctx, Kind::Struct),
            "union_item" => self.adt(n, ctx, Kind::Union),
            "enum_item" => self.adt(n, ctx, Kind::Enum),
            "trait_item" => self.trait_item(n, ctx),
            "impl_item" => self.impl_item(n, ctx),
            "mod_item" => self.mod_item(n, ctx),
            "type_item" => self.simple(n, ctx, Kind::TypeAlias),
            "const_item" => self.simple(n, ctx, Kind::Const),
            "static_item" => self.simple(n, ctx, Kind::Static),
            "macro_definition" => self.simple(n, ctx, Kind::Macro),
            "extern_crate_declaration" => {
                if let Some(name) = n.child_by_field_name("name") {
                    let to = self.text(name).to_string();
                    self.edge(&ctx.module, to, EdgeKind::Imports, n);
                }
            }
            "use_declaration" => {
                if let Some(arg) = n.child_by_field_name("argument") {
                    let to = collapse(self.text(arg));
                    self.edge(&ctx.module, to, EdgeKind::Imports, n);
                }
            }
            _ => {}
        }
    }

    fn base(&self, n: Node, ctx: &Ctx, kind: Kind, name: String) -> Symbol {
        let (qualified, parent) = match &ctx.owner {
            Some(o) => (format!("{o}::{name}"), Some(o.clone())),
            None => (format!("{}::{name}", ctx.module), Some(ctx.module.clone())),
        };
        let doc = outer_doc(n, self.src);
        Symbol {
            kind,
            name,
            qualified,
            parent,
            trait_impl: ctx.trait_impl.clone(),
            start_line: n.start_position().row as u32 + 1,
            end_line: n.end_position().row as u32 + 1,
            signature: String::new(),
            params: vec![],
            returns: None,
            summary: doc.as_deref().and_then(first_sentence),
            doc,
            visibility: visibility(n, self.src),
            test: ctx.in_test || has_test_attr(n, self.src),
            decl: false,
            body_hash: hash_hex(self.text(n).as_bytes()),
            inferred: false,
        }
    }

    /// Declaration text up to (not including) the body, whitespace-collapsed.
    fn header(&self, n: Node, body: Option<Node>) -> String {
        let end = body.map(|b| b.start_byte()).unwrap_or(n.end_byte());
        let s = std::str::from_utf8(&self.src[n.start_byte()..end]).unwrap_or("");
        collapse(s.trim_end().trim_end_matches(';').trim_end_matches('{'))
    }

    fn function(&mut self, n: Node, ctx: &Ctx) {
        let Some(name) = n.child_by_field_name("name") else { return };
        let kind = if ctx.owner.is_some() { Kind::Method } else { Kind::Function };
        let mut sym = self.base(n, ctx, kind, self.text(name).to_string());
        let body = n.child_by_field_name("body");
        sym.signature = self.header(n, body);
        sym.decl = body.is_none();
        if ctx.in_trait && sym.visibility.is_none() {
            sym.visibility = Some("pub".into()); // trait items share the trait's visibility
        }
        if let Some(params) = n.child_by_field_name("parameters") {
            let mut cur = params.walk();
            for p in params.named_children(&mut cur) {
                match p.kind() {
                    "parameter" => {
                        let pname = p.child_by_field_name("pattern").map(|x| self.text(x)).unwrap_or("_");
                        let ty = p.child_by_field_name("type");
                        sym.params.push(Param {
                            name: pname.to_string(),
                            ty: ty.map(|t| collapse(self.text(t))).unwrap_or_default(),
                        });
                        if let Some(t) = ty {
                            self.type_uses(&sym.qualified, t);
                        }
                    }
                    "self_parameter" => sym.params.push(Param {
                        name: "self".into(),
                        ty: collapse(self.text(p)),
                    }),
                    _ => {}
                }
            }
        }
        if let Some(rt) = n.child_by_field_name("return_type") {
            sym.returns = Some(collapse(self.text(rt)));
            self.type_uses(&sym.qualified, rt);
        }
        // trait bounds: `fn largest<S: Shape>(…)`, `where T: Into<Id>`
        self.bound_uses(&sym.qualified, n);
        if let Some(b) = body {
            self.vars = sym.params.iter().filter_map(|p| Some((p.name.clone(), repo_type(&p.ty)?))).collect();
            self.let_types(b);
            // types named inside the body (`let x: T`, `T::new()`, `T { .. }`)
            self.type_uses(&sym.qualified, b);
            let mut calls = Vec::new();
            self.collect_calls(b, &mut calls);
            let locals = crate::local_bindings(n, self.src);
            calls.retain(|(to, _, _)| to.contains("::") || to.starts_with('.') || !locals.contains(to.as_str()));
            for (to, line, col) in calls {
                self.out.edges.push(Edge { from: sym.qualified.clone(), to, kind: EdgeKind::Calls, line, col });
            }
            self.vars.clear();
        }
        self.out.symbols.push(sym);
    }

    fn adt(&mut self, n: Node, ctx: &Ctx, kind: Kind) {
        let Some(name) = n.child_by_field_name("name") else { return };
        let mut sym = self.base(n, ctx, kind, self.text(name).to_string());
        let body = n.child_by_field_name("body");
        sym.signature = self.header(n, body);
        if let Some(b) = body {
            let mut cur = b.walk();
            let mut idx = 0;
            for f in b.named_children(&mut cur) {
                match f.kind() {
                    "field_declaration" => {
                        let fname = f.child_by_field_name("name").map(|x| self.text(x)).unwrap_or("");
                        if let Some(t) = f.child_by_field_name("type") {
                            sym.params.push(Param { name: fname.into(), ty: collapse(self.text(t)) });
                            self.type_uses(&sym.qualified, t);
                        }
                    }
                    "enum_variant" => {
                        let vname = f.child_by_field_name("name").map(|x| self.text(x)).unwrap_or("");
                        let ty = f.child_by_field_name("body").map(|x| collapse(self.text(x))).unwrap_or_default();
                        if let Some(vb) = f.child_by_field_name("body") {
                            self.type_uses(&sym.qualified, vb);
                        }
                        sym.params.push(Param { name: vname.into(), ty });
                    }
                    k if k.ends_with("_type") || k == "type_identifier" || k == "primitive_type" => {
                        // tuple struct: `struct Id(u64);`
                        sym.params.push(Param { name: idx.to_string(), ty: collapse(self.text(f)) });
                        self.type_uses(&sym.qualified, f);
                        idx += 1;
                    }
                    _ => {}
                }
            }
        }
        self.out.symbols.push(sym);
    }

    fn trait_item(&mut self, n: Node, ctx: &Ctx) {
        let Some(name) = n.child_by_field_name("name") else { return };
        let mut sym = self.base(n, ctx, Kind::Trait, self.text(name).to_string());
        let body = n.child_by_field_name("body");
        sym.signature = self.header(n, body);
        if let Some(bounds) = n.child_by_field_name("bounds") {
            let mut cur = bounds.walk();
            for b in bounds.named_children(&mut cur) {
                if b.kind() != "lifetime" {
                    let t = type_base_name(b, self.src);
                    if !t.is_empty() {
                        self.edge(&sym.qualified, t, EdgeKind::Inherits, b);
                    }
                }
            }
        }
        let q = sym.qualified.clone();
        self.out.symbols.push(sym);
        if let Some(b) = body {
            let inner = Ctx { module: ctx.module.clone(), owner: Some(q), trait_impl: None, in_trait: true, in_test: ctx.in_test };
            self.container(b, &inner);
        }
    }

    fn impl_item(&mut self, n: Node, ctx: &Ctx) {
        let Some(ty) = n.child_by_field_name("type") else { return };
        let ty_name = type_base_name(ty, self.src);
        // Blanket impls (`impl<M: Matcher> Matcher for &M`) are forwarding boilerplate,
        // not a real type relationship: skip them.
        if let Some(tp) = n.child_by_field_name("type_parameters") {
            let generics = self.text(tp);
            let is_param = generics
                .trim_matches(|c| c == '<' || c == '>')
                .split(',')
                .any(|g| g.trim().split([':', ' ']).next() == Some(ty_name.as_str()));
            if is_param {
                return;
            }
        }
        let owner = format!("{}::{}", ctx.module, ty_name);
        let trait_name = n.child_by_field_name("trait").map(|t| type_base_name(t, self.src));
        if let Some(t) = &trait_name {
            self.edge(&owner, t.clone(), EdgeKind::Implements, n);
        }
        if let Some(b) = n.child_by_field_name("body") {
            let inner = Ctx { module: ctx.module.clone(), owner: Some(owner), trait_impl: trait_name, in_trait: false, in_test: ctx.in_test };
            self.container(b, &inner);
        }
    }

    fn mod_item(&mut self, n: Node, ctx: &Ctx) {
        let Some(body) = n.child_by_field_name("body") else { return }; // `mod x;` → its own file
        let Some(name) = n.child_by_field_name("name") else { return };
        let name = self.text(name).to_string();
        let mut sym = self.base(n, ctx, Kind::Module, name.clone());
        sym.signature = self.header(n, Some(body));
        if name == "tests" || name == "test" {
            sym.test = true;
        }
        let module = sym.qualified.clone();
        let in_test = sym.test;
        self.out.symbols.push(sym);
        let inner = Ctx { module, owner: None, trait_impl: None, in_trait: false, in_test };
        self.container(body, &inner);
    }

    fn simple(&mut self, n: Node, ctx: &Ctx, kind: Kind) {
        let Some(name) = n.child_by_field_name("name") else { return };
        let mut sym = self.base(n, ctx, kind, self.text(name).to_string());
        sym.signature = match kind {
            Kind::Macro => format!("macro_rules! {}", sym.name),
            Kind::Const | Kind::Static => {
                let end = n.child_by_field_name("value").map(|v| v.start_byte());
                let s = std::str::from_utf8(&self.src[n.start_byte()..end.unwrap_or(n.end_byte())]).unwrap_or("");
                collapse(s.trim_end().trim_end_matches('=').trim_end_matches(';'))
            }
            _ => self.header(n, None),
        };
        if let Some(t) = n.child_by_field_name("type") {
            sym.returns = Some(collapse(self.text(t)));
            self.type_uses(&sym.qualified, t);
        }
        // `const EMPTY: Self = Prerelease { identifier: Identifier::empty() }`: the initializer calls
        if matches!(kind, Kind::Const | Kind::Static) {
            if let Some(v) = n.child_by_field_name("value") {
                let mut calls = Vec::new();
                self.collect_calls(v, &mut calls);
                for (to, line, col) in calls {
                    self.out.edges.push(Edge { from: sym.qualified.clone(), to, kind: EdgeKind::Calls, line, col });
                }
            }
        }
        self.out.symbols.push(sym);
    }

    fn edge(&mut self, from: &str, to: String, kind: EdgeKind, at: Node) {
        let (line, col) = name_pos(at);
        self.out.edges.push(Edge { from: from.to_string(), to, kind, line, col });
    }

    /// `Uses` edges for traits named in generic bounds and where clauses (not the generic names themselves).
    fn bound_uses(&mut self, from: &str, n: Node) {
        let mut generics: Vec<String> = Vec::new();
        let mut parts: Vec<Node> = Vec::new();
        if let Some(tp) = n.child_by_field_name("type_parameters") {
            let mut cur = tp.walk();
            for p in tp.named_children(&mut cur) {
                match p.kind() {
                    "type_identifier" => generics.push(self.text(p).to_string()),
                    "constrained_type_parameter" | "type_parameter" => {
                        if let Some(l) = p.child_by_field_name("left").or_else(|| p.child_by_field_name("name")) {
                            generics.push(self.text(l).to_string());
                        }
                        if let Some(b) = p.child_by_field_name("bounds") {
                            parts.push(b);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            if c.kind() == "where_clause" {
                let mut c2 = c.walk();
                for pred in c.named_children(&mut c2) {
                    if let Some(b) = pred.child_by_field_name("bounds") {
                        parts.push(b);
                    }
                }
            }
        }
        for b in parts {
            let mut names = Vec::new();
            collect_type_names(b, self.src, &mut names);
            for (name, line, col) in names {
                if generics.contains(&name) || self.out.edges.iter().any(|e| e.from == from && e.to == name && e.kind == EdgeKind::Uses) {
                    continue;
                }
                self.out.edges.push(Edge { from: from.to_string(), to: name, kind: EdgeKind::Uses, line, col });
            }
        }
    }

    /// Record `Uses` edges for every user-ish type name inside a type expression.
    fn type_uses(&mut self, from: &str, ty: Node) {
        let mut names = Vec::new();
        collect_type_names(ty, self.src, &mut names);
        for (name, line, col) in names {
            if !self.out.edges.iter().any(|e| e.from == from && e.to == name && e.kind == EdgeKind::Uses) {
                self.out.edges.push(Edge { from: from.to_string(), to: name, kind: EdgeKind::Uses, line, col });
            }
        }
    }

    fn collect_calls(&self, n: Node, out: &mut Vec<(String, u32, u32)>) {
        if n.kind() == "token_tree" {
            // Macro bodies (vec![..], println!(..)) are unparsed token streams:
            // recognise `a::b(`, `f(` and `.m(` patterns heuristically.
            self.calls_in_tokens(n, out);
            return;
        }
        if n.kind() == "call_expression" {
            if let Some(f) = n.child_by_field_name("function") {
                if let Some(name) = callee_name(f, self.src, &self.vars) {
                    let (l, c) = name_pos(f);
                    out.push((name, l, c));
                }
            }
        }
        // functions passed as values: `.map(format_line)`, `Some(handler)`
        if n.kind() == "arguments" {
            let mut cur = n.walk();
            for a in n.named_children(&mut cur) {
                if matches!(a.kind(), "identifier" | "scoped_identifier") {
                    let (l, c) = name_pos(a);
                    out.push((collapse(self.text(a)), l, c));
                }
            }
        }
        // Don't descend into nested item definitions (closures are fine).
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            if matches!(c.kind(), "function_item" | "impl_item" | "mod_item" | "trait_item") {
                continue;
            }
            self.collect_calls(c, out);
        }
    }
}

impl Extractor<'_> {
    /// Heuristic call detection inside a macro token tree:
    /// `path::to::f(` → call, `.m(` → method call, `self.m(` → own method.
    fn calls_in_tokens(&self, tt: Node, out: &mut Vec<(String, u32, u32)>) {
        let mut path = String::new();
        let mut pos = (0u32, 0u32);
        let mut method = false;
        let mut self_recv = false;
        let mut recv_ty: Option<String> = None;
        let (mut prev, mut prev2) = (String::new(), String::new());
        let mut cur = tt.walk();
        for c in tt.children(&mut cur) {
            let text = self.text(c);
            match c.kind() {
                "identifier" | "self" | "super" | "crate" => {
                    pos = (c.start_position().row as u32 + 1, c.start_position().column as u32);
                    if prev == "::" && !path.is_empty() {
                        path.push_str("::");
                        path.push_str(text);
                    } else {
                        method = prev == ".";
                        self_recv = method && prev2 == "self";
                        recv_ty = if method { self.vars.get(prev2.as_str()).cloned() } else { None };
                        path = text.to_string();
                    }
                }
                "::" => {}
                "token_tree" => {
                    if !path.is_empty() && text.starts_with('(') {
                        let name = if self_recv {
                            format!("Self::{path}")
                        } else if let Some(t) = recv_ty.as_ref().filter(|_| method) {
                            format!("{t}::{path}")
                        } else if method {
                            format!(".{path}")
                        } else {
                            path.clone()
                        };
                        out.push((name, pos.0, pos.1));
                    }
                    path.clear();
                    self.calls_in_tokens(c, out);
                }
                _ => path.clear(),
            }
            prev2 = std::mem::replace(&mut prev, text.to_string());
        }
    }
}

fn callee_name(f: Node, src: &[u8], vars: &HashMap<String, String>) -> Option<String> {
    let t = |n: Node| n.utf8_text(src).unwrap_or("").to_string();
    match f.kind() {
        "identifier" => Some(t(f)),
        "scoped_identifier" => Some(collapse(&t(f))),
        "field_expression" => {
            let field = f.child_by_field_name("field").map(t)?;
            let value = f.child_by_field_name("value");
            match value.map(|v| v.kind()) {
                Some("self") => Some(format!("Self::{field}")), // self.method() → own type
                Some("identifier") => match value.and_then(|v| vars.get(&t(v))) {
                    Some(ty) => Some(format!("{ty}::{field}")),
                    None => Some(format!(".{field}")),
                },
                _ => Some(format!(".{field}")),
            }
        }
        "generic_function" => f.child_by_field_name("function").and_then(|x| callee_name(x, src, vars)),
        _ => None,
    }
}

impl Extractor<'_> {
    /// `let x: T = …`, `let x = T::new(…)`, `let x = T::parse(s)?`, `let x = T { … }`.
    fn let_types(&mut self, n: Node) {
        if n.kind() == "let_declaration" {
            let name = n.child_by_field_name("pattern").filter(|p| p.kind() == "identifier").map(|p| self.text(p).to_string());
            let ty = n
                .child_by_field_name("type")
                .and_then(|t| repo_type(&collapse(self.text(t))))
                .or_else(|| n.child_by_field_name("value").and_then(|v| self.value_type(v)));
            if let (Some(name), Some(ty)) = (name, ty) {
                self.vars.insert(name, ty);
            }
        }
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            if !matches!(c.kind(), "function_item" | "impl_item" | "mod_item" | "trait_item") {
                self.let_types(c);
            }
        }
    }

    fn value_type(&self, v: Node) -> Option<String> {
        match v.kind() {
            "try_expression" | "reference_expression" | "parenthesized_expression" => v.named_child(0).and_then(|x| self.value_type(x)),
            "struct_expression" => repo_type(&collapse(self.text(v.child_by_field_name("name")?))),
            "call_expression" => {
                let f = v.child_by_field_name("function")?;
                match f.kind() {
                    // `T::parse(s).unwrap()`, `.expect(..)`, `.clone()`
                    "field_expression" => {
                        let m = self.text(f.child_by_field_name("field")?);
                        if matches!(m, "unwrap" | "expect" | "clone" | "to_owned" | "unwrap_or_default") {
                            self.value_type(f.child_by_field_name("value")?)
                        } else {
                            None
                        }
                    }
                    "scoped_identifier" => {
                        let ty = self.text(f.child_by_field_name("path")?);
                        let m = self.text(f.child_by_field_name("name")?);
                        let ctor = matches!(m, "new" | "parse" | "default" | "empty" | "builder" | "create" | "open" | "from_str")
                            || m.starts_with("from")
                            || m.starts_with("with")
                            || m.starts_with("try_from");
                        if ctor { repo_type(ty) } else { None }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// A type worth tracking for method calls: `&mut a::Foo<T>` → `Foo`; not std, not generic params.
fn repo_type(t: &str) -> Option<String> {
    let t = t.trim().trim_start_matches(['&', ' ']).trim_start_matches("mut ").trim_start_matches("dyn ").trim_start_matches("impl ");
    let base = t.split(['<', ' ', '+', '[']).next()?;
    let last = base.rsplit("::").next()?;
    let first = last.chars().next()?;
    let single_letter = last.len() == 1;
    if !first.is_uppercase() || single_letter || last == "Self" || PRIMITIVE_OR_STD.contains(&last) || STD_TRAITS.contains(&last) {
        return None;
    }
    Some(last.to_string())
}

/// `impl Into<String>`, `&dyn Display`: std traits — their methods are not repo code.
const STD_TRAITS: &[&str] = &[
    "Into", "From", "AsRef", "AsMut", "Borrow", "Fn", "FnMut", "FnOnce", "Iterator", "IntoIterator", "Display", "Debug",
    "ToString", "Clone", "Default", "Hash", "Hasher", "Read", "Write", "BufRead", "Error", "Any", "Send", "Sync", "Deref",
    "PartialEq", "Eq", "PartialOrd", "Ord", "Future", "Formatter", "Result",
];

const PRIMITIVE_OR_STD: &[&str] = &[
    "Self", "String", "str", "Vec", "Option", "Box", "Rc", "Arc", "HashMap", "HashSet",
    "BTreeMap", "BTreeSet", "VecDeque", "Path", "PathBuf", "Cow", "RefCell", "Cell", "Mutex", "RwLock",
];

fn collect_type_names(n: Node, src: &[u8], out: &mut Vec<(String, u32, u32)>) {
    if n.kind() == "type_identifier" {
        let name = n.utf8_text(src).unwrap_or("");
        let single_letter = name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase());
        if !single_letter && !PRIMITIVE_OR_STD.contains(&name) && !out.iter().any(|(x, _, _)| x == name) {
            out.push((name.to_string(), n.start_position().row as u32 + 1, n.start_position().column as u32));
        }
        return;
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        collect_type_names(c, src, out);
    }
}

/// `Foo<T>` → `Foo`, `&mut a::Foo` → `Foo`, `dyn Trait + Send` → `Trait`.
fn type_base_name(n: Node, src: &[u8]) -> String {
    match n.kind() {
        "type_identifier" | "identifier" => n.utf8_text(src).unwrap_or("").to_string(),
        "generic_type" | "reference_type" | "pointer_type" => n
            .child_by_field_name("type")
            .map(|t| type_base_name(t, src))
            .unwrap_or_default(),
        "scoped_type_identifier" => n
            .child_by_field_name("name")
            .map(|t| t.utf8_text(src).unwrap_or("").to_string())
            .unwrap_or_default(),
        _ => {
            let mut cur = n.walk();
            let first = n.named_children(&mut cur).find(|c| c.kind() != "lifetime");
            match first {
                Some(c) => type_base_name(c, src),
                None => collapse(n.utf8_text(src).unwrap_or("")),
            }
        }
    }
}

/// `#[test]`, `#[cfg(test)]`, `#[tokio::test]`… directly above an item.
fn has_test_attr(n: Node, src: &[u8]) -> bool {
    let mut sib = n.prev_sibling();
    while let Some(s) = sib {
        match s.kind() {
            "attribute_item" => {
                let t = s.utf8_text(src).unwrap_or("");
                if t == "#[test]" || t.contains("cfg(test)") || t.contains("::test") {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return false,
        }
        sib = s.prev_sibling();
    }
    false
}

fn visibility(n: Node, src: &[u8]) -> Option<String> {
    let mut cur = n.walk();
    let v = n
        .children(&mut cur)
        .find(|c| c.kind() == "visibility_modifier")
        .map(|c| collapse(c.utf8_text(src).unwrap_or("")));
    v
}

/// `///` / `/** */` comments directly above an item (attributes may sit between).
fn outer_doc(n: Node, src: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut sib = n.prev_sibling();
    while let Some(s) = sib {
        let text = s.utf8_text(src).unwrap_or("");
        match s.kind() {
            "attribute_item" => {}
            "line_comment" if text.starts_with("///") && !text.starts_with("////") => {
                lines.push(strip_marker(text, "///"));
            }
            "block_comment" if text.starts_with("/**") && !text.starts_with("/***") => {
                lines.push(strip_block(text));
            }
            _ => break,
        }
        sib = s.prev_sibling();
    }
    lines.reverse();
    let doc = lines.join("\n").trim().to_string();
    (!doc.is_empty()).then_some(doc)
}

/// `//!` comments at the top of a file.
fn inner_doc(root: Node, src: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut cur = root.walk();
    for c in root.children(&mut cur) {
        let text = c.utf8_text(src).unwrap_or("");
        match c.kind() {
            "line_comment" if text.starts_with("//!") => lines.push(strip_marker(text, "//!")),
            "block_comment" if text.starts_with("/*!") => lines.push(strip_block(text)),
            "line_comment" | "block_comment" | "attribute_item" | "inner_attribute_item" => {}
            _ => break,
        }
    }
    let doc = lines.join("\n").trim().to_string();
    (!doc.is_empty()).then_some(doc)
}

fn strip_marker(line: &str, marker: &str) -> String {
    let s = line.trim_end_matches(['\n', '\r']).strip_prefix(marker).unwrap_or(line);
    s.strip_prefix(' ').unwrap_or(s).to_string()
}

fn strip_block(text: &str) -> String {
    let inner = text.get(3..text.len().saturating_sub(2)).unwrap_or("");
    inner
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn collapse(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // multi-line signatures: `f( a, b, )` → `f(a, b)`
    s.replace("( ", "(").replace(", )", ")").replace(",)", ")").replace(" )", ")").replace("< ", "<").replace(", >", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
//! Expression parser.

use crate::ast::{Expr, Node};

/// A token stream over source text.
pub struct Lexer<'a> { src: &'a str, pos: usize }

/// Things that can be evaluated.
pub trait Eval: std::fmt::Debug + Clone {
    /// Evaluate to a number.
    fn eval(&self, env: &Env) -> f64;
}

impl<'a> Lexer<'a> {
    /// Create a lexer. Starts at zero.
    pub fn new(src: &'a str) -> Self { Lexer { src, pos: 0 } }
    pub fn next_token(&mut self) -> Option<Token> { self.peek(); println!("{}", fmt_tok(self.pos)); vec![Lexer::new("x")]; helper(1) }
    fn peek(&self) -> char { 'x' }
}

impl Eval for Number { fn eval(&self, env: &Env) -> f64 { Lexer::new("").next_token(); 1.0 } }

fn helper(x: u32) -> Option<Token> { None }
"#;

    #[test]
    fn extracts_symbols_and_edges() {
        let a = analyze(Path::new("crates/my-calc/src/parser/mod.rs"), SRC, None).unwrap();
        assert_eq!(a.module_doc.as_deref(), Some("Expression parser."));
        let q: Vec<_> = a.symbols.iter().map(|s| s.qualified.as_str()).collect();
        assert!(q.contains(&"my_calc::parser::Lexer"));
        assert!(q.contains(&"my_calc::parser::Lexer::new"));
        assert!(q.contains(&"my_calc::parser::Eval::eval"));
        let new = a.symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new.kind, Kind::Method);
        assert_eq!(new.params[0].ty, "&'a str");
        assert_eq!(new.returns.as_deref(), Some("Self"));
        assert_eq!(new.summary.as_deref(), Some("Create a lexer."));
        assert_eq!(new.signature, "pub fn new(src: &'a str) -> Self");
        let lexer = a.symbols.iter().find(|s| s.name == "Lexer").unwrap();
        assert_eq!(lexer.params.len(), 2);
        let has = |from: &str, to: &str, k: EdgeKind| a.edges.iter().any(|e| e.from.ends_with(from) && e.to == to && e.kind == k);
        assert!(has("Number", "Eval", EdgeKind::Implements));
        assert!(has("::Eval", "Debug", EdgeKind::Inherits));
        assert!(has("::Eval", "Clone", EdgeKind::Inherits));
        assert!(has("Lexer::next_token", "Self::peek", EdgeKind::Calls));
        assert!(has("Lexer::next_token", "helper", EdgeKind::Calls));
        assert!(has("Lexer::next_token", "fmt_tok", EdgeKind::Calls), "call inside println!");
        assert!(has("Lexer::next_token", "Lexer::new", EdgeKind::Calls), "call inside vec!");
        assert!(has("Number::eval", "Lexer::new", EdgeKind::Calls));
        assert!(has("my_calc::parser", "crate::ast::{Expr, Node}", EdgeKind::Imports));
        assert!(has("Lexer::next_token", "Token", EdgeKind::Uses));
        assert!(a.symbols.iter().find(|s| s.qualified.ends_with("Eval::eval")).unwrap().decl);
        let num_eval = a.symbols.iter().find(|s| s.qualified.ends_with("Number::eval")).unwrap();
        assert_eq!(num_eval.trait_impl.as_deref(), Some("Eval"));
    }

    #[test]
    fn cargo_targets() {
        let toml = "[package]\nname = \"ripgrep\"\n[[bin]]\nname = \"rg\"\npath = \"crates/core/main.rs\"\n";
        let mut read = |p: &str| (p == "Cargo.toml").then(|| toml.to_string());
        let h = crate::find_crate("crates/core/flags/hiargs.rs", &mut read).unwrap();
        assert_eq!((h.name.as_str(), h.src_dir.as_str()), ("ripgrep", "crates/core"));
        let h = crate::find_crate("src/lib.rs", &mut read).unwrap();
        assert_eq!(h.src_dir, "src");
    }

    #[test]
    fn test_code_is_marked() {
        let src = "fn real() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { real(); }\n}\n#[test]\nfn top() {}";
        let a = analyze(Path::new("src/lib.rs"), src, None).unwrap();
        let flag = |n: &str| a.symbols.iter().find(|s| s.name == n).unwrap().test;
        assert!(!flag("real"));
        assert!(flag("tests") && flag("t") && flag("top"));
    }

    #[test]
    fn blanket_impls_skipped() {
        let a = analyze(Path::new("src/lib.rs"), "trait T {} impl<M: T> T for &M {} impl T for X {}", None).unwrap();
        let imps: Vec<_> = a.edges.iter().filter(|e| e.kind == EdgeKind::Implements).map(|e| e.from.as_str()).collect();
        assert_eq!(imps, vec!["crate::X"]);
    }

    #[test]
    fn module_paths() {
        assert_eq!(module_path(Path::new("src/main.rs")), "crate");
        assert_eq!(module_path(Path::new("src/a/b.rs")), "crate::a::b");
        assert_eq!(module_path(Path::new("crates/x-y/src/lib.rs")), "x_y");
        assert_eq!(module_path(Path::new("crates/x-y/src/q/mod.rs")), "x_y::q");
        assert_eq!(module_path(Path::new("build.rs")), "crate::build");
        let k = CrateHint { dir: "crates/core".into(), name: "ripgrep".into(), src_dir: "src".into() };
        assert_eq!(module_path_in(Path::new("crates/core/main.rs"), &k), "ripgrep");
        assert_eq!(module_path_in(Path::new("crates/core/flags/hiargs.rs"), &k), "ripgrep::flags::hiargs");
        let k = CrateHint { dir: "".into(), name: "my-app".into(), src_dir: "src".into() };
        assert_eq!(module_path_in(Path::new("src/a/mod.rs"), &k), "my_app::a");
        let k = CrateHint { dir: "".into(), name: "ripgrep".into(), src_dir: "crates/core".into() };
        assert_eq!(module_path_in(Path::new("crates/core/flags/mod.rs"), &k), "ripgrep::flags");
        assert_eq!(collapse("fn f(\n  &self,\n  x: u8,\n) -> u8"), "fn f(&self, x: u8) -> u8");
    }
}
