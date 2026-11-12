//! Compact plain-text renderers, shared by the CLI and the MCP server.
//!
//! Designed for token economy: one fact per line, `path:line` locations, no
//! repeated information (parameters already in the signature aren't listed
//! again unless `verbose`), test code collapsed into counts.

use gitnext_index::{dir_of, Graph};
use gitnext_parse::{lang_for, Kind, Symbol};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

/// Rendering options.
#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    /// Human mode: list inputs/outputs separately, full external call lists.
    pub verbose: bool,
    /// Include test code instead of summarising it.
    pub tests: bool,
    /// Include private items in outlines of directories.
    pub all: bool,
    /// Full doc comments instead of the first paragraph.
    pub full_doc: bool,
    /// For a small model: leave out what it would have to doubt ("maybe" callers).
    pub lite: bool,
}

fn loc(g: &Graph, i: usize) -> String {
    format!("{}:{}", g.syms[i].file, g.syms[i].sym.start_line)
}

fn is_file_module(s: &Symbol) -> bool {
    s.kind == Kind::Module && s.start_line == 1
}

fn first_para(doc: &str) -> &str {
    doc.split("\n\n").next().unwrap_or(doc).trim()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ambiguous lookups: list the candidates instead of guessing.
pub fn candidates(g: &Graph, q: &str, ids: &[usize]) -> String {
    let mut s = format!("`{q}` matches {} symbols; use a qualified name:\n", ids.len());
    for &i in ids.iter().take(20) {
        let _ = writeln!(s, "{} {}  {}", g.syms[i].sym.kind.as_str(), g.short_name(i), loc(g, i));
    }
    s
}

// ───────────────────────────── outline ─────────────────────────────

/// Signatures + one-line docs of a file (or every file under a directory), no bodies.
pub fn outline(g: &Graph, root: &Path, path: &str, o: Opts) -> String {
    let path = path.trim_end_matches('/');
    let is_file = g.files.iter().any(|f| f.path == path);
    let files: Vec<&str> = if is_file {
        vec![path]
    } else {
        let prefix = if path.is_empty() || path == "." { String::new() } else { format!("{path}/") };
        g.files.iter().filter(|f| f.parsed && f.path.starts_with(&prefix)).map(|f| f.path.as_str()).collect()
    };
    if files.is_empty() {
        return format!("no parsed files at `{path}`\n");
    }
    let mut s = String::new();
    for f in &files {
        // directories: public API only unless --all; a single file: everything
        let public_only = !is_file && !o.all;
        outline_file(g, root, f, public_only, o, &mut s);
    }
    s
}

fn outline_file(g: &Graph, root: &Path, path: &str, public_only: bool, o: Opts, s: &mut String) {
    let Some(f) = g.files.iter().find(|f| f.path == path) else { return };
    let bytes = std::fs::metadata(root.join(path)).map(|m| m.len() as usize).unwrap_or(0);
    let ids = g.in_file(path);
    let syms: Vec<usize> = ids.into_iter().filter(|&i| !is_file_module(&g.syms[i].sym)).collect();
    let (tests, syms): (Vec<usize>, Vec<usize>) = syms.into_iter().partition(|&i| g.syms[i].sym.test && !o.tests);
    let visible: Vec<usize> = syms.into_iter().filter(|&i| !public_only || g.syms[i].sym.is_public()).collect();
    if visible.is_empty() && public_only {
        return;
    }
    let _ = writeln!(s, "# {path} · {} lines · ~{} tok", f.lines, bytes.div_ceil(4));
    if let Some(d) = &f.module_doc {
        let _ = writeln!(s, "{}", one_line(first_para(d)));
    }
    let methods_of = |owner: &str| -> Vec<usize> {
        visible.iter().copied().filter(|&m| g.syms[m].sym.kind == Kind::Method && g.syms[m].sym.parent.as_deref() == Some(owner)).collect()
    };
    let mut printed = std::collections::HashSet::new();
    for &i in &visible {
        let sym = &g.syms[i].sym;
        if printed.contains(&i) || (sym.kind == Kind::Method && visible.iter().any(|&t| g.syms[t].sym.kind.is_type() && sym.parent.as_deref() == Some(g.syms[t].sym.qualified.as_str()))) {
            continue;
        }
        printed.insert(i);
        if sym.kind == Kind::Method {
            // method of a type defined elsewhere: group consecutive ones under a header
            let owner = sym.parent.clone().unwrap_or_default();
            let group: Vec<usize> = visible.iter().copied().filter(|&m| !printed.contains(&m) || m == i).filter(|&m| g.syms[m].sym.kind == Kind::Method && g.syms[m].sym.parent.as_deref() == Some(owner.as_str())).collect();
            emit_method_group(g, &owner, &group, 0, s);
            printed.extend(group);
            continue;
        }
        let _ = writeln!(s, "L{} {}{}", sym.start_line, type_line(sym), summary_tail(sym));
        if sym.kind.is_type() {
            let ms = methods_of(&sym.qualified);
            emit_methods(g, &ms, 1, s);
            printed.extend(ms);
        }
    }
    if !tests.is_empty() {
        let fns = tests.iter().filter(|&&i| g.syms[i].sym.kind.is_callable()).count();
        if fns > 0 {
            let _ = writeln!(s, "tests: {fns} (L{}-{})", g.syms[tests[0]].sym.start_line, g.syms[*tests.last().unwrap()].sym.end_line);
        }
    }
    s.push('\n');
}

/// Methods: inherent ones with full signature; trait / interface impls condensed to names.
fn emit_methods(g: &Graph, ms: &[usize], depth: usize, s: &mut String) {
    let pad = "  ".repeat(depth);
    let mut by_trait: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &m in ms {
        let sym = &g.syms[m].sym;
        match &sym.trait_impl {
            Some(t) => by_trait.entry(t.clone()).or_default().push(m),
            None => {
                let _ = writeln!(s, "{pad}L{} {}{}", sym.start_line, sym.signature, summary_tail(sym));
            }
        }
    }
    for (t, ms) in by_trait {
        let names: Vec<String> = ms.iter().map(|&m| format!("{} L{}", g.syms[m].sym.name, g.syms[m].sym.start_line)).collect();
        let _ = writeln!(s, "{pad}impl {t}: {}", names.join(", "));
    }
}

fn emit_method_group(g: &Graph, owner: &str, ms: &[usize], depth: usize, s: &mut String) {
    let pad = "  ".repeat(depth);
    let o = gitnext_parse::last_segment(owner);
    let traits: Vec<&str> = ms.iter().filter_map(|&m| g.syms[m].sym.trait_impl.as_deref()).collect();
    if !traits.is_empty() && traits.len() == ms.len() {
        let names: Vec<String> = ms.iter().map(|&m| format!("{} L{}", g.syms[m].sym.name, g.syms[m].sym.start_line)).collect();
        let _ = writeln!(s, "{pad}impl {} for {o}: {}", traits[0], names.join(", "));
        return;
    }
    let _ = writeln!(s, "{pad}{o}:");
    emit_methods(g, ms, depth + 1, s);
}

/// Type declaration with inline fields/variants: `struct Task { id: Id, title: String }`.
fn type_line(sym: &Symbol) -> String {
    let inline = |max: usize, f: &dyn Fn(&gitnext_parse::Param) -> String| -> String {
        let mut v: Vec<String> = sym.params.iter().take(max).map(f).collect();
        if sym.params.len() > max {
            v.push(format!("…+{}", sym.params.len() - max));
        }
        v.join(", ")
    };
    match sym.kind {
        Kind::Struct | Kind::Class | Kind::Union if !sym.params.is_empty() => {
            format!("{} {{ {} }}", sym.signature, inline(10, &|p| if p.ty.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty) }))
        }
        Kind::Enum if !sym.params.is_empty() => format!("{} {{ {} }}", sym.signature, inline(14, &|p| {
            if p.ty.is_empty() { p.name.clone() } else if p.ty.starts_with('=') { format!("{} {}", p.name, p.ty) } else { format!("{}{}", p.name, p.ty) }
        })),
        _ => sym.signature.clone(),
    }
}

