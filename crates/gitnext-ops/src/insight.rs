//! Understanding a repository at a glance: activity, hotspots, dead code,
//! dependency cycles, entry points, architecture diagram and a one-screen overview.

use anyhow::Result;
use gitnext_core::Repo;
use gitnext_index::Graph;
pub use gitnext_docs::{area, area_graph, mermaid};
use gitnext_parse::Kind;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

// ───────────────────────────── git activity ─────────────────────────────

#[derive(Debug, Default, Serialize)]
pub struct Activity {
    pub commits: usize,
    pub first_date: String,
    pub last_date: String,
    pub by_area: Vec<(String, usize)>,
    pub by_author: Vec<(String, usize)>,
    pub hot_files: Vec<(String, usize)>,
    /// symbols named in Semantic-Changes trailers, most frequent first
    pub hot_symbols: Vec<(String, usize)>,
    pub recent: Vec<(String, String, String, String)>,
    pub branches: Vec<(String, String, String)>,
    pub uncommitted: usize,
}

/// Paths touched by a commit (vs its first parent).
fn touched(git: &git2::Repository, c: &git2::Commit) -> Vec<String> {
    let Ok(tree) = c.tree() else { return vec![] };
    let parent = c.parent(0).ok().and_then(|p| p.tree().ok());
    let Ok(diff) = git.diff_tree_to_tree(parent.as_ref(), Some(&tree), None) else { return vec![] };
    diff.deltas()
        .filter_map(|d| d.new_file().path().or_else(|| d.old_file().path()).map(|p| p.to_string_lossy().replace('\\', "/")))
        .filter(|p| !p.starts_with(".gitnext/") && p != "AGENTS.md")
        .collect()
}

pub fn activity(repo: &Repo, limit: usize) -> Result<Activity> {
    let git = &repo.inner;
    let mut a = Activity::default();
    let mut walk = git.revwalk()?;
    if walk.push_head().is_ok() {
        walk.set_sorting(git2::Sort::TIME)?;
        let mut areas: HashMap<String, usize> = HashMap::new();
        let mut authors: HashMap<String, usize> = HashMap::new();
        let mut files: HashMap<String, usize> = HashMap::new();
        let mut syms: HashMap<String, usize> = HashMap::new();
        for oid in walk.take(limit) {
            let c = git.find_commit(oid?)?;
            a.commits += 1;
            let d = crate::history_date(c.time().seconds());
            if a.last_date.is_empty() {
                a.last_date = d.clone();
            }
            a.first_date = d.clone();
            let who = c.author().name().unwrap_or("?").to_string();
            *authors.entry(who.clone()).or_default() += 1;
            let mut seen_areas = HashSet::new();
            for p in touched(git, &c) {
                if seen_areas.insert(area(&p)) {
                    *areas.entry(area(&p)).or_default() += 1;
                }
                *files.entry(p).or_default() += 1;
            }
            if let Some(t) = c.message().ok().and_then(|m| m.split("Semantic-Changes (gitnext):\n").nth(1).map(str::to_string)) {
                for l in t.lines() {
                    let l = l.trim();
                    if let Some(rest) = l.strip_prefix("~ ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("+ ")) {
                        if let Some(name) = rest.split_whitespace().nth(1) {
                            let short = name.trim_end_matches(':').rsplit("::").take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("::");
                            *syms.entry(short).or_default() += 1;
                        }
                    }
                }
            }
            if a.recent.len() < 8 {
                a.recent.push((c.id().to_string()[..7].to_string(), d, who, c.summary().ok().flatten().unwrap_or("").to_string()));
            }
        }
        let top = |m: HashMap<String, usize>, n: usize| {
            let mut v: Vec<(String, usize)> = m.into_iter().collect();
            v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            v.truncate(n);
            v
        };
        a.by_area = top(areas, 8);
        a.by_author = top(authors, 8);
        a.hot_files = top(files, 8);
        a.hot_symbols = top(syms, 8);
    }
    if let Ok(bs) = git.branches(Some(git2::BranchType::Local)) {
        for b in bs.flatten() {
            let (b, _) = b;
            let name = b.name().ok().flatten().unwrap_or("").to_string();
            if let Ok(c) = b.get().peel_to_commit() {
                a.branches.push((name, crate::history_date(c.time().seconds()), c.summary().ok().flatten().unwrap_or("").to_string()));
            }
        }
    }
    a.uncommitted = repo.status().map(|s| s.len()).unwrap_or(0);
    Ok(a)
}

