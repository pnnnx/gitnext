//! One extractor for every language described by a [`Spec`].
//!
//! Rust, Python and C/C++ have hand-written extractors (they need language
//! detail: macros, `impl` blocks, headers). Everything else goes through this
//! engine: a language is a table of tree-sitter node kinds plus a few hooks
//! (imports, module path, visibility, tests). The engine walks the tree and
//! produces the same model — symbols with signature, inputs, output and docs;
//! edges for calls, imports, inheritance and type uses — in the same notation
//! the index resolves:
//!
//! - `name` — a plain call; `Self::name` — a method of the enclosing type;
//!   `.name` — a method on a value of unknown type;
//! - `module::name` — through an import (`fs.readFile` with `import * as fs
//!   from "node:fs"` → `node:fs::readFile`), so calls into a library carry the
//!   library's name.
//!
//! Adding a language = a grammar crate + a `Spec` (see `specs`).

use crate::rust::collapse;
use crate::{first_sentence, hash_hex, is_test_path, name_pos, pattern_idents, CrateHint, Edge, EdgeKind, FileAnalysis, Kind, Lang, Param, Symbol};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node, Parser};

/// What a definition node declares.
pub(crate) struct Def<'t> {
    pub kind: Kind,
    pub name: Node<'t>,
    /// Owning type when it is not the enclosing node (a Go method's receiver).
    pub owner: Option<String>,
    /// Node whose `parameters` / `return_type` / `body` describe it (an arrow
    /// function assigned to a variable: the arrow, not the declarator).
    pub func: Option<Node<'t>>,
}

/// Imports of one file: local name → path it stands for, plus the paths themselves.
#[derive(Default)]
pub(crate) struct Imports {
    pub aliases: HashMap<String, String>,
    pub edges: Vec<(String, u32)>,
}

pub(crate) struct Spec {
    pub lang: Lang,
    pub language: fn() -> tree_sitter::Language,
    /// A node → the definition it is, if any.
    pub classify: for<'t> fn(Node<'t>, &[u8], &Ctx) -> Option<Def<'t>>,
    /// Module path of the file (`::`-separated).
    pub module: fn(rel: &str, root: Node, src: &[u8], pkg: Option<&CrateHint>) -> String,
    pub imports: fn(root: Node, src: &[u8], rel: &str, module: &str, pkg: Option<&CrateHint>) -> Imports,
    /// `pub` when visible outside its module.
    pub public: fn(node: Node, name: &str, src: &[u8]) -> bool,
    /// Test code beyond the path rules (`@Test`, `func TestX`).
    pub is_test: fn(node: Node, name: &str, src: &[u8]) -> bool,
    /// Call expressions and the field holding the callee.
    pub calls: &'static [(&'static str, &'static str)],
    /// Member access: (kind, object field, member field).
    pub member: &'static [(&'static str, &'static str, &'static str)],
    /// `new X()`: (kind, type field).
    pub new: &'static [(&'static str, &'static str)],
    /// Receivers that mean "this object".
    pub this: &'static [&'static str],
    /// A bare `f()` inside a class calls a method of that class (Java, C#).
    pub bare_is_self: bool,
    /// Wrappers whose preceding comment documents the definition inside (`export …`).
    pub wrappers: &'static [&'static str],
    /// (kind, edge) — clauses under a type that name its bases.
    pub heritage: &'static [(&'static str, EdgeKind)],
    /// Fields of a type: (kind, field holding the name or a declarator).
    pub fields: &'static [&'static str],
    /// Built-in type names that are not worth a `uses` edge.
    pub builtin_types: &'static [&'static str],
}

pub(crate) struct Ctx {
    pub module: String,
    pub owner: Option<String>,
    pub func: Option<String>,
    pub in_test: bool,
    /// Go: the receiver's name (`s` in `func (s *Store)`) means "this object".
    pub recv: Option<String>,
    /// Local values whose type is known: parameters `c *Command`, `x := &Store{}`,
    /// `Store s = new Store()` — `c.Execute()` is then `Command::Execute`, not a guess.
    pub vars: HashMap<String, String>,
}

