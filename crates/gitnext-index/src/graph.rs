//! In-memory cross-reference graph: resolves name-based edges to symbols and
//! answers callers / callees / hierarchy / dependency questions.
//!
//! Resolution is name-based (like ctags, not like a compiler), so results are
//! *approximate*: method calls on unknown receivers may match several methods.

use crate::{dir_of, FileRow, SymbolRow};
use gitnext_parse::{is_c_family_path, last_segment, qualifier, Edge, EdgeKind, Kind};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

pub struct Graph {
    pub files: Vec<FileRow>,
    pub syms: Vec<SymbolRow>,
    pub edges: Vec<(String, Edge)>,
    by_name: HashMap<String, Vec<usize>>,
    by_qual: HashMap<String, Vec<usize>>,
    crate_roots: HashSet<String>,
    /// crate root → other repo crates it references (via `use` or qualified paths)
    visible: HashMap<String, HashSet<String>>,
    dir_graph: OnceLock<HashMap<String, BTreeMap<String, usize>>>,
    /// target symbol → (referring symbol, edge kind, line); built lazily from all edges.
    rev: OnceLock<(RevIndex, Guesses)>,
    short: OnceLock<Vec<String>>,
    /// Inferred purposes of directories (`dir:<path>` notes).
    pub dir_notes: HashMap<String, String>,
    /// Language-server answers: (file, line, col) → (target file, target line); "" = external.
    pub links: HashMap<(String, u32, u32), (String, u32)>,
}

/// target symbol → (referring symbol, edge kind, line)
type RevIndex = HashMap<usize, Vec<(usize, EdgeKind, u32)>>;
/// (source, target) pairs known only through an ambiguous reference
/// (`x.run()` with an unknown receiver matching several `run` methods).
type Guesses = HashSet<(usize, usize)>;

/// One node of an impact (reverse dependency) walk.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactNode {
    pub depth: usize,
    pub symbol: SymRef,
    /// Qualified name of the symbol it reaches (one level closer to the root).
    pub via: String,
    /// How it depends on `via`: calls / uses / implements / inherits.
    pub how: &'static str,
    pub test: bool,
    /// false: reached through an ambiguous call (receiver type unknown)
    pub certain: bool,
}

/// Compact reference to a symbol, for listings and JSON output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymRef {
    pub qualified: String,
    /// Shortest unique suffix of `qualified` (accepted by every lookup).
    pub short: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Hierarchy {
    pub symbol: Option<SymRef>,
    /// Traits this type implements (Rust `impl Trait for X`).
    pub implements: Vec<String>,
    /// Base classes / supertraits.
    pub extends: Vec<String>,
    /// Types implementing this trait.
    pub implementors: Vec<SymRef>,
    /// Subclasses / traits extending this one.
    pub subtypes: Vec<SymRef>,
    /// Methods grouped by `impl` block ("" = inherent impl).
    pub methods: BTreeMap<String, Vec<SymRef>>,
}

/// Outgoing calls of a function, split by how confidently they resolved.
#[derive(Debug, Default)]
pub struct Callees {
    /// Exactly one candidate in the repo.
    pub resolved: Vec<(usize, u32)>,
    /// Several candidates (e.g. `.as_str()` on an unknown receiver).
    pub ambiguous: Vec<(String, Vec<usize>)>,
    /// Not defined in the repo (std, external crates, closures…).
    pub external: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Counts {
    pub code_files: usize,
    /// functions, methods, types, constants (tests and file modules excluded)
    pub symbols: usize,
    pub tests: usize,
    pub languages: BTreeMap<String, usize>,
}

impl Counts {
    /// `2 code files (python) · 5 functions and types · 0 tests`
    pub fn line(&self) -> String {
        let langs: Vec<String> = self.languages.iter().map(|(l, n)| if self.languages.len() == 1 { l.clone() } else { format!("{l} {n}") }).collect();
        format!(
            "{} code file{}{} · {} functions and types · {} test{}",
            self.code_files,
            if self.code_files == 1 { "" } else { "s" },
            if langs.is_empty() { String::new() } else { format!(" ({})", langs.join(", ")) },
            self.symbols,
            self.tests,
            if self.tests == 1 { "" } else { "s" }
        )
    }
}

#[derive(Debug, Default, Serialize)]
pub struct DirDeps {
    pub dir: String,
    pub depends_on: Vec<(String, usize)>,
    pub used_by: Vec<(String, usize)>,
}

impl Graph {
    pub fn new(files: Vec<FileRow>, mut syms: Vec<SymbolRow>, edges: Vec<(String, Edge)>) -> Self {
        // C/C++: docs usually sit on the header prototype; let the definition inherit them.
        let mut decl_docs: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
        for s in syms.iter().filter(|s| s.sym.decl && s.sym.doc.is_some()) {
            decl_docs.entry(s.sym.qualified.clone()).or_insert((s.sym.doc.clone(), s.sym.summary.clone()));
        }
        for s in syms.iter_mut().filter(|s| !s.sym.decl && s.sym.doc.is_none()) {
            if let Some((d, sum)) = decl_docs.get(&s.sym.qualified) {
                s.sym.doc = d.clone();
                s.sym.summary = sum.clone();
            }
        }
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_qual: HashMap<String, Vec<usize>> = HashMap::new();
        let mut crate_roots = HashSet::new();
        for (i, s) in syms.iter().enumerate() {
            by_name.entry(s.sym.name.clone()).or_default().push(i);
            by_qual.entry(s.sym.qualified.clone()).or_default().push(i);
            if !is_c_family_path(&s.file) {
                if let Some(root) = s.sym.qualified.split("::").next() {
                    crate_roots.insert(root.to_string());
                }
            }
        }
        let mut visible: HashMap<String, HashSet<String>> = HashMap::new();
        for (_, e) in &edges {
            let from_root = e.from.split("::").next().unwrap_or("").to_string();
            let targets: Vec<String> = match e.kind {
                EdgeKind::Imports => flatten_use(&e.to),
                EdgeKind::Calls | EdgeKind::Uses if e.to.contains("::") => vec![e.to.clone()],
                _ => continue,
            };
            for t in targets {
                let root = t.split("::").next().unwrap_or("");
                if root != from_root && crate_roots.contains(root) {
                    visible.entry(from_root.clone()).or_default().insert(root.to_string());
                }
            }
        }
        // Re-exports (`pub use other_crate as x`) make visibility transitive.
        loop {
            let mut grew = false;
            let snapshot = visible.clone();
            for (from, seen) in visible.iter_mut() {
                let extra: Vec<String> = seen
                    .iter()
                    .filter_map(|c| snapshot.get(c))
                    .flatten()
                    .filter(|c| *c != from && !seen.contains(*c))
                    .cloned()
                    .collect();
                if !extra.is_empty() {
                    grew = true;
                    seen.extend(extra);
                }
            }
            if !grew {
                break;
            }
        }
        Graph { files, syms, edges, by_name, by_qual, crate_roots, visible, dir_graph: OnceLock::new(), rev: OnceLock::new(), short: OnceLock::new(), dir_notes: HashMap::new(), links: HashMap::new() }
    }