pub fn render_activity(a: &Activity) -> String {
    let mut s = String::new();
    if a.commits == 0 {
        s.push_str("no commits yet\n");
    } else {
        let _ = writeln!(s, "{} commits from {} to {}", a.commits, a.first_date, a.last_date);
        let j = |v: &[(String, usize)]| v.iter().map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", ");
        let _ = writeln!(s, "areas: {}", j(&a.by_area));
        let _ = writeln!(s, "authors: {}", j(&a.by_author));
        let _ = writeln!(s, "most changed files: {}", j(&a.hot_files));
        if !a.hot_symbols.is_empty() {
            let _ = writeln!(s, "most changed symbols: {}", j(&a.hot_symbols));
        }
        let _ = writeln!(s, "recent:");
        for (id, d, who, msg) in &a.recent {
            let _ = writeln!(s, "  {d} {id} {who}: {msg}");
        }
    }
    if a.branches.len() > 1 {
        let _ = writeln!(s, "branches:");
        for (b, d, msg) in &a.branches {
            let _ = writeln!(s, "  {b} · {d} · {msg}");
        }
    }
    if a.uncommitted > 0 {
        let _ = writeln!(s, "working tree: {} changed paths (gitnext diff)", a.uncommitted);
    }
    s
}

// ───────────────────────────── hotspots ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct Hotspot {
    pub file: String,
    pub commits: usize,
    pub lines: u32,
    pub fan_in: usize,
    pub score: f64,
    /// biggest / most referenced symbols in the file
    pub symbols: Vec<String>,
}

/// Where change frequency meets size and dependants: the risky files.
pub fn hotspots(repo: Option<&Repo>, g: &Graph, limit_commits: usize, n: usize) -> Result<Vec<Hotspot>> {
    let mut churn: HashMap<String, usize> = HashMap::new();
    if let Some(repo) = repo {
        let git = &repo.inner;
        let mut walk = git.revwalk()?;
        if walk.push_head().is_ok() {
            for oid in walk.take(limit_commits) {
                let c = git.find_commit(oid?)?;
                for p in touched(git, &c) {
                    *churn.entry(p).or_default() += 1;
                }
            }
        }
    }
    let mut out = Vec::new();
    for f in g.files.iter().filter(|f| f.parsed) {
        let ids: Vec<usize> = g.in_file(&f.path).into_iter().filter(|&i| !g.syms[i].sym.test).collect();
        if ids.is_empty() || ids.iter().all(|&i| g.syms[i].sym.test) {
            continue;
        }
        let fan_in: usize = ids.iter().map(|&i| g.referrers(i).iter().filter(|(s, _, _)| g.syms[*s].file != f.path).count()).sum();
        let commits = churn.get(&f.path).copied().unwrap_or(0);
        let score = (1.0 + commits as f64) * (1.0 + f.lines as f64).ln() * (1.0 + (1.0 + fan_in as f64).ln());
        let mut top: Vec<(usize, usize)> = ids
            .iter()
            .filter(|&&i| g.syms[i].sym.kind.is_callable() || g.syms[i].sym.kind.is_type())
            .map(|&i| ((g.syms[i].sym.end_line - g.syms[i].sym.start_line) as usize + 5 * g.referrers(i).len(), i))
            .collect();
        top.sort_by_key(|e| std::cmp::Reverse(e.0));
        out.push(Hotspot {
            file: f.path.clone(),
            commits,
            lines: f.lines,
            fan_in,
            score: (score * 10.0).round() / 10.0,
            symbols: top.iter().take(3).map(|&(_, i)| g.short_name(i).to_string()).collect(),
        });
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(n);
    Ok(out)
}

pub fn render_hotspots(v: &[Hotspot]) -> String {
    let mut s = String::from("hotspots (commits × size × dependants):\n");
    for h in v {
        let _ = writeln!(s, "{}  {} commits · {} lines · {} refs in · {}", h.file, h.commits, h.lines, h.fan_in, h.symbols.join(", "));
    }
    s
}

// ───────────────────────────── dead code ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct Dead {
    pub likely: Vec<String>,
    pub public_unused: Vec<String>,
}