fn summary_tail(sym: &Symbol) -> String {
    let mark = if sym.inferred { "≈ " } else { "" };
    sym.summary.as_deref().map(|x| format!(" — {mark}{x}")).unwrap_or_default()
}

// ───────────────────────────── read ─────────────────────────────

/// Exact source of the given symbols (doc comments included), with line numbers.
pub fn read(g: &Graph, root: &Path, ids: &[usize]) -> String {
    let mut s = String::new();
    let mut cache: HashMap<&str, Vec<String>> = HashMap::new();
    for &i in ids {
        let r = &g.syms[i];
        let lines = cache.entry(r.file.as_str()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&r.file)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
        });
        let (mut a, b) = (r.sym.start_line as usize, (r.sym.end_line as usize).min(lines.len()));
        // pull in doc comments / attributes / decorators directly above
        while a > 1 {
            let prev = lines[a - 2].trim_start();
            let is_doc = prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("#[") || prev.starts_with('@')
                || prev.starts_with("/**") || prev.starts_with("* ") || prev == "*" || prev.starts_with("*/")
                || (prev.starts_with("//") && !lines[a - 2].trim().is_empty());
            if is_doc { a -= 1 } else { break }
        }
        if a == 0 || b < a {
            continue;
        }
        let fence = lang_for(Path::new(&r.file)).map(|l| l.fence()).unwrap_or("");
        let _ = writeln!(s, "{}:{}-{} {}", r.file, a, b, g.short_name(i));
        let _ = writeln!(s, "```{fence}");
        for line in &lines[a - 1..b] {
            let _ = writeln!(s, "{line}");
        }
        let _ = writeln!(s, "```");
    }
    s
}