pub(crate) fn analyze(spec: &Spec, rel: &Path, src: &str, pkg: Option<&CrateHint>) -> anyhow::Result<FileAnalysis> {
    let mut parser = Parser::new();
    parser.set_language(&(spec.language)())?;
    let tree = parser.parse(src, None).ok_or_else(|| anyhow::anyhow!("tree-sitter failed on {}", rel.display()))?;
    let root = tree.root_node();
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let b = src.as_bytes();
    let module = (spec.module)(&rel_s, root, b, pkg);
    let in_test = is_test_path(&rel_s);
    let imports = (spec.imports)(root, b, &rel_s, &module, pkg);
    let mut ex = Ex { spec, src: b, out: FileAnalysis::default(), aliases: imports.aliases };
    for (to, line) in imports.edges {
        ex.out.edges.push(Edge { from: module.clone(), to, kind: EdgeKind::Imports, line, col: 0 });
    }
    ex.out.module_doc = file_doc(root, b);
    ex.out.symbols.push(Symbol {
        kind: Kind::Module,
        name: module.rsplit("::").next().unwrap_or(&module).to_string(),
        qualified: module.clone(),
        parent: None,
        trait_impl: None,
        start_line: 1,
        end_line: root.end_position().row as u32 + 1,
        signature: format!("module {}", rel_s),
        params: vec![],
        returns: None,
        summary: ex.out.module_doc.as_deref().and_then(first_sentence),
        doc: ex.out.module_doc.clone(),
        visibility: Some("pub".into()),
        test: in_test,
        decl: false,
        body_hash: hash_hex(b),
        inferred: false,
    });
    let ctx = Ctx { module: module.clone(), owner: None, func: None, in_test, recv: None, vars: HashMap::new() };
    let locals = HashSet::new();
    ex.walk(root, &ctx, &locals);
    Ok(ex.out)
}

struct Ex<'a> {
    spec: &'a Spec,
    src: &'a [u8],
    out: FileAnalysis,
    aliases: HashMap<String, String>,
}