fn is_entry_name(name: &str) -> bool {
    // `init`: Go runs every `func init()` of a package by itself
    matches!(name, "main" | "init" | "new" | "default" | "drop" | "fmt" | "from" | "setup" | "teardown")
        || (name.starts_with("__") && name.ends_with("__"))
        || name.starts_with('~')
}

/// Symbols nothing in the repo refers to: no call in the graph, and the name is written nowhere
/// outside the symbol itself (approximate: reflection, generated names and external users are invisible).
pub fn dead(g: &Graph, root: &Path) -> Dead {
    let entries: HashSet<usize> = entrypoints(g, root).into_iter().map(|e| e.symbol).collect();
    let referenced: HashSet<&str> = g
        .syms
        .iter()
        .enumerate()
        .filter(|(i, _)| !g.referrers(*i).is_empty())
        .map(|(_, s)| s.sym.qualified.as_str())
        .collect();
    let mut likely = Vec::new();
    let mut public_unused = Vec::new();
    let mut candidates = Vec::new();
    for (i, r) in g.syms.iter().enumerate() {
        let s = &r.sym;
        if s.test || s.trait_impl.is_some() || s.decl || !(s.kind.is_callable() || s.kind.is_type()) || is_entry_name(&s.name) || entries.contains(&i) {
            continue;
        }
        // constructors run implicitly (`Scheduler s(q);`, `Foo()`)
        if s.kind == Kind::Method && s.parent.as_deref().is_some_and(|p| gitnext_parse::last_segment(p) == s.name) {
            continue;
        }
        // methods of a trait/interface or overrides are reached through dispatch
        if s.kind == Kind::Method && (s.signature.contains("override") || s.signature.contains("virtual") || g.syms.iter().any(|t| Some(&t.sym.qualified) == s.parent.as_ref() && t.sym.kind == Kind::Trait)) {
            continue;
        }
        if s.signature.starts_with('@') {
            continue; // decorated Python callables are usually registered somewhere
        }
        if r.file.ends_with(".d.ts") {
            continue; // declarations for the compiler (`interface ImportMeta`), merged with ones outside the repo
        }
        if referenced.contains(s.qualified.as_str()) {
            continue;
        }
        // a type is used when any of its methods is (`FileStore::open(..)`)
        if s.kind.is_type() && g.syms.iter().any(|m| m.sym.parent.as_deref() == Some(s.qualified.as_str()) && referenced.contains(m.sym.qualified.as_str())) {
            continue;
        }
        // overrides of a base-class / trait method are reached by dynamic dispatch
        if s.kind == Kind::Method {
            if let Some(owner) = s.parent.as_deref() {
                if !g.inherited(owner, &s.name, &r.file).is_empty() {
                    continue;
                }
            }
        }
        candidates.push(i);
    }
    // The graph knows calls. A name is also used as a value (`return _BaseCommand`), a type
    // (`-> _SourceVarsDict`), a base or metaclass, an import, a callback (`map(helper)`): if the name
    // is written anywhere outside the symbol itself, something refers to it.
    let named = named_elsewhere(g, root, &candidates);
    let run_by_attribute = entry_attribute(g, root, &candidates);
    for i in candidates {
        if named.contains(&i) || run_by_attribute.contains(&i) {
            continue;
        }
        let (r, s) = (&g.syms[i], &g.syms[i].sym);
        let line = format!("{} {}  {}:{}", s.kind.as_str(), g.short_name(i), r.file, s.start_line);
        // `protected`: there for subclasses, outside the repo too — API, not dead
        if (s.is_public() || is_protected(s)) && !s.name.starts_with('_') {
            public_unused.push(line);
        } else {
            likely.push(line);
        }
    }
    Dead { likely, public_unused }
}