// ───────────────────────────── show ─────────────────────────────

pub fn show(g: &Graph, i: usize, o: Opts) -> String {
    let r = &g.syms[i];
    let sym = &r.sym;
    let mut s = String::new();
    let _ = writeln!(s, "{} {}  {}:{}-{}", sym.kind.as_str(), sym.qualified, r.file, sym.start_line, sym.end_line);
    let _ = writeln!(s, "{}", type_line(sym));
    if let Some(t) = &sym.trait_impl {
        let _ = writeln!(s, "implements: {t}");
    }
    if o.verbose && sym.kind.is_callable() {
        if !sym.params.is_empty() {
            let ps: Vec<String> = sym.params.iter().map(|p| if p.ty.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty) }).collect();
            let _ = writeln!(s, "in: {}", ps.join(" · "));
        }
        let _ = writeln!(s, "out: {}", sym.returns.as_deref().unwrap_or("—"));
    }
    if let Some(d) = &sym.doc {
        let text = if o.full_doc { d.trim() } else { first_para(d) };
        let more = if !o.full_doc && text.len() < d.trim().len() { " (…)" } else { "" };
        for (n, l) in text.lines().enumerate() {
            let _ = writeln!(s, "{}{l}", if n == 0 { "doc: " } else { "     " });
        }
        if !more.is_empty() {
            let _ = writeln!(s, "     (more: --full)");
        }
    }
    if sym.doc.is_none() {
        if let Some(sum) = sym.summary.as_deref().filter(|_| sym.inferred) {
            let _ = writeln!(s, "≈ {sum}  (inferred)");
        }
    }
    if sym.decl {
        let _ = writeln!(s, "declaration only (no body here)");
    }
    if sym.kind.is_callable() || sym.kind.is_type() {
        let callers = g.callers(i);
        if !callers.is_empty() {
            let _ = writeln!(s, "{}", list_line("callers", &callers.iter().map(|x| x.0).collect::<Vec<_>>(), g, o));
        }
    }
    if sym.kind.is_callable() {
        let c = g.callees(i);
        let mut parts = Vec::new();
        if !c.resolved.is_empty() {
            let v: Vec<String> = c.resolved.iter().map(|(x, _)| g.short_name(*x).to_string()).collect();
            parts.push(format!("calls ({}): {}", v.len(), v.join(", ")));
        }
        if !c.ambiguous.is_empty() {
            let v: Vec<String> = c.ambiguous.iter().map(|(n, v)| format!("{n}?{}", v.len())).collect();
            parts.push(format!("ambiguous: {}", v.join(", ")));
        }
        if !c.external.is_empty() {
            if o.verbose {
                parts.push(format!("external: {}", c.external.join(", ")));
            } else {
                parts.push(format!("external: {}", c.external.len()));
            }
        }
        for p in parts {
            let _ = writeln!(s, "{p}");
        }
    }
    let uses = g.uses(i);
    if !uses.is_empty() {
        let v: Vec<String> = uses.iter().map(|&u| g.syms[u].sym.name.clone()).collect();
        let _ = writeln!(s, "uses: {}", v.join(", "));
    }
    if sym.kind.is_type() {
        hierarchy_body(g, i, o, &mut s, false);
    }
    s
}

