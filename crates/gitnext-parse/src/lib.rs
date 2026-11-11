//! Language-agnostic code model plus per-language extractors built on tree-sitter.
//!
//! A file is turned into a [`FileAnalysis`]: the symbols it defines (functions,
//! types, classes, traits…) with their signatures, inputs, outputs and docs, and
//! the edges leaving it (calls, imports, implementations, inheritance, type uses).
//!
//! Languages: Rust, Python, C and C++ (hand-written extractors; C is parsed with
//! the C++ grammar) and, through one table-driven engine (`generic`, `specs`),
//! TypeScript, JavaScript, Go and Java.

/// Bump when extractor output changes; the index re-parses everything on mismatch.
pub const PARSER_VERSION: u32 = 29;

mod cfamily;
mod generic;
mod python;
mod rust;
mod specs;

use serde::{Deserialize, Serialize};
use std::path::Path;

/// What kind of code entity a symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Module,
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Union,
    Trait,
    TypeAlias,
    Const,
    Static,
    Macro,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Module => "module",
            Kind::Function => "function",
            Kind::Method => "method",
            Kind::Struct => "struct",
            Kind::Class => "class",
            Kind::Enum => "enum",
            Kind::Union => "union",
            Kind::Trait => "trait",
            Kind::TypeAlias => "type",
            Kind::Const => "const",
            Kind::Static => "static",
            Kind::Macro => "macro",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "module" | "mod" => Kind::Module,
            "function" | "fn" => Kind::Function,
            "method" => Kind::Method,
            "struct" => Kind::Struct,
            "class" => Kind::Class,
            "enum" => Kind::Enum,
            "union" => Kind::Union,
            "trait" => Kind::Trait,
            "type" => Kind::TypeAlias,
            "const" => Kind::Const,
            "static" => Kind::Static,
            "macro" => Kind::Macro,
            _ => return None,
        })
    }

    /// Single-glyph marker used in the TUI and in compact listings.
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Module => "▣",
            Kind::Function | Kind::Method => "ƒ",
            Kind::Struct | Kind::Union => "◆",
            Kind::Class => "◈",
            Kind::Enum => "◇",
            Kind::Trait => "▲",
            Kind::TypeAlias => "τ",
            Kind::Const | Kind::Static => "·",
            Kind::Macro => "!",
        }
    }

    pub fn is_callable(self) -> bool {
        matches!(self, Kind::Function | Kind::Method)
    }

    pub fn is_type(self) -> bool {
        matches!(
            self,
            Kind::Struct | Kind::Class | Kind::Enum | Kind::Union | Kind::Trait | Kind::TypeAlias
        )
    }
}

/// A named input. For functions: a parameter. For structs: a field. For enums: a variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub kind: Kind,
    pub name: String,
    /// Fully qualified path, e.g. `gitnext_parse::rust::parse_file`, `ns::Shape::area`, `pkg::mod::Class`.
    pub qualified: String,
    /// Qualified name of the owning type (for methods) or module.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent: Option<String>,
    /// Trait this method implements (`impl Display for X { fn fmt }` → `Display`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub trait_impl: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    /// Declaration as written, without body, whitespace-collapsed.
    pub signature: String,
    /// Inputs: parameters / fields / variants.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub params: Vec<Param>,
    /// Output type of a function.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub returns: Option<String>,
    /// Doc comment / docstring text (markers stripped).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub doc: Option<String>,
    /// One-line meaning. MVP: first sentence of the doc comment.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub visibility: Option<String>,
    /// Test code (`#[test]`, `mod tests`, `test_*.py`, `tests/`…). Hidden from maps.
    #[serde(skip_serializing_if = "is_false", default)]
    pub test: bool,
    /// Declaration without a body (C/C++ prototype, header method, trait method without default).
    #[serde(skip_serializing_if = "is_false", default)]
    pub decl: bool,
    /// Hash of the full source text of the item; changes when the body changes.
    #[serde(skip_serializing, default)]
    pub body_hash: String,
    /// `summary` was written by an LLM / agent (`gitnext explain`, MCP `annotate`), not the author.
    #[serde(skip_serializing_if = "is_false", default)]
    pub inferred: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Symbol {
    pub fn is_public(&self) -> bool {
        self.visibility.as_deref().is_some_and(|v| v.starts_with("pub"))
            || self.trait_impl.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// Function/method calls another (target is the callee as written).
    Calls,
    /// Module imports a path (`use a::b::C`).
    Imports,
    /// Type implements a trait.
    Implements,
    /// Trait extends a supertrait (Rust) / class extends base (OOP languages).
    Inherits,
    /// Symbol references a type in its signature or fields.
    Uses,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Implements => "implements",
            EdgeKind::Inherits => "inherits",
            EdgeKind::Uses => "uses",
        }
    }
    pub fn parse(s: &str) -> Option<EdgeKind> {
        Some(match s {
            "calls" => EdgeKind::Calls,
            "imports" => EdgeKind::Imports,
            "implements" => EdgeKind::Implements,
            "inherits" => EdgeKind::Inherits,
            "uses" => EdgeKind::Uses,
            _ => return None,
        })
    }
}