/// Of `candidates`, the symbols whose name is written in the code outside their own lines
/// (comment lines aside), or is built from a piece written next to reflection
/// (`getattr(self, f"_process_{kind}")`: every `_process_*` is reached). One pass over the files.
fn named_elsewhere(g: &Graph, root: &Path, candidates: &[usize]) -> HashSet<usize> {
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in candidates {
        by_name.entry(g.syms[i].sym.name.as_str()).or_default().push(i);
    }
    let mut named = HashSet::new();
    if by_name.is_empty() {
        return named;
    }
    let mut pieces: HashSet<NamePiece> = HashSet::new();
    for f in g.files.iter().filter(|f| f.parsed && f.lines < 50_000) {
        let Ok(text) = std::fs::read_to_string(root.join(&f.path)) else { continue };
        let mut found: Vec<(u32, NamePiece)> = Vec::new();
        let mut reflection: Vec<u32> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with('#') || t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                continue;
            }
            let ln = n as u32 + 1;
            for word in line.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                let Some(ids) = by_name.get(word) else { continue };
                for &i in ids {
                    let r = &g.syms[i];
                    if !(r.file == f.path && r.sym.start_line <= ln && ln <= r.sym.end_line) {
                        named.insert(i);
                    }
                }
            }
            if REFLECTION.iter().any(|m| line.contains(m)) {
                reflection.push(ln);
            }
            for (piece, indexed) in name_pieces(line) {
                if indexed {
                    reflection.push(ln); // `obj["on" + kind]`, `` obj[`on${kind}`] ``
                }
                found.push((ln, piece));
            }
        }
        // a piece counts only near reflection: `'Token' + …` in a `__repr__` builds no name
        for (ln, piece) in found {
            if reflection.iter().any(|&r| r.abs_diff(ln) <= 3) {
                pieces.insert(piece);
            }
        }
    }
    if !pieces.is_empty() {
        for &i in candidates {
            let name = g.syms[i].sym.name.to_lowercase();
            if pieces.iter().any(|p| match p {
                NamePiece::Prefix(x) => name.len() > x.len() && name.starts_with(x.as_str()),
                NamePiece::Suffix(x) => name.len() > x.len() && name.ends_with(x.as_str()),
            }) {
                named.insert(i);
            }
        }
    }
    named
}

/// Calls that reach a member by a name held in a string.
const REFLECTION: &[&str] = &[
    "getattr(", "hasattr(", "globals()", "locals()", "vars(", "__dict__", "methodcaller(", // Python
    "getMethod(", "getDeclaredMethod(", "getField(", "getDeclaredField(", "forName(",      // Java
    "MethodByName(", "FieldByName(",                                                      // Go
];

/// The start or the end of a name glued together at run time, lowercased (sqlparse lowercases it).
#[derive(Debug, PartialEq, Eq, Hash)]
enum NamePiece {
    Prefix(String),
    Suffix(String),
}

/// String pieces of `line` that are glued to something else into a name: `"_process_" + x`,
/// `f'_process_{x}'`, `'_process_%s' % x`, `'_process_{}'.format(x)`, `` `on${x}` ``, `x + "_handler"`,
/// `f'{x}_handler'`. Only identifier characters, at least 3 letters or digits (`f"_{name}"` would make
/// every `_x` alive). The flag: the string stands right after `[` (`obj["on" + x]`), itself a lookup by name.
fn name_pieces(line: &str) -> Vec<(NamePiece, bool)> {
    fn piece(s: &str) -> Option<String> {
        (s.chars().all(|c| c.is_alphanumeric() || c == '_') && s.chars().filter(|c| c.is_alphanumeric()).count() >= 3)
            .then(|| s.to_lowercase())
    }
    let b: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let q = b[i];
        if !matches!(q, '"' | '\'' | '`') {
            i += 1;
            continue;
        }
        // f'…' / F'…' / rf'…': an interpolating string
        let fstr = q == '`' || (i > 0 && matches!(b[i - 1], 'f' | 'F') && (i < 2 || !b[i - 2].is_alphanumeric() || matches!(b[i - 2], 'r' | 'R')));
        let mut j = i + 1;
        while j < b.len() && b[j] != q {
            j += if b[j] == '\\' { 2 } else { 1 };
        }
        if j >= b.len() {
            break;
        }
        let body: String = b[i + 1..j].iter().collect();
        let before = b[..i - usize::from(fstr && q != '`')].iter().rev().find(|c| !c.is_whitespace()).copied();
        let after = b[j + 1..].iter().find(|c| !c.is_whitespace()).copied();
        let indexed = before == Some('[');
        let open = if q == '`' { "${" } else { "{" };
        if fstr && body.contains(open) {
            let head = &body[..body.find(open).unwrap()];
            if let Some(p) = piece(head) {
                out.push((NamePiece::Prefix(p), indexed));
            }
            if let Some(k) = body.rfind('}') {
                if let Some(p) = piece(&body[k + 1..]) {
                    out.push((NamePiece::Suffix(p), indexed));
                }
            }
        } else if let Some(head) = body.strip_suffix("%s").or_else(|| body.strip_suffix("{}")) {
            if let Some(p) = piece(head) {
                out.push((NamePiece::Prefix(p), indexed));
            }
        } else if let Some(p) = piece(&body) {
            if after == Some('+') {
                out.push((NamePiece::Prefix(p), indexed));
            } else if before == Some('+') {
                out.push((NamePiece::Suffix(p), indexed));
            }
        }
        i = j + 1;
    }
    out
}