impl<'a> Ex<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn walk(&mut self, n: Node, ctx: &Ctx, locals: &HashSet<String>) {
        if let Some(def) = (self.spec.classify)(n, self.src, ctx) {
            self.define(n, def, ctx);
            return;
        }
        self.reference(n, ctx, locals);
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            self.walk(ch, ctx, locals);
        }
    }

    fn define(&mut self, n: Node, def: Def, ctx: &Ctx) {
        let name = self.text(def.name).to_string();
        if name.is_empty() {
            return;
        }
        let func = def.func.unwrap_or(n);
        let owner = def.owner.or_else(|| ctx.owner.clone());
        let kind = if def.kind == Kind::Function && owner.is_some() { Kind::Method } else { def.kind };
        let qualified = format!("{}::{name}", owner.as_deref().unwrap_or(&ctx.module));
        let body = func.child_by_field_name("body");
        let sig_end = body.map(|b| b.start_byte()).unwrap_or(n.end_byte());
        let mut signature: String = collapse(std::str::from_utf8(&self.src[n.start_byte()..sig_end.max(n.start_byte())]).unwrap_or("")).trim_end_matches(['{', '=', ' ']).trim_end_matches("=>").trim().chars().take(240).collect();
        // a type's declaration line, not its members (Go `Store struct { … }`)
        if kind.is_type() {
            if let Some((head, _)) = signature.split_once('{') {
                signature = head.trim().to_string();
            }
        }
        let doc = self.doc_for(n);
        let test = ctx.in_test || (self.spec.is_test)(n, &name, self.src);
        let mut params = Vec::new();
        let mut types: Vec<(String, u32, u32)> = Vec::new();
        let mut returns = None;
        if kind.is_callable() {
            if let Some(ps) = func.child_by_field_name("parameters").or_else(|| func.child_by_field_name("parameter")) {
                params = self.params(ps, &mut types);
            }
            if let Some(r) = ["return_type", "result"].iter().find_map(|f| func.child_by_field_name(f)).or_else(|| if self.spec.lang == Lang::Java { func.child_by_field_name("type") } else { None }) {
                let t = clean_type(self.text(r));
                if !t.is_empty() && t != "void" {
                    self.type_refs(r, &mut types);
                    returns = Some(t);
                }
            }
        } else if kind.is_type() {
            params = self.fields(n, &mut types);
        }
        let vars = if kind.is_callable() { self.var_types(func, &params) } else { HashMap::new() };
        let parent = owner.clone().or_else(|| Some(ctx.module.clone()));
        self.out.symbols.push(Symbol {
            kind,
            name: name.clone(),
            qualified: qualified.clone(),
            parent,
            trait_impl: None,
            start_line: line(n),
            end_line: n.end_position().row as u32 + 1,
            signature,
            params,
            returns,
            summary: doc.as_deref().and_then(first_sentence),
            doc,
            visibility: (self.spec.public)(n, &name, self.src).then(|| "pub".to_string()),
            test,
            decl: kind.is_callable() && body.is_none(),
            body_hash: hash_hex(&self.src[n.start_byte()..n.end_byte()]),
            inferred: false,
        });
        for (t, l, c) in types {
            let to = self.map_alias(&t);
            self.out.edges.push(Edge { from: qualified.clone(), to, kind: EdgeKind::Uses, line: l, col: c });
        }
        if kind.is_type() {
            self.heritage(n, &qualified);
        }
        // inside: a type's members belong to it; a function's body makes calls from it
        let recv = (n.kind() == "method_declaration" && self.spec.lang == Lang::Go)
            .then(|| n.child_by_field_name("receiver")?.named_child(0)?.child_by_field_name("name").map(|x| self.text(x).to_string()))
            .flatten();
        let inner = if kind.is_type() {
            Ctx { module: ctx.module.clone(), owner: Some(qualified.clone()), func: None, in_test: test, recv: None, vars: HashMap::new() }
        } else {
            Ctx { module: ctx.module.clone(), owner: owner.clone(), func: Some(qualified.clone()), in_test: test, recv, vars }
        };
        // a type alias is a type expression: nothing inside it is a definition or a call
        if kind == Kind::TypeAlias {
            return;
        }
        let locals = if kind.is_callable() { self.locals(func) } else { HashSet::new() };
        let mut c = func.walk();
        let kids: Vec<Node> = func.named_children(&mut c).collect();
        for ch in kids {
            if ch.id() == def.name.id() {
                continue;
            }
            self.walk(ch, &inner, &locals);
        }
    }

    fn params(&self, ps: Node, types: &mut Vec<(String, u32, u32)>) -> Vec<Param> {
        let mut out = Vec::new();
        let list: Vec<Node> = if ps.kind() == "identifier" {
            vec![ps]
        } else {
            let mut c = ps.walk();
            ps.named_children(&mut c).filter(|p| !p.kind().contains("comment")).collect()
        };
        for p in list {
            let ty = p.child_by_field_name("type");
            if let Some(t) = ty {
                self.type_refs(t, types);
            }
            let ty_s = ty.map(|t| clean_type(self.text(t))).unwrap_or_default();
            // Go: `a, b int` — several names share one type
            let mut c = p.walk();
            let names: Vec<Node> = p.children_by_field_name("name", &mut c).collect();
            let names = if names.is_empty() { p.child_by_field_name("pattern").into_iter().collect() } else { names };
            if names.is_empty() && p.kind().contains("pattern") {
                // `({ title, onClose })` — the destructured names are the inputs
                let mut set = HashSet::new();
                pattern_idents(p.child_by_field_name("left").unwrap_or(p), self.src, &mut set);
                let mut v: Vec<String> = set.into_iter().collect();
                v.sort();
                out.extend(v.into_iter().map(|name| Param { name, ty: ty_s.clone() }));
                continue;
            }
            if names.is_empty() {
                let t = self.text(p);
                if p.kind() == "identifier" {
                    out.push(Param { name: t.into(), ty: String::new() });
                } else if !ty_s.is_empty() {
                    out.push(Param { name: String::new(), ty: ty_s.clone() });
                }
                continue;
            }
            for nm in names {
                let name = self.text(nm).trim_start_matches("...").to_string();
                if name == "this" {
                    continue;
                }
                out.push(Param { name, ty: ty_s.clone() });
            }
        }
        out
    }

    fn fields(&self, n: Node, types: &mut Vec<(String, u32, u32)>) -> Vec<Param> {
        let mut out = Vec::new();
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            let mut c = x.walk();
            for ch in x.named_children(&mut c) {
                if self.spec.fields.contains(&ch.kind()) {
                    let ty = ch.child_by_field_name("type");
                    if let Some(t) = ty {
                        self.type_refs(t, types);
                    }
                    let ty_s = ty.map(|t| clean_type(self.text(t))).unwrap_or_default();
                    let name = ch
                        .child_by_field_name("name")
                        .or_else(|| ch.child_by_field_name("declarator").and_then(|d| d.child_by_field_name("name").or(Some(d))))
                        .map(|x| self.text(x).to_string())
                        .unwrap_or_default();
                    if !name.is_empty() && out.len() < 40 {
                        out.push(Param { name, ty: ty_s });
                    }
                } else if matches!(ch.kind(), "class_body" | "field_declaration_list" | "struct_type" | "interface_type" | "enum_body" | "body" | "object_type" | "interface_body" | "formal_parameters") {
                    stack.push(ch);
                } else if ch.kind() == "enum_constant" || (ch.kind() == "property_identifier" && x.kind() == "enum_body") {
                    let name = ch.child_by_field_name("name").map(|n| self.text(n)).unwrap_or_else(|| self.text(ch));
                    out.push(Param { name: name.into(), ty: String::new() });
                }
            }
        }
        out
    }

    /// Named types inside a type expression (`Promise<Task[]>` → `Task`).
    fn type_refs(&self, t: Node, out: &mut Vec<(String, u32, u32)>) {
        let mut stack = vec![t];
        while let Some(x) = stack.pop() {
            match x.kind() {
                "type_identifier" | "qualified_type" | "scoped_type_identifier" => {
                    let name = self.text(x).replace('.', "::");
                    if !self.spec.builtin_types.contains(&name.as_str()) && out.len() < 30 {
                        out.push((name, line(x), x.start_position().column as u32));
                    }
                    if x.kind() != "type_identifier" {
                        continue;
                    }
                }
                _ => {}
            }
            let mut c = x.walk();
            for ch in x.named_children(&mut c) {
                stack.push(ch);
            }
        }
    }

    fn heritage(&mut self, n: Node, from: &str) {
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            let mut c = x.walk();
            for ch in x.named_children(&mut c) {
                if let Some(&(_, kind)) = self.spec.heritage.iter().find(|(k, _)| *k == ch.kind()) {
                    let mut names = Vec::new();
                    let mut st = vec![ch];
                    while let Some(y) = st.pop() {
                        if matches!(y.kind(), "type_identifier" | "identifier" | "member_expression" | "scoped_type_identifier" | "nested_type_identifier") {
                            names.push(y);
                            continue;
                        }
                        if y.kind() == "type_arguments" {
                            continue;
                        }
                        let mut c2 = y.walk();
                        for z in y.named_children(&mut c2) {
                            st.push(z);
                        }
                    }
                    for y in names {
                        let to = self.map_alias(&self.text(y).replace('.', "::"));
                        let (l, col) = name_pos(y);
                        self.out.edges.push(Edge { from: from.into(), to, kind, line: l, col });
                    }
                } else if !matches!(ch.kind(), "class_body" | "body" | "interface_body" | "enum_body" | "declaration_list") && ch.id() != n.child_by_field_name("body").map(|b| b.id()).unwrap_or(usize::MAX) {
                    stack.push(ch);
                }
            }
        }
    }

    fn reference(&mut self, n: Node, ctx: &Ctx, locals: &HashSet<String>) {
        let from = ctx.func.clone().unwrap_or_else(|| ctx.module.clone());
        if let Some(&(_, field)) = self.spec.calls.iter().find(|(k, _)| *k == n.kind()) {
            // Java `obj.m()` keeps object and name on the call node itself
            let target = if n.child_by_field_name("object").is_some() && n.kind() == "method_invocation" {
                self.member_target(n.child_by_field_name("object").unwrap(), n.child_by_field_name("name"), ctx, locals)
            } else {
                n.child_by_field_name(field).and_then(|f| self.callee(f, ctx, locals))
            };
            if let Some(to) = target {
                let (l, c) = name_pos(n.child_by_field_name(field).unwrap_or(n));
                self.out.edges.push(Edge { from, to, kind: EdgeKind::Calls, line: l, col: c });
            }
        } else if let Some(&(_, field)) = self.spec.new.iter().find(|(k, _)| *k == n.kind()) {
            if let Some(t) = n.child_by_field_name(field) {
                let name = self.text(t).split('<').next().unwrap_or("").replace('.', "::");
                // <div> is markup; <Button> is a component
                let html = n.kind().starts_with("jsx") && !name.chars().next().is_some_and(char::is_uppercase);
                if !name.is_empty() && !html {
                    let to = self.map_alias(&name);
                    let (l, c) = name_pos(t);
                    self.out.edges.push(Edge { from, to, kind: EdgeKind::Calls, line: l, col: c });
                }
            }
        }
    }

    fn callee(&self, f: Node, ctx: &Ctx, locals: &HashSet<String>) -> Option<String> {
        if let Some(&(_, of, mf)) = self.spec.member.iter().find(|(k, _, _)| *k == f.kind()) {
            return self.member_target(f.child_by_field_name(of)?, f.child_by_field_name(mf), ctx, locals);
        }
        match f.kind() {
            "identifier" => {
                let name = self.text(f);
                // `require("x")` is the import itself, recorded as one
                if locals.contains(name) || (name == "require" && self.spec.lang.family() == "js") {
                    return None;
                }
                if let Some(a) = self.aliases.get(name) {
                    return Some(a.clone());
                }
                if self.spec.bare_is_self && ctx.owner.is_some() {
                    return Some(format!("Self::{name}"));
                }
                Some(name.to_string())
            }
            // generic call `f<T>()`
            "generic_function" | "instantiation_expression" => f.child_by_field_name("function").and_then(|x| self.callee(x, ctx, locals)),
            _ => None,
        }
    }

    fn member_target(&self, obj: Node, member: Option<Node>, ctx: &Ctx, locals: &HashSet<String>) -> Option<String> {
        let m = self.text(member?);
        if m.is_empty() {
            return None;
        }
        let o = self.text(obj);
        if self.spec.this.contains(&o) || self.spec.this.contains(&obj.kind()) || ctx.recv.as_deref() == Some(o) {
            return Some(format!("Self::{m}"));
        }
        if obj.kind() == "identifier" {
            if let Some(t) = ctx.vars.get(o) {
                return Some(format!("{t}::{m}"));
            }
        }
        if obj.kind() == "identifier" || obj.kind() == "package_identifier" {
            if let Some(a) = self.aliases.get(o) {
                if !locals.contains(o) {
                    return Some(format!("{a}::{m}"));
                }
            }
            // `Type.staticMethod()` — a capitalised name that is not a local value
            if o.chars().next().is_some_and(char::is_uppercase) && !locals.contains(o) {
                return Some(format!("{o}::{m}"));
            }
        }
        Some(format!(".{m}"))
    }

    /// An imported name or a path through an imported package (`model::Task` with
    /// `import ".../internal/model"` → that package's path).
    fn map_alias(&self, name: &str) -> String {
        if let Some(a) = self.aliases.get(name) {
            return a.clone();
        }
        if let Some((first, rest)) = name.split_once("::") {
            if let Some(a) = self.aliases.get(first) {
                return format!("{a}::{rest}");
            }
        }
        name.to_string()
    }

    /// Named type of a type expression: `*Command` → `Command`, `List<Task>` → `List`,
    /// `pkg.Type` → through the import. Built-ins and literals give nothing.
    fn base_type(&self, t: &str) -> Option<String> {
        let t = t.trim().trim_start_matches(':').trim();
        let t = t.trim_start_matches(['*', '&', ' ']);
        let t = t.strip_prefix("[]").unwrap_or(t);
        let t = t.split(['<', '[', '(', '|', ' ', '{', ',', '?']).next()?.trim_end_matches("[]");
        let first = t.chars().next()?;
        if t.is_empty() || !(first.is_alphabetic() || first == '_') || self.spec.builtin_types.contains(&t) || matches!(t, "map" | "chan" | "func" | "var" | "const" | "let") {
            return None;
        }
        Some(self.map_alias(&t.replace('.', "::")))
    }

    /// Type of the value an expression makes: `&Command{}`, `new Store()`, `NewCommand()`, `new(T)`.
    fn expr_type(&self, e: Node) -> Option<String> {
        match e.kind() {
            "composite_literal" | "object_creation_expression" => self.base_type(self.text(e.child_by_field_name("type")?)),
            "new_expression" => self.base_type(self.text(e.child_by_field_name("constructor")?)),
            "unary_expression" | "parenthesized_expression" | "await_expression" => self.expr_type(e.child_by_field_name("operand").or_else(|| e.named_child(0))?),
            "call_expression" => {
                let f = e.child_by_field_name("function")?;
                let name = self.text(f.child_by_field_name("field").or_else(|| f.child_by_field_name("property")).unwrap_or(f));
                if name == "new" {
                    return self.base_type(self.text(e.child_by_field_name("arguments")?.named_child(0)?));
                }
                // Go constructors by convention: NewCommand() → Command (keep a package prefix: pkg.NewX → pkg::X)
                let rest = name.strip_prefix("New").filter(|r| r.chars().next().is_some_and(char::is_uppercase))?;
                let pkg = f.child_by_field_name("operand").map(|o| format!("{}.", self.text(o))).unwrap_or_default();
                self.base_type(&format!("{pkg}{rest}"))
            }
            _ => None,
        }
    }

    /// Local names with a known type inside one function.
    fn var_types(&self, func: Node, params: &[Param]) -> HashMap<String, String> {
        let mut v = HashMap::new();
        for p in params {
            if let Some(t) = self.base_type(&p.ty) {
                v.insert(p.name.clone(), t);
            }
        }
        let mut stack = vec![func];
        while let Some(n) = stack.pop() {
            match n.kind() {
                // Go: a, b := X{}, NewY()
                "short_var_declaration" => {
                    if let (Some(l), Some(r)) = (n.child_by_field_name("left"), n.child_by_field_name("right")) {
                        let (mut c1, mut c2) = (l.walk(), r.walk());
                        let names: Vec<Node> = l.named_children(&mut c1).collect();
                        let vals: Vec<Node> = r.named_children(&mut c2).collect();
                        for (nm, val) in names.iter().zip(vals.iter()) {
                            if let Some(t) = self.expr_type(*val) {
                                v.insert(self.text(*nm).to_string(), t);
                            }
                        }
                    }
                }
                // Go `var x T`; TS `let x: T` / `const x = new T()`
                "var_spec" | "variable_declarator" => {
                    let t = n.child_by_field_name("type").and_then(|t| self.base_type(self.text(t))).or_else(|| n.child_by_field_name("value").and_then(|x| self.expr_type(x)));
                    if let (Some(t), Some(nm)) = (t, n.child_by_field_name("name")) {
                        if nm.kind() == "identifier" {
                            v.insert(self.text(nm).to_string(), t);
                        }
                    }
                }
                // Java `Store s = …;` / `var s = new Store();`
                "local_variable_declaration" => {
                    let declared = n.child_by_field_name("type").map(|t| self.text(t)).unwrap_or("");
                    let mut c = n.walk();
                    for d in n.children_by_field_name("declarator", &mut c) {
                        let t = if declared == "var" { d.child_by_field_name("value").and_then(|x| self.expr_type(x)) } else { self.base_type(declared) };
                        if let (Some(t), Some(nm)) = (t, d.child_by_field_name("name")) {
                            v.insert(self.text(nm).to_string(), t);
                        }
                    }
                }
                _ => {}
            }
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                stack.push(ch);
            }
        }
        v
    }

    /// Names bound inside a function: parameters, variables, loop variables, lambda parameters.
    fn locals(&self, func: Node) -> HashSet<String> {
        let mut out = HashSet::new();
        let mut stack = vec![func];
        while let Some(n) = stack.pop() {
            let binds: Vec<Node> = match n.kind() {
                "required_parameter" | "optional_parameter" => n.child_by_field_name("pattern").into_iter().collect(),
                "formal_parameter" | "parameter_declaration" | "variadic_parameter_declaration" | "spread_parameter" | "catch_formal_parameter" => n.child_by_field_name("name").into_iter().collect(),
                "variable_declarator" | "var_spec" | "const_spec" => n.child_by_field_name("name").into_iter().collect(),
                "short_var_declaration" | "range_clause" | "for_in_statement" => n.child_by_field_name("left").into_iter().collect(),
                "arrow_function" => n.child_by_field_name("parameter").into_iter().collect(),
                "lambda_expression" => n.child_by_field_name("parameters").into_iter().collect(),
                // JS: plain and destructured parameters
                "formal_parameters" => {
                    let mut c = n.walk();
                    n.named_children(&mut c).filter(|p| matches!(p.kind(), "identifier" | "object_pattern" | "array_pattern" | "assignment_pattern" | "rest_pattern")).collect()
                }
                "enhanced_for_statement" => n.child_by_field_name("name").into_iter().collect(),
                _ => vec![],
            };
            for b in binds {
                pattern_idents(b, self.src, &mut out);
            }
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                stack.push(ch);
            }
        }
        out
    }

    /// Comment block right above the definition (or above its `export` wrapper).
    fn doc_for(&self, n: Node) -> Option<String> {
        let mut anchor = n;
        while let Some(p) = anchor.parent() {
            if self.spec.wrappers.contains(&p.kind()) {
                anchor = p;
            } else {
                break;
            }
        }
        let mut lines: Vec<String> = Vec::new();
        let mut cur = anchor.prev_named_sibling();
        let mut next_row = anchor.start_position().row;
        while let Some(c) = cur {
            if !c.kind().contains("comment") || c.end_position().row + 1 < next_row {
                break;
            }
            lines.insert(0, strip_comment(self.text(c)));
            next_row = c.start_position().row;
            cur = c.prev_named_sibling();
        }
        let d = lines.join("\n").trim().to_string();
        (!d.is_empty()).then_some(d)
    }
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