/// A directed, *unresolved* reference: `to` is the name as written in source.
/// Resolution to concrete symbols happens in the index (by name, approximate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub line: u32,
    /// 0-based byte column of the referenced name token (for rename / LSP).
    #[serde(default)]
    pub col: u32,
}

/// Position (1-based line, 0-based byte column) of the name token inside a
/// reference expression: `a::b::c` → `c`, `obj.m` → `m`, `f::<T>` → `f`.
/// Names bound inside a function: parameters, `let` / assignments, loop and
/// match variables, closure and lambda parameters, nested functions. A call or
/// argument that names one of these is a local value, not a repo symbol.
pub(crate) fn local_bindings(func: tree_sitter::Node, src: &[u8]) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![func];
    while let Some(n) = stack.pop() {
        let binding: Vec<tree_sitter::Node> = match n.kind() {
            // Rust
            "parameter" | "let_declaration" | "for_expression" | "let_condition" | "match_arm" => n.child_by_field_name("pattern").into_iter().collect(),
            "closure_parameters" => vec![n],
            // Python
            "parameters" | "lambda_parameters" => vec![n],
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => n.child_by_field_name("left").into_iter().collect(),
            "as_pattern" => n.child_by_field_name("alias").into_iter().collect(),
            "named_expression" => n.child_by_field_name("name").into_iter().collect(),
            "function_definition" if n.id() != func.id() && n.child_by_field_name("name").is_some_and(|x| x.kind() == "identifier") => {
                n.child_by_field_name("name").into_iter().collect()
            }
            // C / C++
            "parameter_declaration" | "optional_parameter_declaration" | "for_range_loop" => n.child_by_field_name("declarator").into_iter().collect(),
            "declaration" => {
                let mut c = n.walk();
                n.children_by_field_name("declarator", &mut c).collect()
            }
            _ => vec![],
        };
        for b in binding {
            pattern_idents(b, src, &mut out);
        }
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            stack.push(ch);
        }
    }
    out
}

pub(crate) fn pattern_idents(n: tree_sitter::Node, src: &[u8], out: &mut std::collections::HashSet<String>) {
    match n.kind() {
        "identifier" | "shorthand_field_identifier" | "shorthand_property_identifier_pattern" => {
            if let Ok(t) = n.utf8_text(src) {
                out.insert(t.to_string());
            }
            return;
        }
        // not bindings: paths, attributes, subscripts, types, default values
        "scoped_identifier" | "attribute" | "subscript" | "field_expression" | "type_identifier" | "parameter_list" | "argument_list" => return,
        _ => {}
    }
    let mut c = n.walk();
    for (i, ch) in n.named_children(&mut c).enumerate() {
        let field = n.field_name_for_named_child(i as u32);
        if matches!(field, Some("type" | "value" | "default" | "arguments" | "parameters" | "body")) {
            continue;
        }
        pattern_idents(ch, src, out);
    }
}