/// Rust functions a harness or the linker runs by an attribute: `#[bench]`, `#[no_mangle]`,
/// `#[export_name = ..]`, `#[proc_macro..]`, `#[tokio::main]`. Read from the lines above the item.
fn entry_attribute(g: &Graph, root: &Path, candidates: &[usize]) -> HashSet<usize> {
    let mut by_file: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for &i in candidates {
        let r = &g.syms[i];
        if r.file.ends_with(".rs") && r.sym.kind.is_callable() {
            by_file.entry(r.file.as_str()).or_default().push(i);
        }
    }
    let mut out = HashSet::new();
    for (file, ids) in by_file {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        for i in ids {
            let start = g.syms[i].sym.start_line as usize; // 1-based: lines[start - 1] is the item
            let mut k = start.saturating_sub(1);
            // the item's own first line too: an attribute may share it (`#[bench] fn f(..)`)
            let mut attrs = lines.get(k).map(|l| l.trim_start().to_string()).filter(|l| l.starts_with("#[")).unwrap_or_default();
            while k > 0 {
                k -= 1;
                let t = lines[k].trim_start();
                if t.starts_with("#[") || t.starts_with("//") {
                    attrs.push_str(t);
                } else {
                    break;
                }
            }
            if ["#[bench]", "no_mangle", "export_name", "#[proc_macro", "::main]", "::main("].iter().any(|a| attrs.contains(a)) {
                out.insert(i);
            }
        }
    }
    out
}

/// `protected` in Java / C++ / TypeScript: written before the name in the signature.
fn is_protected(s: &gitnext_parse::Symbol) -> bool {
    s.visibility.as_deref() == Some("protected")
        || s.signature.find(s.name.as_str()).is_some_and(|k| s.signature[..k].split_whitespace().any(|w| w == "protected"))
}

pub fn render_dead(d: &Dead) -> String {
    let mut s = format!("unreferenced, not public ({}): likely dead\n", d.likely.len());
    for l in &d.likely {
        let _ = writeln!(s, "  {l}");
    }
    let _ = writeln!(s, "public but unused inside the repo ({}): API or dead", d.public_unused.len());
    for l in d.public_unused.iter().take(40) {
        let _ = writeln!(s, "  {l}");
    }
    if d.public_unused.len() > 40 {
        let _ = writeln!(s, "  … +{}", d.public_unused.len() - 40);
    }
    s.push_str("(approximate: dynamic dispatch, macros, reflection and external users are invisible)\n");
    s
}

// ───────────────────────────── cycles ─────────────────────────────