    pub fn sym_ref(&self, i: usize) -> SymRef {
        let s = &self.syms[i];
        SymRef {
            qualified: s.sym.qualified.clone(),
            short: self.short_name(i).to_string(),
            kind: s.sym.kind.as_str().to_string(),
            file: s.file.clone(),
            line: s.sym.start_line,
            signature: s.sym.signature.clone(),
            summary: s.sym.summary.clone(),
        }
    }

    /// Fill missing summaries from notes whose body hash still matches.
    pub fn apply_notes(&mut self, notes: &crate::notes::Notes) {
        for s in &mut self.syms {
            if s.sym.summary.is_some() {
                continue;
            }
            if let Some(n) = notes.get(&s.sym.qualified) {
                if n.hash == s.sym.body_hash {
                    s.sym.summary = Some(n.summary.clone());
                    s.sym.inferred = true;
                }
            }
        }
        for (k, n) in notes {
            if let Some(d) = k.strip_prefix("dir:") {
                if n.hash == self.dir_hash(d) {
                    self.dir_notes.insert(d.to_string(), n.summary.clone());
                }
            }
        }
    }

    /// Fingerprint of a directory's API (changes when files or signatures change).
    pub fn dir_hash(&self, dir: &str) -> String {
        let mut s = String::new();
        for r in &self.syms {
            if dir_of(&r.file) == dir && !r.sym.test {
                s.push_str(&r.file);
                s.push_str(&r.sym.signature);
            }
        }
        gitnext_parse::hash_hex(s.as_bytes())
    }

    /// Shortest `::`-suffix of the qualified name that identifies this symbol
    /// uniquely (overloads share a name). Saves tokens in every listing.
    pub fn short_name(&self, i: usize) -> &str {
        let all = self.short.get_or_init(|| {
            let mut count: HashMap<&str, usize> = HashMap::new();
            let uniq: HashSet<&str> = self.syms.iter().map(|s| s.sym.qualified.as_str()).collect();
            for q in &uniq {
                for (idx, _) in q.match_indices("::") {
                    *count.entry(&q[idx + 2..]).or_default() += 1;
                }
                *count.entry(q).or_default() += 1;
            }
            self.syms
                .iter()
                .map(|s| {
                    let q = s.sym.qualified.as_str();
                    let mut cands: Vec<&str> = q.match_indices("::").map(|(idx, _)| &q[idx + 2..]).collect();
                    cands.reverse(); // shortest first
                    cands.into_iter().find(|c| count.get(c) == Some(&1)).unwrap_or(q).to_string()
                })
                .collect()
        });
        &all[i]
    }

    /// Find symbols by exact qualified name, `::`-suffix, or bare name.
    pub fn find(&self, q: &str) -> Vec<usize> {
        let q = q.trim();
        // `name@123` / `name:L123` pins an overload / duplicate by its start line.
        if let Some((name, line)) = q.rsplit_once('@').or_else(|| q.rsplit_once(":L")) {
            if let Ok(l) = line.trim_start_matches('L').parse::<u32>() {
                let name = name.replace('.', "::");
                let suffix = format!("::{name}");
                return (0..self.syms.len())
                    .filter(|&i| {
                        let s = &self.syms[i].sym;
                        s.start_line == l && (s.qualified == name || s.qualified.ends_with(&suffix) || s.name == name)
                    })
                    .collect();
            }
        }
        // Accept Python-style dotted names: `Session.request` → `Session::request`.
        let owned;
        let q = if !q.contains("::") && q.contains('.') && !q.contains('/') {
            owned = q.replace('.', "::");
            owned.as_str()
        } else {
            q
        };
        if let Some(v) = self.by_qual.get(q) {
            return drop_decls(&self.syms, v.clone());
        }
        let suffix = format!("::{q}");
        let mut v: Vec<usize> = (0..self.syms.len())
            .filter(|&i| self.syms[i].sym.qualified.ends_with(&suffix))
            .collect();
        if v.is_empty() {
            if let Some(n) = self.by_name.get(q) {
                v = n.clone();
            }
        }
        // Prefer real definitions: not file modules, not prototypes, not tests.
        v.sort_by_key(|&i| {
            let s = &self.syms[i].sym;
            (s.kind == Kind::Module, s.decl, s.test, self.syms[i].file.clone())
        });
        drop_decls(&self.syms, v)
    }