/// `callers (3): a, b, c  +5 tests`
fn list_line(label: &str, ids: &[usize], g: &Graph, o: Opts) -> String {
    let (tests, real): (Vec<usize>, Vec<usize>) = ids.iter().partition(|&&i| g.syms[i].sym.test);
    let mut v: Vec<String> = real.iter().map(|&i| g.short_name(i).to_string()).collect();
    if o.tests {
        v.extend(tests.iter().map(|&i| format!("{} [test]", g.short_name(i))));
    }
    let mut line = format!("{label} ({}): {}", real.len(), v.join(", "));
    if !o.tests && !tests.is_empty() {
        let _ = write!(line, "  +{} tests", tests.len());
    }
    line
}

// ───────────────────────────── callers / callees ─────────────────────────────

pub fn callers(g: &Graph, i: usize, o: Opts) -> String {
    let all = g.callers(i);
    let (tests, real): (Vec<_>, Vec<_>) = all.iter().partition(|(c, _)| g.syms[*c].sym.test);
    // sure: the call names this function, or the receiver's type is known; maybe: a call on a
    // value of unknown type that could reach it
    let (sure, maybe): (Vec<_>, Vec<_>) = real.iter().partition(|(c, _)| !g.is_guess(*c, i));
    let trust = crate::trust::callers_sure(&g.syms[i].file);
    let mut s = format!(
        "callers of {} ({} sure{}{}):\n",
        g.syms[i].sym.qualified,
        sure.len(),
        if trust.is_empty() { String::new() } else { format!(", {trust}") },
        if maybe.is_empty() { String::new() } else { format!("; {} maybe", maybe.len()) }
    );
    for (c, line) in &sure {
        let _ = writeln!(s, "{}  {}:{line}", g.short_name(*c), g.syms[*c].file);
    }
    if !maybe.is_empty() && !o.lite {
        let _ = writeln!(s, "maybe — the receiver's type is unknown, check each before relying on it:");
        for (c, line) in &maybe {
            let _ = writeln!(s, "{}  {}:{line}", g.short_name(*c), g.syms[*c].file);
        }
    }
    if o.tests {
        for (c, line) in &tests {
            let _ = writeln!(s, "{} [test]  {}:{line}", g.short_name(*c), g.syms[*c].file);
        }
    } else if !tests.is_empty() {
        let _ = writeln!(s, "+{} in tests (--tests to list)", tests.len());
    }
    s
}