/// Directory dependency cycles (strongly connected components of the dir graph).
pub fn cycles(g: &Graph) -> Vec<Vec<String>> {
    let dg = g.dir_graph();
    let nodes: Vec<&String> = dg.keys().chain(dg.values().flat_map(|m| m.keys())).collect::<BTreeSet<_>>().into_iter().collect();
    let idx: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let adj: Vec<Vec<usize>> = nodes
        .iter()
        .map(|n| dg.get(*n).map(|m| m.keys().filter_map(|k| idx.get(k.as_str()).copied()).collect()).unwrap_or_default())
        .collect();
    // Tarjan
    struct T<'a> {
        adj: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn strong(t: &mut T, v: usize) {
        t.index[v] = Some(t.next);
        t.low[v] = t.next;
        t.next += 1;
        t.stack.push(v);
        t.on[v] = true;
        for &w in &t.adj[v].clone() {
            if t.index[w].is_none() {
                strong(t, w);
                t.low[v] = t.low[v].min(t.low[w]);
            } else if t.on[w] {
                t.low[v] = t.low[v].min(t.index[w].unwrap());
            }
        }
        if Some(t.low[v]) == t.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = t.stack.pop() {
                t.on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            if comp.len() > 1 {
                t.out.push(comp);
            }
        }
    }
    let n = nodes.len();
    let mut t = T { adj: &adj, index: vec![None; n], low: vec![0; n], on: vec![false; n], stack: vec![], next: 0, out: vec![] };
    for v in 0..n {
        if t.index[v].is_none() {
            strong(&mut t, v);
        }
    }
    // Rust: a module and its submodules referencing each other is idiomatic, not a smell.
    let is_ancestor = |a: &str, b: &str| a.is_empty() || b.starts_with(&format!("{a}/"));
    let rust_only = |d: &str| g.files.iter().filter(|f| f.parsed && gitnext_index::dir_of(&f.path) == d).all(|f| f.path.ends_with(".rs"));
    t.out
        .into_iter()
        .filter(|c| {
            let names: Vec<&str> = c.iter().map(|&i| nodes[i].as_str()).collect();
            let nested = names.len() == 2 && (is_ancestor(names[0], names[1]) || is_ancestor(names[1], names[0]));
            !(nested && names.iter().all(|d| rust_only(d)))
        })
        .map(|c| {
            let mut v: Vec<String> = c.into_iter().map(|i| if nodes[i].is_empty() { "/".to_string() } else { nodes[i].clone() }).collect();
            v.sort();
            v
        })
        .collect()
}

pub fn render_cycles(g: &Graph, cs: &[Vec<String>]) -> String {
    if cs.is_empty() {
        return "no directory dependency cycles\n".into();
    }
    let dg = g.dir_graph();
    let mut s = format!("{} dependency cycles between directories:\n", cs.len());
    for c in cs {
        let _ = writeln!(s, "cycle: {}", c.join(" ⇄ "));
        for a in c {
            let ak = if a == "/" { "" } else { a.as_str() };
            if let Some(m) = dg.get(ak) {
                for b in c {
                    let bk = if b == "/" { "" } else { b.as_str() };
                    if let Some(n) = m.get(bk) {
                        let _ = writeln!(s, "  {a} → {b} ({n} refs)");
                    }
                }
            }
        }
    }
    s
}

// ───────────────────────────── entry points ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    #[serde(skip)]
    pub symbol: usize,
    pub kind: String,
    pub name: String,
    pub location: String,
    pub detail: String,
}

const ROUTE_MARKERS: &[&str] = &[".route(", ".get(", ".post(", ".put(", ".delete(", ".patch(", ".websocket(", "#[get(", "#[post(", "#[put(", "#[delete(", "#[patch(", "#[route("];
const MAIN_MARKERS: &[&str] = &["#[tokio::main", "#[actix_web::main", "#[async_std::main"];
const CLI_MARKERS: &[&str] = &["@click.command", "@click.group", ".command(", "@app.command"];