pub(crate) fn name_pos(n: tree_sitter::Node) -> (u32, u32) {
    fn last_ident(n: tree_sitter::Node) -> Option<tree_sitter::Node> {
        const IDENT: &[&str] = &["identifier", "field_identifier", "type_identifier", "property_identifier", "namespace_identifier", "destructor_name", "operator_name"];
        if IDENT.contains(&n.kind()) {
            return Some(n);
        }
        // generic / template calls: the name is the function part, not the type args
        if let Some(f) = n.child_by_field_name("function").or_else(|| n.child_by_field_name("name")).filter(|_| matches!(n.kind(), "generic_function" | "template_function" | "template_type")) {
            return last_ident(f);
        }
        for field in ["name", "field", "attribute", "property"] {
            if let Some(c) = n.child_by_field_name(field) {
                if let Some(x) = last_ident(c) {
                    return Some(x);
                }
            }
        }
        let mut cur = n.walk();
        let kids: Vec<tree_sitter::Node> = n.named_children(&mut cur).collect();
        kids.into_iter().rev().filter(|k| !matches!(k.kind(), "type_arguments" | "template_argument_list" | "arguments" | "argument_list")).find_map(last_ident)
    }
    let t = last_ident(n).unwrap_or(n);
    (t.start_position().row as u32 + 1, t.start_position().column as u32)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileAnalysis {
    /// Inner doc comment of the file (`//!` in Rust, module docstring in Python).
    pub module_doc: Option<String>,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    C,
    Cpp,
    TypeScript,
    Tsx,
    JavaScript,
    Go,
    Java,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Go => "go",
            Lang::Java => "java",
        }
    }

    /// Languages whose code can call each other by name (C with C++, TS with JS).
    pub fn family(self) -> &'static str {
        match self {
            Lang::C | Lang::Cpp => "c",
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => "js",
            other => other.as_str(),
        }
    }

    /// Extracted by the table-driven engine (module paths come from files, not crates).
    pub fn is_generic(self) -> bool {
        matches!(self, Lang::TypeScript | Lang::Tsx | Lang::JavaScript | Lang::Go | Lang::Java)
    }

    fn grammar(self) -> tree_sitter::Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::C | Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    /// Fence tag for code blocks.
    pub fn fence(self) -> &'static str {
        self.as_str()
    }

    pub fn is_c_family(self) -> bool {
        matches!(self, Lang::C | Lang::Cpp)
    }
}

/// Language with a semantic parser for this path, if any.
pub fn lang_for(path: &Path) -> Option<Lang> {
    let file = path.file_name()?.to_str()?;
    // bundles and minified files are build output, not code anyone edits
    if file.ends_with(".min.js") || file.ends_with(".bundle.js") || file.ends_with(".min.mjs") {
        return None;
    }
    match path.extension()?.to_str()? {
        "ts" | "mts" | "cts" => Some(Lang::TypeScript),
        "tsx" => Some(Lang::Tsx),
        "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
        "go" => Some(Lang::Go),
        "java" => Some(Lang::Java),
        "rs" => Some(Lang::Rust),
        "py" | "pyi" => Some(Lang::Python),
        "c" | "h" => Some(Lang::C),
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" | "tpp" | "inl" => Some(Lang::Cpp),
        _ => None,
    }
}

/// Language family of a repo-relative path (see [`Lang::family`]).
pub fn family_of(path: &str) -> Option<&'static str> {
    lang_for(Path::new(path)).map(Lang::family)
}

/// Whether a repo-relative path is C or C++ source.
pub fn is_c_family_path(path: &str) -> bool {
    lang_for(Path::new(path)).is_some_and(Lang::is_c_family)
}

/// Heuristic: is this file test code by its path?
pub fn is_test_path(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.starts_with("test/")
        || path.contains("/test/")
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem == "conftest"
        || file.contains(".test.")
        || file.contains(".spec.")
        || path.contains("__tests__/")
        || path.contains("src/test/")
}

/// Human label for any file type (used in listings, even without a parser).
pub fn file_type_label(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" => "python",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "md" => "markdown",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "sh" => "shell",
        _ => "text",
    }
}

/// The package a file belongs to (nearest manifest: Cargo.toml, later package.json…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateHint {
    /// Repo-relative directory of the manifest ("" = repo root).
    pub dir: String,
    /// Package name as other code imports it.
    pub name: String,
    /// Source root inside `dir` ("src" unless `[lib]`/`[[bin]]` `path` says otherwise).
    pub src_dir: String,
}

/// Analyze a file. `rel` is the path relative to the repository root (used to
/// derive module paths). Returns `None` for unsupported languages.
pub fn analyze(rel: &Path, src: &str) -> anyhow::Result<Option<FileAnalysis>> {
    analyze_in(rel, src, None)
}

/// Like [`analyze`], with the owning package known (gives correct module paths
/// for crates without `src/` and for packages whose name differs from their folder).
pub fn analyze_in(rel: &Path, src: &str, krate: Option<&CrateHint>) -> anyhow::Result<Option<FileAnalysis>> {
    match lang_for(rel) {
        Some(Lang::Rust) => rust::analyze(rel, src, krate).map(Some),
        Some(Lang::Python) => python::analyze(rel, src, krate).map(Some),
        Some(Lang::C | Lang::Cpp) => cfamily::analyze(rel, src).map(Some),
        Some(Lang::TypeScript) => generic::analyze(&specs::TYPESCRIPT, rel, src, krate).map(Some),
        Some(Lang::Tsx) => generic::analyze(&specs::TSX, rel, src, krate).map(Some),
        Some(Lang::JavaScript) => generic::analyze(&specs::JAVASCRIPT, rel, src, krate).map(Some),
        Some(Lang::Go) => generic::analyze(&specs::GO, rel, src, krate).map(Some),
        Some(Lang::Java) => generic::analyze(&specs::JAVA, rel, src, krate).map(Some),
        None => Ok(None),
    }
}