pub fn callees(g: &Graph, i: usize, o: Opts) -> String {
    let c = g.callees(i);
    let mut s = format!("calls from {}:\n", g.syms[i].sym.qualified);
    for (x, line) in &c.resolved {
        let _ = writeln!(s, "{}  L{line} → {}", g.short_name(*x), loc(g, *x));
    }
    for (name, cands) in &c.ambiguous {
        let v: Vec<&str> = cands.iter().take(6).map(|&x| g.short_name(x)).collect();
        let _ = writeln!(s, "{name} ambiguous: {}", v.join(" | "));
    }
    if !c.external.is_empty() {
        if o.verbose || c.external.len() <= 12 {
            let _ = writeln!(s, "external: {}", c.external.join(", "));
        } else {
            let _ = writeln!(s, "external ({}): {} …", c.external.len(), c.external[..12].join(", "));
        }
    }
    s
}

// ───────────────────────────── impact ─────────────────────────────

/// Who is affected if `i` changes: reverse dependency tree, files to re-check, tests to run.
pub fn impact(g: &Graph, i: usize, depth: usize, o: Opts) -> String {
    let all = g.impact(i, depth);
    let (maybe, nodes): (Vec<_>, Vec<_>) = all.into_iter().partition(|n| !n.certain);
    let (tests, real): (Vec<_>, Vec<_>) = nodes.iter().partition(|n| n.test);
    let mut files: Vec<&str> = real.iter().map(|n| n.symbol.file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let mut s = format!(
        "impact of {} (depth {depth}): {} symbols in {} files, {} tests\n",
        g.syms[i].sym.qualified,
        real.len(),
        files.len(),
        tests.len()
    );
    // print as a tree: children grouped by `via`
    let mut kids: HashMap<&str, Vec<&gitnext_index::ImpactNode>> = HashMap::new();
    for n in &real {
        kids.entry(n.via.as_str()).or_default().push(n);
    }
    fn walk(q: &str, d: usize, kids: &HashMap<&str, Vec<&gitnext_index::ImpactNode>>, s: &mut String, budget: &mut usize) {
        if let Some(v) = kids.get(q) {
            for n in v {
                if *budget == 0 {
                    return;
                }
                *budget -= 1;
                let _ = writeln!(s, "{}← {} ({})  {}:{}", "  ".repeat(d), n.symbol.short, n.how, n.symbol.file, n.symbol.line);
                walk(&n.symbol.qualified, d + 1, kids, s, budget);
            }
        }
    }
    let mut budget = 80;
    walk(&g.syms[i].sym.qualified, 0, &kids, &mut s, &mut budget);
    if budget == 0 {
        let _ = writeln!(s, "… (truncated)");
    }
    if !files.is_empty() {
        let _ = writeln!(s, "files: {}", files.join(", "));
    }
    if !tests.is_empty() {
        let mut v: Vec<String> = tests.iter().map(|n| n.symbol.short.clone()).collect();
        v.dedup();
        if o.tests || v.len() <= 8 {
            let _ = writeln!(s, "tests: {}", v.join(", "));
        } else {
            let _ = writeln!(s, "tests: {} … +{} (--tests)", v[..8].join(", "), v.len() - 8);
        }
    }
    if !maybe.is_empty() {
        let shown: Vec<String> = maybe.iter().take(12).map(|n| format!("{} ({}:{})", n.symbol.short, n.symbol.file, n.symbol.line)).collect();
        let _ = writeln!(
            s,
            "maybe ({}, call on an object of unknown type with the same method name — not followed further): {}{}",
            maybe.len(),
            shown.join(", "),
            if maybe.len() > 12 { " …" } else { "" }
        );
    }
    s
}

// ───────────────────────────── hierarchy ─────────────────────────────

pub fn hierarchy(g: &Graph, i: usize, o: Opts) -> String {
    let r = &g.syms[i];
    let mut s = format!("{} {}  {}\n", r.sym.kind.as_str(), r.sym.qualified, loc(g, i));
    hierarchy_body(g, i, o, &mut s, true);
    s
}

fn hierarchy_body(g: &Graph, i: usize, _o: Opts, s: &mut String, with_methods: bool) {
    let h = g.hierarchy(i);
    if !h.extends.is_empty() {
        let _ = writeln!(s, "extends: {}", h.extends.join(", "));
    }
    if !h.implements.is_empty() {
        let _ = writeln!(s, "implements: {}", h.implements.join(", "));
    }
    let short = |v: &[gitnext_index::SymRef]| -> String {
        v.iter().map(|x| format!("{} ({}:{})", x.short, x.file, x.line)).collect::<Vec<_>>().join(", ")
    };
    if !h.implementors.is_empty() {
        let _ = writeln!(s, "implemented by: {}", short(&h.implementors));
    }
    if !h.subtypes.is_empty() {
        let _ = writeln!(s, "extended by: {}", short(&h.subtypes));
    }
    let n: usize = h.methods.values().map(Vec::len).sum();
    if n == 0 {
        return;
    }
    if !with_methods {
        let _ = writeln!(s, "methods: {n} (hierarchy for the list)");
        return;
    }
    for (imp, ms) in &h.methods {
        let title = if imp.is_empty() { "methods".to_string() } else { format!("impl {imp}") };
        let _ = writeln!(s, "{title}:");
        for m in ms {
            let _ = writeln!(s, "  L{} {}", m.line, m.signature);
        }
    }
}

// ───────────────────────────── deps / search ─────────────────────────────

pub fn deps(g: &Graph, dir: &str) -> String {
    let d = g.dir_deps(dir);
    let name = if dir.is_empty() { "/" } else { dir };
    let fmt = |v: &[(String, usize)]| {
        if v.is_empty() {
            "—".to_string()
        } else {
            v.iter().map(|(x, n)| format!("{} ({n})", if x.is_empty() { "/" } else { x })).collect::<Vec<_>>().join(", ")
        }
    };
    format!("{name}\ndepends on: {}\nused by: {}\n", fmt(&d.depends_on), fmt(&d.used_by))
}

pub fn search(g: &Graph, q: &str, limit: usize, o: Opts) -> String {
    let hits: Vec<usize> = g.search(q, limit * 3).into_iter().filter(|&i| o.tests || !g.syms[i].sym.test).filter(|&i| !is_file_module(&g.syms[i].sym)).take(limit).collect();
    if hits.is_empty() {
        return format!("no symbols match `{q}`\n");
    }
    let mut s = String::new();
    for i in hits {
        let sym = &g.syms[i].sym;
        let _ = writeln!(s, "{} {}  {}{}", sym.kind.as_str(), g.short_name(i), loc(g, i), summary_tail(sym));
    }
    s
}

/// `symbols` listing: one line per symbol, filterable.
pub fn symbols(g: &Graph, path: &str, kind: Option<Kind>, public: bool, o: Opts) -> String {
    let mut s = String::new();
    let mut last = "";
    for (i, r) in g.syms.iter().enumerate() {
        let inside = path.is_empty() || r.file == path || r.file.starts_with(&format!("{path}/"));
        if !inside || is_file_module(&r.sym) || (r.sym.test && !o.tests) || kind.is_some_and(|k| r.sym.kind != k) || (public && !r.sym.is_public()) {
            continue;
        }
        if r.file != last {
            let _ = writeln!(s, "# {}", r.file);
            last = &r.file;
        }
        let pad = if r.sym.kind == Kind::Method { "  " } else { "" };
        let _ = writeln!(s, "{pad}L{} {}{}", r.sym.start_line, r.sym.signature, summary_tail(&r.sym));
        let _ = i;
    }
    if s.is_empty() {
        s.push_str("no symbols\n");
    }
    s
}

/// Directory of a path, for callers that accept a file where a dir is expected.
pub fn dir_for(g: &Graph, p: &str) -> String {
    if g.files.iter().any(|f| f.path == p) { dir_of(p).to_string() } else { p.to_string() }
}