/// main functions, HTTP routes, CLI commands (clap / click / typer) and `__main__` scripts.
pub fn entrypoints(g: &Graph, root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut cache: HashMap<&str, Vec<String>> = HashMap::new();
    for (i, r) in g.syms.iter().enumerate() {
        let s = &r.sym;
        if s.test {
            continue;
        }
        let lines = cache.entry(r.file.as_str()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&r.file)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
        });
        // attributes / decorators directly above the item, plus the signature
        let mut head = s.signature.clone();
        let mut l = s.start_line as usize;
        while l > 1 {
            let prev = lines.get(l - 2).map(|x| x.trim()).unwrap_or("");
            if prev.starts_with("#[") || prev.starts_with('@') || prev.starts_with("///") {
                head.push(' ');
                head.push_str(prev);
                l -= 1;
            } else {
                break;
            }
        }
        let loc = format!("{}:{}", r.file, s.start_line);
        if s.kind.is_callable() && s.name == "main" && s.kind == Kind::Function {
            out.push(Entry { symbol: i, kind: "main".into(), name: g.short_name(i).into(), location: loc, detail: String::new() });
            continue;
        }
        if s.kind.is_callable() && MAIN_MARKERS.iter().any(|m| head.contains(m)) {
            out.push(Entry { symbol: i, kind: "main".into(), name: g.short_name(i).into(), location: loc, detail: "async runtime".into() });
            continue;
        }
        if s.kind.is_callable() {
            if let Some(m) = ROUTE_MARKERS.iter().find(|m| head.contains(*m)) {
                let detail = head
                    .split(m)
                    .nth(1)
                    .and_then(|x| x.split(['"', '\'']).nth(1))
                    .map(|p| format!("{} {p}", m.trim_matches(['.', '(', '#', '[']).to_uppercase()))
                    .unwrap_or_default();
                out.push(Entry { symbol: i, kind: "route".into(), name: g.short_name(i).into(), location: loc, detail });
                continue;
            }
            if CLI_MARKERS.iter().any(|m| head.contains(m)) {
                out.push(Entry { symbol: i, kind: "cli".into(), name: g.short_name(i).into(), location: loc, detail: String::new() });
                continue;
            }
        }
        // clap: #[derive(Subcommand)] enum → its variants are commands
        if s.kind == Kind::Enum && (head.contains("Subcommand") || head.contains("Parser")) && r.file.ends_with(".rs") {
            let v: Vec<String> = s.params.iter().map(|p| p.name.to_lowercase()).collect();
            out.push(Entry { symbol: i, kind: "cli".into(), name: g.short_name(i).into(), location: loc, detail: format!("commands: {}", v.join(", ")) });
        }
    }
    // Python scripts with `if __name__ == "__main__":`
    for f in g.files.iter().filter(|f| f.path.ends_with(".py")) {
        let text = std::fs::read_to_string(root.join(&f.path)).unwrap_or_default();
        if text.contains("__name__ == \"__main__\"") || text.contains("__name__ == '__main__'") {
            if let Some(i) = g.in_file(&f.path).into_iter().find(|&i| g.syms[i].sym.kind == Kind::Module) {
                if !out.iter().any(|e| e.location.starts_with(&f.path)) {
                    out.push(Entry { symbol: i, kind: "script".into(), name: f.path.clone(), location: format!("{}:1", f.path), detail: "__main__".into() });
                }
            }
        }
    }
    out
}

pub fn render_entrypoints(v: &[Entry]) -> String {
    if v.is_empty() {
        return "no entry points detected\n".into();
    }
    let mut s = String::new();
    for e in v {
        let d = if e.detail.is_empty() { String::new() } else { format!("  {}", e.detail) };
        let _ = writeln!(s, "{} {}  {}{d}", e.kind, e.name, e.location);
    }
    s
}

// ───────────────────────────── overview ─────────────────────────────