/// Find the package owning `rel` by walking up to the nearest `Cargo.toml` with a
/// `[package]` name. `read` loads a repo-relative file (lets callers cache / use git blobs).
pub fn find_crate(rel: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Option<CrateHint> {
    if rel.ends_with(".py") || rel.ends_with(".pyi") {
        return find_python_root(rel, read);
    }
    if rel.ends_with(".go") {
        return find_go_module(rel, read);
    }
    if lang_for(Path::new(rel)).is_some_and(|l| l.family() == "js") {
        return find_js_package(rel, read);
    }
    if lang_for(Path::new(rel)).is_some_and(Lang::is_generic) {
        return None;
    }
    let mut dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    loop {
        let manifest = if dir.is_empty() { "Cargo.toml".to_string() } else { format!("{dir}/Cargo.toml") };
        if let Some(toml) = read(&manifest) {
            if let Some(name) = cargo_package_name(&toml) {
                let src_dir = cargo_src_dir(&toml, rel.strip_prefix(&format!("{dir}/")).unwrap_or(rel));
                return Some(CrateHint { dir: dir.to_string(), name, src_dir });
            }
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
}

/// JS/TS: the nearest `package.json` with a name — imports of that name are this repo's code.
fn find_js_package(rel: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Option<CrateHint> {
    let mut dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    loop {
        let manifest = if dir.is_empty() { "package.json".to_string() } else { format!("{dir}/package.json") };
        if let Some(m) = read(&manifest) {
            // "name": "…" — the first one is the package's own (no JSON parser needed for this)
            let name = m.split("\"name\"").nth(1).and_then(|r| r.split('"').nth(1)).map(str::to_string);
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                return Some(CrateHint { dir: dir.to_string(), name, src_dir: String::new() });
            }
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
}

/// Go: the nearest `go.mod`; `name` is its module path (imports start with it).
fn find_go_module(rel: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Option<CrateHint> {
    let mut dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    loop {
        let manifest = if dir.is_empty() { "go.mod".to_string() } else { format!("{dir}/go.mod") };
        if let Some(m) = read(&manifest) {
            let name = m.lines().find_map(|l| l.trim().strip_prefix("module ")).map(|s| s.trim().trim_matches('"').to_string())?;
            return Some(CrateHint { dir: dir.to_string(), name, src_dir: String::new() });
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
}

/// Python: the directory *containing* the top-level package (walk up while
/// `__init__.py` exists), so `py/pkg/sub/m.py` becomes module `pkg::sub::m`.
fn find_python_root(rel: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Option<CrateHint> {
    let mut dir = rel.rsplit_once('/').map(|(d, _)| d)?;
    let mut top: Option<&str> = None;
    while !dir.is_empty() && read(&format!("{dir}/__init__.py")).is_some() {
        top = Some(dir);
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
    let top = top?;
    let parent = top.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    Some(CrateHint { dir: parent.to_string(), name: String::new(), src_dir: String::new() })
}

/// Source root for `file` (relative to the manifest dir): the directory of a
/// `[lib]`/`[[bin]]` `path = "..."` containing it, else "src".
fn cargo_src_dir(toml: &str, file: &str) -> String {
    let mut in_target = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_target = t == "[lib]" || t == "[[bin]]";
            continue;
        }
        if in_target {
            if let Some(v) = t.strip_prefix("path").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
                let p = v.trim().trim_matches('"');
                if let Some((d, _)) = p.rsplit_once('/') {
                    if file.starts_with(&format!("{d}/")) {
                        return d.to_string();
                    }
                }
            }
        }
    }
    "src".into()
}

/// `name = "..."` inside the `[package]` table of a Cargo.toml.
pub fn cargo_package_name(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(v) = t.strip_prefix("name").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
                return Some(v.trim().trim_matches('"').replace('-', "_"));
            }
        }
    }
    None
}

/// Byte ranges of comments and string literals in a source file — where a name is
/// only text. `None` for files without a parser (all of it is text).
pub fn text_spans(rel: &Path, src: &str) -> Option<Vec<(usize, usize)>> {
    let lang = lang_for(rel)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang.grammar()).ok()?;
    let tree = parser.parse(src, None)?;
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        let k = n.kind();
        if k.contains("comment") || k.contains("string") || k == "char_literal" || k == "character_literal" || k == "rune_literal" || k == "template_literal" {
            out.push((n.start_byte(), n.end_byte()));
            continue;
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            stack.push(ch);
        }
    }
    Some(out)
}

/// Byte ranges of names inside import / export statements (`import { a as b } from …`,
/// `export { a } from …`, Java `import static p.C.m;`): where a rename must follow
/// the name even though no call edge points there.
pub fn import_name_spans(rel: &Path, src: &str) -> Vec<(usize, usize)> {
    let Some(lang) = lang_for(rel) else { return vec![] };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&lang.grammar()).is_err() {
        return vec![];
    }
    let Some(tree) = parser.parse(src, None) else { return vec![] };
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "import_specifier" | "export_specifier" => {
                if let Some(x) = n.child_by_field_name("name") {
                    out.push((x.start_byte(), x.end_byte()));
                }
                continue;
            }
            // Java: the last identifier of `import [static] a.b.C.m;`
            "import_declaration" if lang == Lang::Java => {
                let mut c = n.walk();
                if let Some(p) = n.named_children(&mut c).find(|x| x.kind() == "scoped_identifier") {
                    if let Some(x) = p.child_by_field_name("name") {
                        out.push((x.start_byte(), x.end_byte()));
                    }
                }
                continue;
            }
            _ => {}
        }
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            stack.push(ch);
        }
    }
    out
}

