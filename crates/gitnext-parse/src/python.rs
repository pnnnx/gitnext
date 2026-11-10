//! Python extractor: modules, classes (with base classes → `inherits` edges and
//! fields from annotations / `self.x = …` in `__init__`), functions and methods
//! (typed parameters, return annotations, decorators, docstrings), imports, calls.
//!
//! Qualified names use `::` like the other languages: `pkg::module::Class::method`.

use crate::rust::collapse;
use crate::{name_pos, first_sentence, hash_hex, is_test_path, CrateHint, Edge, EdgeKind, FileAnalysis, Kind, Param, Symbol};
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::{Node, Parser};

pub fn analyze(rel: &Path, src: &str, pkg: Option<&CrateHint>) -> anyhow::Result<FileAnalysis> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter failed on {}", rel.display()))?;
    let root = tree.root_node();
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    // Inside a package: module path relative to the dir holding the top package.
    let module = match pkg {
        Some(h) if !h.dir.is_empty() => module_path(rel_s.strip_prefix(&format!("{}/", h.dir)).unwrap_or(&rel_s)),
        _ => module_path(&rel_s),
    };
    let is_pkg = rel_s.ends_with("__init__.py");
    let in_test = is_test_path(&rel_s);

    let mut ex = Ex { src: src.as_bytes(), out: FileAnalysis::default(), imported: HashSet::new(), aliases: std::collections::HashMap::new() };
    ex.out.module_doc = docstring(root, ex.src);
    ex.out.symbols.push(Symbol {
        kind: Kind::Module,
        name: module.rsplit("::").next().unwrap_or(&module).to_string(),
        qualified: module.clone(),
        parent: None,
        trait_impl: None,
        start_line: 1,
        end_line: root.end_position().row as u32 + 1,
        signature: format!("module {}", module.replace("::", ".")),
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
    // Pass 1: imports (names bound by imports decide how `x.f()` calls are read).
    let package = if is_pkg { module.clone() } else { module.rsplit_once("::").map(|(p, _)| p.to_string()).unwrap_or_default() };
    ex.imports(root, &module, &package);
    // Pass 2: definitions.
    let ctx = Ctx { module: module.clone(), owner: None, in_test };
    ex.block(root, &ctx);
    Ok(ex.out)
}

/// `src/pkg/sub/mod.py` → `pkg::sub::mod`; `pkg/__init__.py` → `pkg`.
pub fn module_path(rel: &str) -> String {
    let mut p = rel.strip_suffix(".pyi").or_else(|| rel.strip_suffix(".py")).unwrap_or(rel);
    for prefix in ["src/", "lib/"] {
        if let Some(rest) = p.strip_prefix(prefix) {
            p = rest;
        }
    }
    let mut segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    if segs.last() == Some(&"__init__") && segs.len() > 1 {
        segs.pop();
    }
    segs.iter().map(|s| s.replace(['-', '.'], "_")).collect::<Vec<_>>().join("::")
}

struct Ctx {
    module: String,
    /// Qualified class name when inside a class body.
    owner: Option<String>,
    in_test: bool,
}

struct Ex<'a> {
    src: &'a [u8],
    out: FileAnalysis,
    /// Local names bound by `import x` / `from m import x` at module level.
    imported: HashSet<String>,
    /// local name → module path it stands for (`tt` → `loca::tools::tests`)
    aliases: std::collections::HashMap<String, String>,
}

const BUILTIN_TYPES: &[&str] = &[
    "None", "Any", "Optional", "Union", "List", "Dict", "Set", "Tuple", "Callable", "Iterable",
    "Iterator", "Sequence", "Mapping", "Type", "Self", "Literal", "Generator", "Awaitable",
];