/// One screen for a human (or an agent's first call): what is here and what is going on.
pub fn overview(repo: Option<&Repo>, g: &Graph, root: &Path) -> String {
    let ctx = gitnext_docs::Ctx::new(root, g);
    let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut langs: BTreeMap<&str, usize> = BTreeMap::new();
    for f in g.files.iter().filter(|f| f.parsed) {
        *langs.entry(f.file_type.as_str()).or_default() += 1;
    }
    let n_syms = g.syms.iter().filter(|s| !(s.sym.kind == Kind::Module && s.sym.start_line == 1) && !s.sym.test).count();
    let n_tests = g.syms.iter().filter(|s| s.sym.test && s.sym.kind.is_callable()).count();
    let mut s = String::new();
    let _ = (&langs, n_syms, n_tests);
    let _ = writeln!(s, "{name} · {} · ≈{} tokens of source", g.counts().line(), gitnext_docs::fmt_k(ctx.source_tokens()));
    if let Some(p) = ctx.dir_purpose("") {
        let _ = writeln!(s, "{p}");
    }
    // areas
    let mut areas: BTreeMap<String, usize> = BTreeMap::new();
    for f in g.files.iter().filter(|f| f.parsed) {
        *areas.entry(area(&f.path)).or_default() += 1;
    }
    let _ = writeln!(s, "\nareas:");
    for (a, n) in &areas {
        let key = if a == "/" { String::new() } else { a.clone() };
        let purpose = ctx.dir_purpose(&key).or_else(|| ctx.dir_purpose(&format!("{key}/src"))).unwrap_or_default();
        let _ = writeln!(s, "  {a} ({n} code file{}){}", if *n == 1 { "" } else { "s" }, if purpose.is_empty() { String::new() } else { format!(" — {purpose}") });
    }
    let ent = entrypoints(g, root);
    if !ent.is_empty() {
        let _ = writeln!(s, "\nentry points:");
        for e in ent.iter().take(12) {
            let d = if e.detail.is_empty() { String::new() } else { format!(" ({})", e.detail) };
            let _ = writeln!(s, "  {} {}{d}  {}", e.kind, e.name, e.location);
        }
    }
    let libs = crate::libs::render_libraries(root, g, 10);
    if !libs.starts_with("no external") {
        let _ = writeln!(s, "\n{}", libs.trim_end());
    }
    let ly = gitnext_docs::layers(g);
    if !ly.uses.is_empty() {
        let _ = writeln!(s, "\nlayers (each uses only the ones below it):");
        let w = ly.layers.iter().flatten().map(|a| a.chars().count()).max().unwrap_or(4) + 2;
        for (k, layer) in ly.layers.iter().enumerate().rev() {
            let label = if k == 0 { "foundation" } else if k + 1 == ly.layers.len() { "top" } else { "middle" };
            for (j, a) in layer.iter().enumerate() {
                let uses = ly.uses.get(a).map(|v| format!("uses → {}", v.iter().take(4).map(|(t, n)| format!("{t} ({n})")).collect::<Vec<_>>().join(", "))).unwrap_or_default();
                let _ = writeln!(s, "  {:<11} {a:<w$}{uses}", if j == 0 { label } else { "" });
            }
        }
        for c in &ly.cycles {
            let _ = writeln!(s, "  cycle: {} use each other", c.join(" ⇄ "));
        }
    }
    let cyc = cycles(g);
    if let Some(repo) = repo {
        if let Ok(a) = activity(repo, 200) {
            if a.commits > 0 {
                let _ = writeln!(
                    s,
                    "\nactivity: {} commits {}…{} · most active: {} · authors: {}",
                    a.commits,
                    a.first_date,
                    a.last_date,
                    a.by_area.iter().take(3).map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", "),
                    a.by_author.iter().take(4).map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", ")
                );
                if let Some((id, d, who, msg)) = a.recent.first() {
                    let _ = writeln!(s, "last: {d} {id} {who}: {msg}");
                }
            }
            if a.commits == 0 {
                let _ = writeln!(s, "\nno commits yet · {} files not committed", a.uncommitted);
            } else if a.uncommitted > 0 {
                let n = crate::semantic_diff(repo, crate::DiffMode::Worktree).map(|d| d.iter().map(|f| f.changes.len()).sum::<usize>()).unwrap_or(0);
                let _ = writeln!(s, "working tree: {} changed paths, {n} symbol changes (gitnext diff)", a.uncommitted);
            }
        }
        if let Ok(h) = hotspots(Some(repo), g, 300, 3) {
            if h.iter().any(|x| x.commits > 0) {
                let _ = writeln!(s, "hotspots: {}", h.iter().map(|x| format!("{} ({} commits)", x.file, x.commits)).collect::<Vec<_>>().join(", "));
            }
        }
    }
    let d = dead(g, root);
    let _ = writeln!(
        s,
        "health: {} likely-dead symbols · {} directory cycles{}",
        d.likely.len(),
        cyc.len(),
        if cyc.is_empty() { String::new() } else { format!(" ({})", cyc.iter().map(|c| c.join("⇄")).collect::<Vec<_>>().join("; ")) }
    );
    s
}