/// Positions (1-based line, 0-based col) of syntax errors in a source file.
/// Used to refuse edits that would break the file. Empty for unknown languages.
pub fn syntax_errors(rel: &Path, src: &str) -> Vec<(u32, u32)> {
    let Some(lang) = lang_for(rel) else { return vec![] };
    let mut parser = tree_sitter::Parser::new();
    let language = lang.grammar();
    if parser.set_language(&language).is_err() {
        return vec![];
    }
    let clean;
    let src = if lang.is_c_family() {
        clean = cfamily::sanitize(src);
        clean.as_str()
    } else {
        src
    };
    let Some(tree) = parser.parse(src, None) else { return vec![(1, 0)] };
    let mut out = Vec::new();
    fn walk(n: tree_sitter::Node, out: &mut Vec<(u32, u32)>) {
        if out.len() >= 20 {
            return;
        }
        if n.is_error() || n.is_missing() {
            out.push((n.start_position().row as u32 + 1, n.start_position().column as u32));
            return;
        }
        if !n.has_error() {
            return;
        }
        let mut cur = n.walk();
        for c in n.children(&mut cur) {
            walk(c, out);
        }
    }
    walk(tree.root_node(), &mut out);
    out
}

/// Stable 64-bit FNV-1a hash as hex (stable across Rust versions, unlike `DefaultHasher`).
pub fn hash_hex(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// First sentence of a doc comment, capped, used as the one-line "meaning".
pub fn first_sentence(doc: &str) -> Option<String> {
    let para = doc
        .split("\n\n")
        .map(str::trim)
        .find(|p| !p.is_empty() && !p.starts_with('#') && !p.starts_with("```"))?;
    let line = para.split_whitespace().collect::<Vec<_>>().join(" ");
    // sentence end = ". " not preceded by a common abbreviation
    const ABBR: &[&str] = &["e.g", "i.e", "etc", "vs", "cf", "approx", "incl", "Mr", "Dr", "No"];
    let mut end = line.len();
    for (i, _) in line.match_indices(". ") {
        let before = &line[..i];
        let word = before.rsplit(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("");
        if !ABBR.contains(&word) && !word.ends_with("e.g") && !word.ends_with("i.e") {
            end = i + 1;
            break;
        }
    }
    let mut s: String = line[..end].to_string();
    if s.chars().count() > 160 {
        s = s.chars().take(157).collect::<String>() + "…";
    }
    Some(s)
}

/// Last path segment of a reference as written: `a::b::C` → `C`, `.next` → `next`.
pub fn last_segment(name: &str) -> &str {
    let n = name.trim_start_matches('.');
    n.rsplit("::").next().unwrap_or(n)
}

/// Qualifier of a reference: `Lexer::new` → `Lexer`, `a::b::c` → `b`, `.next`/`foo` → "".
pub fn qualifier(name: &str) -> &str {
    if name.starts_with('.') {
        return "";
    }
    let mut it = name.rsplit("::");
    it.next();
    it.next().unwrap_or("")
}