/// `: Promise<void>` → `Promise<void>`.
fn clean_type(t: &str) -> String {
    collapse(t.trim().trim_start_matches(':').trim())
}

fn strip_comment(c: &str) -> String {
    let c = c.trim();
    let c = c.strip_prefix("/**").or_else(|| c.strip_prefix("/*")).map(|x| x.strip_suffix("*/").unwrap_or(x)).unwrap_or(c);
    c.lines()
        .map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("///").or_else(|| l.strip_prefix("//")).or_else(|| l.strip_prefix('*')).unwrap_or(l);
            l.trim().to_string()
        })
        .filter(|l| !l.starts_with('@'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A leading comment before any code (a Go package comment, a file header).
fn file_doc(root: Node, src: &[u8]) -> Option<String> {
    let first = root.named_child(0)?;
    if !first.kind().contains("comment") {
        return None;
    }
    let t = strip_comment(first.utf8_text(src).ok()?);
    // licence headers are not a description
    (!t.is_empty() && !t.to_lowercase().contains("copyright") && !t.to_lowercase().contains("license")).then_some(t)
}

/// `a/b/c.ts` relative import resolved against the importing file → `a::b::c`.
pub(crate) fn join_relative(rel_file: &str, spec: &str) -> String {
    let mut parts: Vec<&str> = rel_file.split('/').collect();
    parts.pop();
    for seg in spec.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    path_module(&parts.join("/"))
}

/// File path → module path: extension and `/index` dropped, `/` → `::`.
pub(crate) fn path_module(p: &str) -> String {
    let mut p = p;
    for ext in [".d.ts", ".tsx", ".ts", ".mts", ".cts", ".jsx", ".js", ".mjs", ".cjs"] {
        if let Some(x) = p.strip_suffix(ext) {
            p = x;
            break;
        }
    }
    let p = p.strip_suffix("/index").unwrap_or(p);
    p.split('/').filter(|s| !s.is_empty()).map(|s| s.replace(['-', '.', '@'], "_")).collect::<Vec<_>>().join("::")
}