    /// Case-insensitive substring search over names (for fuzzy pickers).
    pub fn search(&self, q: &str, limit: usize) -> Vec<usize> {
        let q = q.to_lowercase();
        let mut hits: Vec<(usize, usize)> = self
            .syms
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let name = s.sym.name.to_lowercase();
                let qual = s.sym.qualified.to_lowercase();
                let score = if name == q {
                    0
                } else if name.starts_with(&q) {
                    1
                } else if name.contains(&q) {
                    2
                } else if qual.contains(&q) {
                    3
                } else if s.sym.doc.as_deref().is_some_and(|d| d.to_lowercase().contains(&q)) {
                    4
                } else {
                    return None;
                };
                Some((score, i))
            })
            .collect();
        hits.sort();
        let ids: Vec<usize> = hits.into_iter().map(|(_, i)| i).collect();
        drop_decls(&self.syms, ids).into_iter().take(limit).collect()
    }

    /// The one set of numbers every screen shows: code files, functions and
    /// types (no tests, no file-level modules), test functions, languages.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for f in self.files.iter().filter(|f| f.parsed) {
            c.code_files += 1;
            *c.languages.entry(f.file_type.clone()).or_default() += 1;
        }
        for s in &self.syms {
            if s.sym.test {
                if s.sym.kind.is_callable() {
                    c.tests += 1;
                }
            } else if !(s.sym.kind == Kind::Module && s.sym.start_line == 1) {
                c.symbols += 1;
            }
        }
        c
    }

    pub fn in_file(&self, path: &str) -> Vec<usize> {
        (0..self.syms.len()).filter(|&i| self.syms[i].file == path).collect()
    }

    /// Can code in `from_file` (crate/package `from_root`) see symbol `i`?
    /// Rust/Python: same crate or an imported repo crate. C/C++: global namespace,
    /// visible to other C/C++ files only.
    fn can_see(&self, from_root: &str, from_file: &str, i: usize) -> bool {
        let (tc, fc) = (is_c_family_path(&self.syms[i].file), is_c_family_path(from_file));
        if tc || fc {
            return tc == fc;
        }
        // languages never call each other by name; TS and JS are one family
        let (tf, ff) = (gitnext_parse::family_of(&self.syms[i].file), gitnext_parse::family_of(from_file));
        if tf != ff {
            return false;
        }
        // TS / JS / Go / Java: any module may import any other; imports decide, not crates
        if gitnext_parse::lang_for(std::path::Path::new(from_file)).is_some_and(gitnext_parse::Lang::is_generic) {
            return true;
        }
        // Rust and Python never call each other by name
        let py = |f: &str| f.ends_with(".py") || f.ends_with(".pyi");
        if py(&self.syms[i].file) != py(from_file) {
            return false;
        }
        let root = self.syms[i].sym.qualified.split("::").next().unwrap_or("");
        root == from_root || self.visible.get(from_root).is_some_and(|v| v.contains(root))
    }

    fn visible_only(&self, from: &str, from_file: &str, c: Vec<usize>) -> Vec<usize> {
        let from_root = from.split("::").next().unwrap_or("");
        c.into_iter()
            .filter(|&i| self.can_see(from_root, from_file, i))
            .filter(|&i| {
                // tests/, examples/, benches/ are separate targets: only visible to themselves.
                let f = &self.syms[i].file;
                !is_aux_target(f) || f == from_file || dir_of(f) == dir_of(from_file)
            })
            .collect()
    }

    /// A first path segment that names code in this repo (a crate, package or module root).
    pub fn is_repo_root(&self, first: &str) -> bool {
        self.crate_roots.contains(first)
    }

    /// The symbol an edge originates from (if it is a real symbol in that file).
    pub fn source_of(&self, file: &str, e: &Edge) -> Option<usize> {
        self.by_qual.get(&e.from)?.iter().copied().find(|&i| self.syms[i].file == file)
    }

    /// Resolve an edge target to candidate symbols. Empty = external / unknown.
    pub fn resolve(&self, file: &str, e: &Edge) -> Vec<usize> {
        match e.kind {
            EdgeKind::Calls => self.resolve_call(file, e),
            EdgeKind::Uses | EdgeKind::Implements | EdgeKind::Inherits => {
                let c: Vec<usize> = self
                    .by_name
                    .get(last_segment(&e.to))
                    .map(|v| v.iter().copied().filter(|&i| self.syms[i].sym.kind.is_type()).collect())
                    .unwrap_or_default();
                prefer_same_file(&self.syms, self.visible_only(&e.from, file, c), file)
            }
            EdgeKind::Imports if is_c_family_path(file) => {
                // #include "dir/x.h" → the file-module symbol of a header ending with that path
                let leaf = e.to.rsplit('/').next().unwrap_or(&e.to);
                let mut c: Vec<usize> = self
                    .by_name
                    .get(leaf)
                    .map(|v| {
                        v.iter()
                            .copied()
                            .filter(|&i| {
                                let s = &self.syms[i];
                                s.sym.kind == Kind::Module && (s.file == e.to || s.file.ends_with(&format!("/{}", e.to)))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // prefer the header next to the including file
                let d = dir_of(file);
                if c.len() > 1 {
                    let near: Vec<usize> = c.iter().copied().filter(|&i| dir_of(&self.syms[i].file) == d).collect();
                    if !near.is_empty() {
                        c = near;
                    }
                }
                c
            }
            EdgeKind::Imports => {
                let mut out = Vec::new();
                let from_root = e.from.split("::").next().unwrap_or("");
                for path in flatten_use(&e.to) {
                    out.extend(self.resolve_import(&path, from_root, file));
                }
                out.sort_unstable();
                out.dedup();
                out
            }
        }
    }

    fn resolve_import(&self, path: &str, from_root: &str, file: &str) -> Vec<usize> {
        let mut segs: Vec<&str> = path.split("::").collect();
        let Some(first) = segs.first().copied() else { return vec![] };
        let in_repo = matches!(first, "crate" | "self" | "super") || self.crate_roots.contains(first);
        if !in_repo {
            return vec![];
        }
        if first == "crate" {
            segs[0] = from_root;
        }
        let full = segs.join("::");
        if let Some(v) = self.by_qual.get(&full) {
            return v.clone();
        }
        let leaf = *segs.last().unwrap();
        let c: Vec<usize> = self
            .by_name
            .get(leaf)
            .map(|v| v.iter().copied().filter(|&i| self.syms[i].sym.kind != Kind::Method && self.can_see(from_root, file, i)).collect())
            .unwrap_or_default();
        prefer_same_file(&self.syms, c, file)
    }

    /// Fully qualified paths a file imports via `use` (with `crate` expanded).
    fn imported_paths(&self, file: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        for (f, e) in &self.edges {
            if f != file || e.kind != EdgeKind::Imports {
                continue;
            }
            let root = e.from.split("::").next().unwrap_or("");
            for p in flatten_use(&e.to) {
                out.insert(match p.strip_prefix("crate::") {
                    Some(rest) => format!("{root}::{rest}"),
                    None => p,
                });
            }
        }
        out
    }

    fn resolve_call(&self, file: &str, e: &Edge) -> Vec<usize> {
        if let Some((tf, tl)) = self.links.get(&(file.to_string(), e.line, e.col)) {
            if tf.is_empty() {
                return vec![]; // the language server says: outside the repo
            }
            let name = last_segment(&e.to);
            let inside: Vec<usize> = self
                .in_file(tf)
                .into_iter()
                .filter(|&i| {
                    let s = &self.syms[i].sym;
                    s.start_line <= *tl && *tl <= s.end_line && !(s.kind == Kind::Module && s.start_line == 1)
                })
                .collect();
            let best = inside
                .iter()
                .copied()
                .filter(|&i| self.syms[i].sym.name == name || self.syms[i].sym.kind.is_type())
                .min_by_key(|&i| self.syms[i].sym.end_line - self.syms[i].sym.start_line)
                .or_else(|| inside.iter().copied().min_by_key(|&i| self.syms[i].sym.end_line - self.syms[i].sym.start_line));
            if let Some(b) = best {
                return vec![b];
            }
        }
        drop_decls(&self.syms, self.resolve_call_raw(file, e))
    }

    fn resolve_call_raw(&self, file: &str, e: &Edge) -> Vec<usize> {
        let name = last_segment(&e.to);
        let Some(all) = self.by_name.get(name) else { return vec![] };
        let cands: Vec<usize> = all.iter().copied().filter(|&i| self.syms[i].sym.kind.is_callable()).collect();
        let cands = self.visible_only(&e.from, file, cands);
        let caller = self.source_of(file, e).map(|i| &self.syms[i]);
        let caller_owner = caller.and_then(|c| c.sym.parent.clone()).unwrap_or_default();
        let qual = qualifier(&e.to);

        if e.to.starts_with('.') {
            // std-trait methods on an unknown receiver are almost always std types.
            if COMMON_METHODS.contains(&name) {
                return vec![];
            }
            // receiver.method() with unknown receiver type: every visible method with
            // that name is a candidate (trait objects dispatch to all implementors).
            // Rust/Python: only methods taking `self` can be called with dot syntax.
            let cands: Vec<usize> = cands
                .into_iter()
                .filter(|&i| {
                    let s = &self.syms[i];
                    // C++, TS/JS, Go, Java: the receiver is implicit, every method can be called with a dot
                    is_c_family_path(&s.file)
                        || gitnext_parse::lang_for(std::path::Path::new(&s.file)).is_some_and(gitnext_parse::Lang::is_generic)
                        || s.sym.params.first().is_some_and(|p| p.name == "self" || p.name == "cls" || p.name.starts_with("mut self"))
                })
                .collect();
            return cands.into_iter().filter(|&i| self.syms[i].sym.kind == Kind::Method).collect();
        }
        // `Circle(...)` / `new Circle(...)`: instantiating a class or struct.
        if qual.is_empty() && name.chars().next().is_some_and(char::is_uppercase)
            && !cands.iter().any(|&i| self.syms[i].sym.kind == Kind::Function)
        {
            let types: Vec<usize> = all
                .iter()
                .copied()
                .filter(|&i| matches!(self.syms[i].sym.kind, Kind::Class | Kind::Struct))
                .collect();
            let types = self.visible_only(&e.from, file, types);
            if !types.is_empty() {
                return prefer_same_file(&self.syms, types, file);
            }
        }
        if is_c_family_path(file) && qual.is_empty() {
            // C/C++ bare call: a method of the enclosing class (implicit `this`) or a global function.
            let own: Vec<usize> = cands
                .iter()
                .copied()
                .filter(|&i| self.syms[i].sym.kind == Kind::Method && !caller_owner.is_empty() && self.syms[i].sym.parent.as_deref() == Some(caller_owner.as_str()))
                .collect();
            if !own.is_empty() {
                return own;
            }
            let funcs: Vec<usize> = cands.into_iter().filter(|&i| self.syms[i].sym.kind == Kind::Function).collect();
            return prefer_same_file(&self.syms, funcs, file);
        }
        let picked: Vec<usize> = if qual.is_empty() {
            // bare call: free functions; prefer same module, then what the file imports
            let funcs: Vec<usize> = cands.into_iter().filter(|&i| self.syms[i].sym.kind == Kind::Function).collect();
            let module = caller.and_then(|c| c.sym.parent.clone()).unwrap_or_default();
            let same_mod: Vec<usize> = funcs.iter().copied().filter(|&i| self.syms[i].sym.parent.as_deref() == Some(module.as_str())).collect();
            if !same_mod.is_empty() {
                same_mod
            } else {
                let imported = self.imported_paths(file);
                let via_use: Vec<usize> = funcs
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let q = &self.syms[i].sym.qualified;
                        imported.contains(q) || self.syms[i].sym.parent.as_ref().is_some_and(|p| imported.contains(p)) // glob `use m::*`
                    })
                    .collect();
                if !via_use.is_empty() {
                    via_use
                } else if gitnext_parse::lang_for(std::path::Path::new(file)).is_some_and(gitnext_parse::Lang::is_generic) {
                    // TS/JS/Go/Java: the name came in through an import (often re-exported by an index file)
                    importable(&self.syms, funcs, file)
                } else {
                    // Without an import a bare name can only reach its own crate.
                    let root = e.from.split("::").next().unwrap_or("");
                    funcs.into_iter().filter(|&i| self.syms[i].sym.qualified.split("::").next() == Some(root)).collect()
                }
            }
        } else if qual == "Self" {
            let own: Vec<usize> = cands
                .iter()
                .copied()
                .filter(|&i| self.syms[i].sym.parent.as_deref() == Some(caller_owner.as_str()))
                .collect();
            if own.is_empty() {
                // inherited: walk base classes / implemented traits of the owner
                self.inherited(&caller_owner, name, file)
            } else {
                own
            }
        } else if matches!(qual, "crate" | "super" | "self") {
            cands.into_iter().filter(|&i| self.syms[i].sym.kind == Kind::Function).collect()
        } else {
            // `Type::method` or `module::function`
            cands
                .into_iter()
                .filter(|&i| {
                    self.syms[i]
                        .sym
                        .parent
                        .as_deref()
                        .is_some_and(|p| last_segment(p) == qual)
                })
                .collect()
        };
        if picked.is_empty() && gitnext_parse::lang_for(std::path::Path::new(file)).is_some_and(gitnext_parse::Lang::is_generic) && !qual.is_empty() && qual != "Self" {
            // `pkg::index::create` where the index file only re-exports `create`: in-repo path → by name
            let first = e.to.split("::").next().unwrap_or("");
            if self.is_repo_root(first) {
                let funcs: Vec<usize> = all
                    .iter()
                    .copied()
                    .filter(|&i| matches!(self.syms[i].sym.kind, Kind::Function | Kind::Class | Kind::Struct))
                    .collect();
                return prefer_same_file(&self.syms, importable(&self.syms, self.visible_only(&e.from, file, funcs), file), file);
            }
        }
        prefer_same_file(&self.syms, picked, file)
    }

    /// Whether the repo defines a method with this name (for LSP site selection).
    pub fn has_method_named(&self, name: &str) -> bool {
        self.by_name.get(name).is_some_and(|v| v.iter().any(|&i| self.syms[i].sym.kind == Kind::Method))
    }

    /// Methods named `name` reachable from type `owner` through base classes /
    /// implemented traits (nearest level wins).
    pub fn inherited(&self, owner: &str, name: &str, file: &str) -> Vec<usize> {
        let mut frontier: Vec<String> = vec![owner.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        for _ in 0..6 {
            let mut next = Vec::new();
            for o in &frontier {
                for (f, e) in &self.edges {
                    if !matches!(e.kind, EdgeKind::Inherits | EdgeKind::Implements) || &e.from != o {
                        continue;
                    }
                    for b in self.resolve(f, e) {
                        let bq = self.syms[b].sym.qualified.clone();
                        if seen.insert(bq.clone()) {
                            next.push(bq);
                        }
                    }
                }
            }
            let hits: Vec<usize> = self
                .by_name
                .get(name)
                .map(|v| {
                    v.iter()
                        .copied()
                        .filter(|&i| self.syms[i].sym.kind == Kind::Method && self.syms[i].sym.parent.as_ref().is_some_and(|p| next.contains(p)))
                        .collect()
                })
                .unwrap_or_default();
            if !hits.is_empty() {
                return prefer_same_file(&self.syms, hits, file);
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        vec![]
    }

    /// Methods in subtypes of `owner` that override method `name`.
    pub fn overrides(&self, method: usize) -> Vec<usize> {
        let m = &self.syms[method].sym;
        let Some(owner) = m.parent.as_deref() else { return vec![] };
        let Some(&t) = self.by_qual.get(owner).and_then(|v| v.iter().find(|&&i| self.syms[i].sym.kind.is_type())) else { return vec![] };
        let mut out = Vec::new();
        let mut frontier = vec![t];
        let mut seen = HashSet::from([t]);
        while let Some(x) = frontier.pop() {
            let h = self.hierarchy(x);
            for sub in h.subtypes.iter().chain(h.implementors.iter()) {
                if let Some(&si) = self.by_qual.get(&sub.qualified).and_then(|v| v.iter().find(|&&i| self.syms[i].sym.kind.is_type())) {
                    if seen.insert(si) {
                        frontier.push(si);
                        let q = format!("{}::{}", self.syms[si].sym.qualified, m.name);
                        if let Some(v) = self.by_qual.get(&q) {
                            out.extend(v.iter().copied());
                        }
                    }
                }
            }
        }
        out
    }

    /// Reverse index of all resolved non-import edges.
    fn rev(&self) -> &RevIndex {
        &self.rev_all().0
    }

    fn rev_all(&self) -> &(RevIndex, Guesses) {
        self.rev.get_or_init(|| {
            let mut m: HashMap<usize, Vec<(usize, EdgeKind, u32)>> = HashMap::new();
            let mut sure: HashSet<(usize, usize)> = HashSet::new();
            let mut guess: HashSet<(usize, usize)> = HashSet::new();
            for (file, e) in &self.edges {
                if e.kind == EdgeKind::Imports {
                    continue;
                }
                let Some(src) = self.source_of(file, e) else { continue };
                let targets = self.resolve(file, e);
                let distinct: HashSet<&str> = targets.iter().map(|&t| self.syms[t].sym.qualified.as_str()).collect();
                for t in targets {
                    if t != src {
                        m.entry(t).or_default().push((src, e.kind, e.line));
                        if distinct.len() > 1 {
                            guess.insert((src, t));
                        } else {
                            sure.insert((src, t));
                        }
                    }
                }
            }
            for v in m.values_mut() {
                v.sort_unstable_by_key(|x| (x.0, x.2));
                v.dedup_by_key(|x| (x.0, x.1));
            }
            guess.retain(|p| !sure.contains(p));
            (m, guess)
        })
    }

    /// `src` reaches `target` only through an ambiguous reference (several
    /// same-named candidates, receiver type unknown).
    pub fn is_guess(&self, src: usize, target: usize) -> bool {
        self.rev_all().1.contains(&(src, target))
    }

    /// Everything that references symbol `target` (calls, type uses, impls, subclassing).
    pub fn referrers(&self, target: usize) -> &[(usize, EdgeKind, u32)] {
        self.rev().get(&target).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn callers(&self, target: usize) -> Vec<(usize, u32)> {
        let mut out: Vec<(usize, u32)> = self
            .referrers(target)
            .iter()
            .filter(|(_, k, _)| *k == EdgeKind::Calls)
            .map(|(s, _, l)| (*s, *l))
            .collect();
        // a definition also answers for its prototype (C/C++ header decl) and vice versa
        let me = &self.syms[target].sym;
        if let Some(twins) = self.by_qual.get(&me.qualified) {
            for &t in twins {
                if t != target && self.syms[t].sym.decl != me.decl {
                    out.extend(self.referrers(t).iter().filter(|(_, k, _)| *k == EdgeKind::Calls).map(|(s, _, l)| (*s, *l)));
                }
            }
        }
        out.sort_unstable();
        out.dedup_by_key(|x| x.0);
        out
    }

    /// Reverse-dependency walk: who breaks if `target` changes (BFS up to `depth`).
    pub fn impact(&self, target: usize, depth: usize) -> Vec<ImpactNode> {
        let mut out = Vec::new();
        let mut seen: HashSet<usize> = HashSet::from([target]);
        let mut frontier = vec![target];
        for d in 1..=depth {
            let mut next = Vec::new();
            for &t in &frontier {
                let mut refs: Vec<(usize, EdgeKind, u32)> = self.referrers(t).to_vec();
                // methods: also follow refs to the prototype twin
                if let Some(twins) = self.by_qual.get(&self.syms[t].sym.qualified) {
                    for &tw in twins {
                        if tw != t {
                            refs.extend_from_slice(self.referrers(tw));
                        }
                    }
                }
                for (src, kind, _) in refs {
                    let guess = self.is_guess(src, t);
                    if seen.insert(src) {
                        let s = &self.syms[src].sym;
                        out.push(ImpactNode {
                            depth: d,
                            symbol: self.sym_ref(src),
                            via: self.syms[t].sym.qualified.clone(),
                            how: kind.as_str(),
                            test: s.test,
                            certain: !guess,
                        });
                        // a guess is shown but not followed: its own callers are unrelated
                        if !guess {
                            next.push(src);
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        out
    }

    pub fn callees(&self, src: usize) -> Callees {
        let s = &self.syms[src];
        let mut out = Callees::default();
        let mut external = BTreeSet::new();
        let mut seen = HashSet::new();
        for (file, e) in &self.edges {
            if e.kind != EdgeKind::Calls || e.from != s.sym.qualified || *file != s.file {
                continue;
            }
            match self.resolve(file, e)[..] {
                [] => {
                    external.insert(e.to.clone());
                }
                [i] => {
                    if seen.insert(i) {
                        out.resolved.push((i, e.line));
                    }
                }
                ref many => {
                    if !out.ambiguous.iter().any(|(n, _)| *n == e.to) {
                        out.ambiguous.push((e.to.clone(), many.to_vec()));
                    }
                }
            }
        }
        out.external = external.into_iter().collect();
        out
    }

    /// Types referenced by a symbol (params, return type, fields) that live in the repo.
    pub fn uses(&self, src: usize) -> Vec<usize> {
        let s = &self.syms[src];
        let mut out = BTreeSet::new();
        for (file, e) in &self.edges {
            if e.kind == EdgeKind::Uses && e.from == s.sym.qualified && *file == s.file {
                out.extend(self.resolve(file, e));
            }
        }
        out.into_iter().collect()
    }

    pub fn hierarchy(&self, target: usize) -> Hierarchy {
        let t = &self.syms[target];
        let name = t.sym.name.as_str();
        let root = t.sym.qualified.split("::").next().unwrap_or("");
        let mut h = Hierarchy { symbol: Some(self.sym_ref(target)), ..Default::default() };
        let mut implements = BTreeSet::new();
        let mut extends = BTreeSet::new();
        let mut implementors = BTreeSet::new();
        let mut subtraits = BTreeSet::new();
        for (file, e) in &self.edges {
            let from_name = last_segment(&e.from);
            match e.kind {
                EdgeKind::Implements if from_name == name && e.from.starts_with(root) => {
                    implements.insert(e.to.clone());
                }
                EdgeKind::Inherits if e.from == t.sym.qualified => {
                    extends.insert(e.to.clone());
                }
                EdgeKind::Implements if t.sym.kind == Kind::Trait && last_segment(&e.to) == name => {
                    if self.resolve(file, e).contains(&target) {
                        let types: Vec<usize> = self
                            .by_name
                            .get(from_name)
                            .map(|v| v.iter().copied().filter(|&i| self.syms[i].sym.kind.is_type()).collect())
                            .unwrap_or_default();
                        match prefer_same_file(&self.syms, types, file).first() {
                            Some(&i) => {
                                implementors.insert(self.sym_ref(i));
                            }
                            None => {
                                implementors.insert(SymRef {
                                    qualified: e.from.clone(),
                                    short: last_segment(&e.from).to_string(),
                                    kind: "type".into(),
                                    file: file.clone(),
                                    line: e.line,
                                    signature: format!("impl {name} for {from_name}"),
                                    summary: None,
                                });
                            }
                        }
                    }
                }
                EdgeKind::Inherits if last_segment(&e.to) == name && self.resolve(file, e).contains(&target) => {
                    if let Some(i) = self.source_of(file, e) {
                        subtraits.insert(self.sym_ref(i));
                    }
                }
                _ => {}
            }
        }
        // One entry per method overload; a header declaration wins over the out-of-line
        // definition (its signature reads like the API).
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        for (i, s) in self.syms.iter().enumerate() {
            if s.sym.kind == Kind::Method
                && !s.sym.test
                && s.sym.parent.as_deref().is_some_and(|p| last_segment(p) == name && p.starts_with(root))
            {
                let key = (s.sym.qualified.clone(), param_key(&s.sym.params));
                let group = s.sym.trait_impl.clone().unwrap_or_default();
                match seen.get(&key) {
                    Some(&j) if self.syms[j].sym.decl || !s.sym.decl => continue,
                    Some(&j) => {
                        // replace definition with declaration
                        let old_q = self.syms[j].sym.signature.clone();
                        if let Some(v) = h.methods.get_mut(&group) {
                            if let Some(slot) = v.iter_mut().find(|r| r.signature == old_q) {
                                *slot = self.sym_ref(i);
                            }
                        }
                        seen.insert(key, i);
                    }
                    None => {
                        seen.insert(key, i);
                        h.methods.entry(group).or_default().push(self.sym_ref(i));
                    }
                }
            }
        }
        h.implements = implements.into_iter().collect();
        h.extends = extends.into_iter().collect();
        h.implementors = implementors.into_iter().collect();
        h.subtypes = subtraits.into_iter().collect();
        h
    }

    /// dir → (dir it depends on → number of references)
    pub fn dir_graph(&self) -> &HashMap<String, BTreeMap<String, usize>> {
        self.dir_graph.get_or_init(|| {
            let mut g: HashMap<String, BTreeMap<String, usize>> = HashMap::new();
            for (file, e) in &self.edges {
                let from_dir = dir_of(file);
                let mut seen = HashSet::new();
                for i in self.resolve(file, e) {
                    let to_dir = dir_of(&self.syms[i].file);
                    if to_dir != from_dir && seen.insert(to_dir) {
                        *g.entry(from_dir.to_string()).or_default().entry(to_dir.to_string()).or_default() += 1;
                    }
                }
            }
            g
        })
    }

    pub fn dir_deps(&self, dir: &str) -> DirDeps {
        let g = self.dir_graph();
        let mut d = DirDeps { dir: dir.to_string(), ..Default::default() };
        if let Some(m) = g.get(dir) {
            d.depends_on = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
        }
        let mut used: Vec<(String, usize)> = g
            .iter()
            .filter_map(|(from, m)| m.get(dir).map(|n| (from.clone(), *n)))
            .collect();
        used.sort();
        d.used_by = used;
        d.depends_on.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        d.used_by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        d
    }

    /// All directories that contain indexed files, including ancestors ("" = root).
    pub fn dirs(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        set.insert(String::new());
        for f in &self.files {
            let mut d = dir_of(&f.path);
            while !d.is_empty() {
                set.insert(d.to_string());
                d = dir_of(d);
            }
        }
        set.into_iter().collect()
    }
}

fn is_aux_target(path: &str) -> bool {
    ["tests/", "examples/", "benches/"].iter().any(|d| path.starts_with(d) || path.contains(&format!("/{d}")))
}

/// Methods of std traits / std types: `x.clone()` on an unknown `x` is not a repo call.
const COMMON_METHODS: &[&str] = &[
    "clone", "to_string", "to_owned", "fmt", "eq", "ne", "cmp", "partial_cmp", "hash", "into", "from",
    "try_into", "try_from", "as_ref", "as_mut", "borrow", "borrow_mut", "deref", "deref_mut", "drop",
    "next", "default", "iter", "iter_mut", "into_iter", "len", "is_empty", "unwrap", "expect", "map",
    "ok", "err", "get", "get_mut", "insert", "remove", "push", "pop", "contains", "extend", "collect",
    "append", "join", "split", "trim", "lower", "upper", "format", "items", "keys", "values", "copy",
    "update", "write", "read", "close", "flush", "c_str", "size", "begin", "end", "empty", "clear",
];

/// Parameter types without names/defaults, to tell overloads apart.
fn param_key(ps: &[gitnext_parse::Param]) -> String {
    ps.iter()
        .map(|p| p.ty.split('=').next().unwrap_or("").replace(' ', ""))
        .collect::<Vec<_>>()
        .join(",")
}

/// If both a prototype and its definition matched, keep only the definition.
fn drop_decls(syms: &[SymbolRow], c: Vec<usize>) -> Vec<usize> {
    if c.len() < 2 {
        return c;
    }
    let defined: HashSet<&str> = c.iter().filter(|&&i| !syms[i].sym.decl).map(|&i| syms[i].sym.qualified.as_str()).collect();
    c.iter().copied().filter(|&i| !(syms[i].sym.decl && defined.contains(syms[i].sym.qualified.as_str()))).collect()
}

/// What another file can import: exported, non-test definitions (a test file's
/// helpers are its own). Keeps everything when nothing qualifies.
fn importable(syms: &[SymbolRow], c: Vec<usize>, file: &str) -> Vec<usize> {
    let ok: Vec<usize> = c.iter().copied().filter(|&i| syms[i].file == file || (syms[i].sym.visibility.is_some() && !syms[i].sym.test)).collect();
    if ok.is_empty() { c } else { ok }
}

fn prefer_same_file(syms: &[SymbolRow], c: Vec<usize>, file: &str) -> Vec<usize> {
    let same: Vec<usize> = c.iter().copied().filter(|&i| syms[i].file == file).collect();
    if same.is_empty() { c } else { same }
}

/// Expand a Rust use-tree into flat paths:
/// `a::b::{c, d::{e, f as g}, self}` → `a::b::c`, `a::b::d::e`, `a::b::d::f`, `a::b`.
pub fn flatten_use(tree: &str) -> Vec<String> {
    fn go(prefix: &str, s: &str, out: &mut Vec<String>) {
        let s = s.trim();
        if let Some(open) = s.find('{') {
            let head = s[..open].trim().trim_end_matches("::");
            let body = &s[open + 1..s.rfind('}').unwrap_or(s.len())];
            let base = join(prefix, head);
            for part in split_top(body) {
                go(&base, part, out);
            }
        } else {
            let s = s.split(" as ").next().unwrap_or(s).trim();
            if s.is_empty() || s == "*" {
                if !prefix.is_empty() {
                    out.push(prefix.to_string());
                }
            } else if s == "self" {
                out.push(prefix.to_string());
            } else {
                out.push(join(prefix, s.trim_end_matches("::*")));
            }
        }
    }
    fn join(a: &str, b: &str) -> String {
        match (a.is_empty(), b.is_empty()) {
            (true, _) => b.to_string(),
            (_, true) => a.to_string(),
            _ => format!("{a}::{b}"),
        }
    }
    fn split_top(s: &str) -> Vec<&str> {
        let (mut depth, mut start, mut out) = (0, 0, vec![]);
        for (i, c) in s.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(&s[start..]);
        out
    }
    let mut out = vec![];
    go("", tree, &mut out);
    out.retain(|p| !p.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::flatten_use;
    #[test]
    fn use_trees() {
        assert_eq!(
            flatten_use("a::b::{c, d::{e, f as g}, self}"),
            vec!["a::b::c", "a::b::d::e", "a::b::d::f", "a::b"]
        );
        assert_eq!(flatten_use("std::io::*"), vec!["std::io"]);
        assert_eq!(flatten_use("crate::X"), vec!["crate::X"]);
    }
}