impl<'a> Ex<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn imports(&mut self, root: Node, module: &str, package: &str) {
        // imports anywhere: module level, inside functions (`def test(): from x import y`), in if/try
        let mut children: Vec<Node> = Vec::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let mut cur = n.walk();
            for c in n.named_children(&mut cur) {
                if matches!(c.kind(), "import_statement" | "import_from_statement") {
                    children.push(c);
                } else {
                    stack.push(c);
                }
            }
        }
        children.sort_by_key(|n| n.start_byte());
        for n in children {
            match n.kind() {
                "import_statement" => {
                    let mut c = n.walk();
                    for name in n.children_by_field_name("name", &mut c) {
                        let (path, bound) = self.import_name(name);
                        let target = if name.kind() == "aliased_import" { path.replace('.', "::") } else { bound.clone() };
                        self.aliases.insert(bound.clone(), target);
                        self.imported.insert(bound);
                        self.out.edges.push(Edge { from: module.into(), to: path.replace('.', "::"), kind: EdgeKind::Imports, line: line(n), col: 0 });
                    }
                }
                "import_from_statement" => {
                    let Some(m) = n.child_by_field_name("module_name") else { continue };
                    let base = if m.kind() == "relative_import" {
                        let txt = self.text(m);
                        let dots = txt.chars().take_while(|c| *c == '.').count();
                        let rest = txt.trim_start_matches('.');
                        let mut segs: Vec<&str> = package.split("::").filter(|s| !s.is_empty()).collect();
                        for _ in 1..dots {
                            segs.pop();
                        }
                        let mut b = segs.join("::");
                        if !rest.is_empty() {
                            if !b.is_empty() {
                                b.push_str("::");
                            }
                            b.push_str(&rest.replace('.', "::"));
                        }
                        b
                    } else {
                        self.text(m).replace('.', "::")
                    };
                    let mut c = n.walk();
                    let names: Vec<Node> = n.children_by_field_name("name", &mut c).collect();
                    if names.is_empty() {
                        // `from m import *`
                        self.out.edges.push(Edge { from: module.into(), to: format!("{base}::*"), kind: EdgeKind::Imports, line: line(n), col: 0 });
                    }
                    for name in names {
                        let (path, bound) = self.import_name(name);
                        let to = if base.is_empty() { path.replace('.', "::") } else { format!("{base}::{}", path.replace('.', "::")) };
                        self.aliases.insert(bound.clone(), to.clone());
                        self.imported.insert(bound);
                        self.out.edges.push(Edge { from: module.into(), to, kind: EdgeKind::Imports, line: line(n), col: 0 });
                    }
                }
                _ => {}
            }
        }
    }

    /// (dotted path, local name it binds)
    fn import_name(&self, n: Node) -> (String, String) {
        if n.kind() == "aliased_import" {
            let path = n.child_by_field_name("name").map(|x| self.text(x)).unwrap_or("");
            let alias = n.child_by_field_name("alias").map(|x| self.text(x)).unwrap_or(path);
            (path.to_string(), alias.to_string())
        } else {
            let path = self.text(n).to_string();
            let bound = path.split('.').next().unwrap_or(&path).to_string();
            (path, bound)
        }
    }

    fn block(&mut self, n: Node, ctx: &Ctx) {
        let mut cur = n.walk();
        let children: Vec<Node> = n.named_children(&mut cur).collect();
        for c in children {
            self.stmt(c, ctx, None);
        }
    }

    fn stmt(&mut self, n: Node, ctx: &Ctx, decorators: Option<String>) {
        match n.kind() {
            "decorated_definition" => {
                let mut cur = n.walk();
                let decs: Vec<String> = n
                    .named_children(&mut cur)
                    .filter(|c| c.kind() == "decorator")
                    .map(|c| collapse(self.text(c)))
                    .collect();
                if let Some(def) = n.child_by_field_name("definition") {
                    self.stmt(def, ctx, Some(decs.join(" ")));
                }
                // `@app.route("/x")` calls the decorator factory at import time
                let mut calls = Vec::new();
                let mut cur = n.walk();
                let ds: Vec<Node> = n.named_children(&mut cur).filter(|c| c.kind() == "decorator").collect();
                for d in ds {
                    self.collect_calls(d, &mut calls);
                }
                for (to, l, col) in calls {
                    self.out.edges.push(Edge { from: ctx.module.clone(), to, kind: EdgeKind::Calls, line: l, col });
                }
            }
            "function_definition" => self.function(n, ctx, decorators),
            "class_definition" => self.class(n, ctx, decorators),
            "expression_statement" if ctx.owner.is_none() => {
                self.module_const(n, ctx);
                // module-level code runs at import / as a script: `app = Flask(...)`, `main()`
                let mut calls = Vec::new();
                self.collect_calls(n, &mut calls);
                for (to, l, col) in calls {
                    self.out.edges.push(Edge { from: ctx.module.clone(), to, kind: EdgeKind::Calls, line: l, col });
                }
            }
            // Definitions inside if/try at module level (e.g. `if TYPE_CHECKING:`)
            "if_statement" | "try_statement" | "with_statement" if ctx.owner.is_none() => {
                let mut cur = n.walk();
                let kids: Vec<Node> = n.named_children(&mut cur).collect();
                for k in kids {
                    if k.kind() == "block" {
                        self.block(k, ctx);
                    } else if matches!(k.kind(), "else_clause" | "elif_clause" | "except_clause" | "finally_clause") {
                        let mut c2 = k.walk();
                        let blocks: Vec<Node> = k.named_children(&mut c2).filter(|x| x.kind() == "block").collect();
                        for b in blocks {
                            self.block(b, ctx);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn base(&self, n: Node, ctx: &Ctx, kind: Kind, name: &str, doc: Option<String>) -> Symbol {
        let (qualified, parent) = match &ctx.owner {
            Some(o) => (format!("{o}::{name}"), Some(o.clone())),
            None => (format!("{}::{name}", ctx.module), Some(ctx.module.clone())),
        };
        let private = name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__"));
        Symbol {
            kind,
            name: name.to_string(),
            qualified,
            parent,
            trait_impl: None,
            start_line: line(n),
            end_line: n.end_position().row as u32 + 1,
            signature: String::new(),
            params: vec![],
            returns: None,
            summary: doc.as_deref().and_then(first_sentence),
            doc,
            visibility: (!private).then(|| "pub".to_string()),
            test: ctx.in_test,
            decl: false,
            body_hash: hash_hex(self.text(n).as_bytes()),
            inferred: false,
        }
    }

    fn header(&self, n: Node, body: Option<Node>, decorators: &Option<String>) -> String {
        let end = body.map(|b| b.start_byte()).unwrap_or(n.end_byte());
        let s = std::str::from_utf8(&self.src[n.start_byte()..end]).unwrap_or("");
        let h = collapse(s.trim_end().trim_end_matches(':'));
        match decorators {
            Some(d) if !d.is_empty() => format!("{d} {h}"),
            _ => h,
        }
    }

    fn function(&mut self, n: Node, ctx: &Ctx, decorators: Option<String>) {
        let Some(name_n) = n.child_by_field_name("name") else { return };
        let name = self.text(name_n).to_string();
        let body = n.child_by_field_name("body");
        let doc = body.and_then(|b| docstring(b, self.src));
        let kind = if ctx.owner.is_some() { Kind::Method } else { Kind::Function };
        let mut sym = self.base(n, ctx, kind, &name, doc);
        sym.signature = self.header(n, body, &decorators);
        sym.test = sym.test || name.starts_with("test_") || name == "test" || ctx.owner.as_deref().is_some_and(|o| o.rsplit("::").next().unwrap_or("").starts_with("Test"));
        // abstract / protocol stubs: body is only `...`, `pass` or a docstring
        sym.decl = body.is_some_and(|b| self.is_stub(b)) || decorators.as_deref().is_some_and(|d| d.contains("abstractmethod"));
        if let Some(params) = n.child_by_field_name("parameters") {
            let mut cur = params.walk();
            for p in params.named_children(&mut cur) {
                let (pname, ty) = match p.kind() {
                    "identifier" => (self.text(p).to_string(), String::new()),
                    "typed_parameter" => {
                        let mut c = p.walk();
                        let nm = p.named_children(&mut c).find(|x| x.kind() != "type").map(|x| self.text(x).to_string()).unwrap_or_default();
                        (nm, p.child_by_field_name("type").map(|t| collapse(self.text(t))).unwrap_or_default())
                    }
                    "default_parameter" => (
                        p.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
                        format!("= {}", p.child_by_field_name("value").map(|v| collapse(self.text(v))).unwrap_or_default()),
                    ),
                    "typed_default_parameter" => (
                        p.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
                        format!(
                            "{} = {}",
                            p.child_by_field_name("type").map(|t| collapse(self.text(t))).unwrap_or_default(),
                            p.child_by_field_name("value").map(|v| collapse(self.text(v))).unwrap_or_default()
                        ),
                    ),
                    "list_splat_pattern" | "dictionary_splat_pattern" => (self.text(p).to_string(), String::new()),
                    _ => continue,
                };
                if let Some(t) = p.child_by_field_name("type") {
                    self.type_uses(&sym.qualified, t);
                }
                sym.params.push(Param { name: pname, ty });
            }
        }
        if let Some(rt) = n.child_by_field_name("return_type") {
            let text = collapse(self.text(rt));
            // forward references are strings: `-> "JsonBackend"`
            let unquoted = text.trim_matches(|c| c == '"' || c == '\'').to_string();
            if unquoted != text && unquoted.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && unquoted.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                let (l, col) = name_pos(rt);
                self.out.edges.push(Edge { from: sym.qualified.clone(), to: unquoted.clone(), kind: EdgeKind::Uses, line: l, col });
            }
            sym.returns = Some(unquoted);
            self.type_uses(&sym.qualified, rt);
        }
        if let Some(b) = body {
            let mut calls = Vec::new();
            self.collect_calls(b, &mut calls);
            let locals = crate::local_bindings(n, self.src);
            calls.retain(|(to, _, _)| to.contains("::") || to.starts_with('.') || !locals.contains(to.as_str()));
            for (to, l, col) in calls {
                self.out.edges.push(Edge { from: sym.qualified.clone(), to, kind: EdgeKind::Calls, line: l, col });
            }
        }
        let q = sym.qualified.clone();
        let is_init = name == "__init__";
        self.out.symbols.push(sym);
        // `self.x = …` in __init__ become fields of the class.
        if is_init {
            if let (Some(b), Some(owner)) = (body, ctx.owner.as_ref()) {
                let mut fields = Vec::new();
                self.self_fields(b, &mut fields);
                if let Some(cls) = self.out.symbols.iter_mut().rev().find(|s| &s.qualified == owner) {
                    for f in fields {
                        if !cls.params.iter().any(|p| p.name == f.name) {
                            cls.params.push(f);
                        }
                    }
                }
            }
        }
        let _ = q;
    }

    fn is_stub(&self, body: Node) -> bool {
        let mut cur = body.walk();
        let stub = body.named_children(&mut cur).all(|s| match s.kind() {
            "pass_statement" => true,
            "expression_statement" => {
                let t = self.text(s).trim();
                t == "..." || t.starts_with("\"\"\"") || t.starts_with("'''") || t.starts_with('"') || t.starts_with('\'')
            }
            _ => false,
        });
        stub
    }

    fn self_fields(&self, n: Node, out: &mut Vec<Param>) {
        if n.kind() == "assignment" {
            if let Some(left) = n.child_by_field_name("left") {
                if left.kind() == "attribute" && left.child_by_field_name("object").is_some_and(|o| self.text(o) == "self") {
                    if let Some(attr) = left.child_by_field_name("attribute") {
                        let ty = n.child_by_field_name("type").map(|t| collapse(self.text(t))).unwrap_or_default();
                        out.push(Param { name: self.text(attr).to_string(), ty });
                    }
                }
            }
        }
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            if !matches!(c.kind(), "function_definition" | "class_definition") {
                self.self_fields(c, out);
            }
        }
    }

    fn class(&mut self, n: Node, ctx: &Ctx, decorators: Option<String>) {
        let Some(name_n) = n.child_by_field_name("name") else { return };
        let name = self.text(name_n).to_string();
        let body = n.child_by_field_name("body");
        let doc = body.and_then(|b| docstring(b, self.src));
        let mut sym = self.base(n, ctx, Kind::Class, &name, doc);
        sym.signature = self.header(n, body, &decorators);
        sym.test = sym.test || name.starts_with("Test");
        if let Some(sup) = n.child_by_field_name("superclasses") {
            let mut cur = sup.walk();
            for b in sup.named_children(&mut cur) {
                let base_name = match b.kind() {
                    "identifier" | "attribute" => self.text(b).replace('.', "::"),
                    "subscript" => b.child_by_field_name("value").map(|v| self.text(v).replace('.', "::")).unwrap_or_default(),
                    _ => continue, // keyword args like metaclass=…
                };
                if !base_name.is_empty() && base_name != "object" {
                    self.out.edges.push(Edge { from: sym.qualified.clone(), to: base_name, kind: EdgeKind::Inherits, line: line(b), col: name_pos(b).1 });
                }
            }
        }
        // class-level annotated attributes (dataclasses, pydantic, typing.NamedTuple)
        if let Some(b) = body {
            let mut cur = b.walk();
            for st in b.named_children(&mut cur) {
                if st.kind() != "expression_statement" {
                    continue;
                }
                let Some(a) = st.named_child(0).filter(|a| a.kind() == "assignment") else { continue };
                let Some(left) = a.child_by_field_name("left").filter(|l| l.kind() == "identifier") else { continue };
                let ty = a.child_by_field_name("type").map(|t| collapse(self.text(t))).unwrap_or_default();
                if let Some(t) = a.child_by_field_name("type") {
                    self.type_uses(&sym.qualified, t);
                }
                sym.params.push(Param { name: self.text(left).to_string(), ty });
            }
        }
        let q = sym.qualified.clone();
        let in_test = sym.test;
        self.out.symbols.push(sym);
        if let Some(b) = body {
            let inner = Ctx { module: ctx.module.clone(), owner: Some(q), in_test };
            self.block(b, &inner);
        }
    }

    /// `MAX_SIZE = 10` / `TIMEOUT: float = 1.0` at module level.
    fn module_const(&mut self, n: Node, ctx: &Ctx) {
        let Some(a) = n.named_child(0).filter(|a| a.kind() == "assignment") else { return };
        let Some(left) = a.child_by_field_name("left").filter(|l| l.kind() == "identifier") else { return };
        let name = self.text(left);
        if !(name.len() > 1 && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')) {
            return;
        }
        let mut sym = self.base(n, ctx, Kind::Const, name, comment_above(n, self.src));
        let ty = a.child_by_field_name("type").map(|t| collapse(self.text(t)));
        sym.signature = match &ty {
            Some(t) => format!("{name}: {t}"),
            None => name.to_string(),
        };
        sym.returns = ty;
        self.out.symbols.push(sym);
    }

    fn type_uses(&mut self, from: &str, ty: Node) {
        let mut names = Vec::new();
        collect_idents(ty, self.src, &mut names);
        for (name, l, col) in names {
            let first_upper = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            if first_upper && !BUILTIN_TYPES.contains(&name.as_str())
                && !self.out.edges.iter().any(|e| e.from == from && e.to == name && e.kind == EdgeKind::Uses)
            {
                self.out.edges.push(Edge { from: from.to_string(), to: name, kind: EdgeKind::Uses, line: l, col });
            }
        }
    }

    fn collect_calls(&self, n: Node, out: &mut Vec<(String, u32, u32)>) {
        if n.kind() == "call" {
            if let Some(f) = n.child_by_field_name("function") {
                let name = match f.kind() {
                    "identifier" => Some(self.text(f).to_string()),
                    "attribute" => {
                        let attr = f.child_by_field_name("attribute").map(|a| self.text(a)).unwrap_or("");
                        let obj = f.child_by_field_name("object");
                        match obj.map(|o| (o.kind(), self.text(o))) {
                            Some((_, "self" | "cls")) => Some(format!("Self::{attr}")),
                            // `Class.method()` or `module.func()` for imported / capitalised names
                            // `tt.run()` where `tt` is an imported module: the module's function
                            Some(("identifier", o)) if self.aliases.contains_key(o) && !self.aliases[o].is_empty() => {
                                Some(format!("{}::{attr}", self.aliases[o]))
                            }
                            Some(("identifier", o)) if self.imported.contains(o) || o.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => {
                                Some(format!("{o}::{attr}"))
                            }
                            _ => Some(format!(".{attr}")),
                        }
                    }
                    _ => None,
                };
                if let Some(nm) = name {
                    let (l, c) = name_pos(f);
                    out.push((nm, l, c));
                }
            }
        }
        // `real = tt.detect_test_command`, `tt.detect_test_command = fake`: a module function
        // referenced (or patched) without being called
        if n.kind() == "attribute" {
            let is_callee = n.parent().is_some_and(|p| p.kind() == "call" && p.child_by_field_name("function").map(|f| f.id()) == Some(n.id()));
            if !is_callee {
                if let (Some(o), Some(attr)) = (n.child_by_field_name("object"), n.child_by_field_name("attribute")) {
                    if o.kind() == "identifier" {
                        if let Some(path) = self.aliases.get(self.text(o)).filter(|p| !p.is_empty()) {
                            let (l, c) = name_pos(n);
                            out.push((format!("{path}::{}", self.text(attr)), l, c));
                        }
                    }
                }
            }
        }
        // functions / classes passed as values: `map(parse_line, xs)`, `key=sort_key`
        if n.kind() == "argument_list" {
            let mut cur = n.walk();
            for a in n.named_children(&mut cur) {
                let v = if a.kind() == "keyword_argument" { a.child_by_field_name("value") } else { Some(a) };
                if let Some(v) = v.filter(|v| v.kind() == "identifier") {
                    let (l, c) = name_pos(v);
                    out.push((self.text(v).to_string(), l, c));
                }
            }
        }
        let mut cur = n.walk();
        for c in n.named_children(&mut cur) {
            if !matches!(c.kind(), "function_definition" | "class_definition") {
                self.collect_calls(c, out);
            }
        }
    }
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn collect_idents(n: Node, src: &[u8], out: &mut Vec<(String, u32, u32)>) {
    if n.kind() == "identifier" {
        let t = n.utf8_text(src).unwrap_or("").to_string();
        if !out.iter().any(|(x, _, _)| *x == t) {
            out.push((t, line(n), n.start_position().column as u32));
        }
        return;
    }
    if n.kind() == "string" {
        return;
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        collect_idents(c, src, out);
    }
}

/// First statement of a module/class/function body if it is a string literal.
fn docstring(body: Node, src: &[u8]) -> Option<String> {
    let mut cur = body.walk();
    let first = body.named_children(&mut cur).find(|c| c.kind() != "comment")?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let s = first.named_child(0).filter(|x| x.kind() == "string")?;
    let raw = s.utf8_text(src).ok()?;
    let raw = raw.trim_start_matches(['r', 'R', 'u', 'U', 'b', 'B', 'f', 'F']);
    let inner = ["\"\"\"", "'''", "\"", "'"]
        .iter()
        .find_map(|q| raw.strip_prefix(q).and_then(|r| r.strip_suffix(q)))
        .unwrap_or(raw);
    let doc = dedent(inner);
    (!doc.is_empty()).then_some(doc)
}

/// `# comment` lines directly above a statement.
fn comment_above(n: Node, src: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut expect_row = n.start_position().row;
    let mut sib = n.prev_sibling();
    while let Some(s) = sib {
        if s.kind() != "comment" || s.end_position().row + 1 != expect_row {
            break;
        }
        lines.push(s.utf8_text(src).unwrap_or("").trim_start_matches('#').trim().to_string());
        expect_row = s.start_position().row;
        sib = s.prev_sibling();
    }
    lines.reverse();
    let d = lines.join("\n");
    (!d.is_empty()).then_some(d)
}

/// Like `inspect.cleandoc`: strip common indentation of continuation lines.
fn dedent(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let indent = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            out.push(l.trim().to_string());
        } else {
            out.push(l.get(indent..).unwrap_or(l.trim_start()).trim_end().to_string());
        }
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
"""Shapes and their areas."""
from abc import ABC, abstractmethod
from .util import clamp
import math as m

MAX_SIDES: int = 12

class Shape(ABC):
    """Anything with an area."""
    name: str

    @abstractmethod
    def area(self) -> float:
        """Area in square units."""
        ...

class Circle(Shape):
    """A circle by radius."""
    def __init__(self, r: float = 1.0):
        self.r = clamp(r, 0, 100)

    def area(self) -> float:
        return m.pi * self.r ** 2

    def scaled(self, k: float) -> "Circle":
        return Circle(self.area() * k)

def total(shapes: list[Shape], *, rounded: bool = False) -> float:
    """Sum of areas."""
    return sum(s.area() for s in shapes)

def test_total():
    assert total([Circle()]) > 0
"#;

    #[test]
    fn extracts_python() {
        let a = analyze(Path::new("src/geo/shapes.py"), SRC, None).unwrap();
        assert_eq!(a.module_doc.as_deref(), Some("Shapes and their areas."));
        let get = |q: &str| a.symbols.iter().find(|s| s.qualified == q).unwrap_or_else(|| panic!("{q} missing: {:?}", a.symbols.iter().map(|s| &s.qualified).collect::<Vec<_>>()));
        let circle = get("geo::shapes::Circle");
        assert_eq!(circle.kind, Kind::Class);
        assert!(circle.params.iter().any(|p| p.name == "r"), "self.r field");
        assert_eq!(get("geo::shapes::Shape").params[0].name, "name");
        let area = get("geo::shapes::Shape::area");
        assert!(area.decl, "abstract method is a declaration");
        assert_eq!(area.returns.as_deref(), Some("float"));
        assert!(area.signature.starts_with("@abstractmethod def area"));
        let total = get("geo::shapes::total");
        assert_eq!(total.params[0].ty, "list[Shape]");
        assert_eq!(total.params[1].name, "rounded");
        assert_eq!(total.summary.as_deref(), Some("Sum of areas."));
        assert!(get("geo::shapes::test_total").test);
        assert_eq!(get("geo::shapes::MAX_SIDES").kind, Kind::Const);
        let has = |from: &str, to: &str, k: EdgeKind| a.edges.iter().any(|e| e.from == from && e.to == to && e.kind == k);
        assert!(has("geo::shapes::Circle", "Shape", EdgeKind::Inherits));
        assert!(has("geo::shapes::Shape", "ABC", EdgeKind::Inherits));
        assert!(has("geo::shapes", "geo::util::clamp", EdgeKind::Imports));
        assert!(has("geo::shapes", "math", EdgeKind::Imports));
        assert!(has("geo::shapes::Circle::__init__", "clamp", EdgeKind::Calls));
        assert!(has("geo::shapes::Circle::scaled", "Self::area", EdgeKind::Calls));
        assert!(has("geo::shapes::Circle::scaled", "Circle", EdgeKind::Calls));
        assert!(has("geo::shapes::total", ".area", EdgeKind::Calls));
        assert!(has("geo::shapes::total", "Shape", EdgeKind::Uses));
    }

    #[test]
    fn package_root() {
        let mut read = |p: &str| matches!(p, "py/pkg/__init__.py" | "py/pkg/sub/__init__.py").then(String::new);
        let h = crate::find_crate("py/pkg/sub/m.py", &mut read).unwrap();
        assert_eq!(h.dir, "py");
        let a = analyze(Path::new("py/pkg/sub/m.py"), "def f(): pass", Some(&h)).unwrap();
        assert!(a.symbols.iter().any(|s| s.qualified == "pkg::sub::m::f"));
        assert!(crate::find_crate("scripts/run.py", &mut read).is_none());
    }

    #[test]
    fn module_paths() {
        assert_eq!(module_path("src/pkg/__init__.py"), "pkg");
        assert_eq!(module_path("pkg/sub/mod.py"), "pkg::sub::mod");
        assert_eq!(module_path("main.py"), "main");
    }
}
