//! Libraries in words: what a function calls outside the repo, by package, with
//! the version the project declares (`react 18.2 (useState, useEffect)`), and
//! the project's whole stack for the overview and `gitnext libs`.

use gitnext_index::libs::{manifest_deps, match_dep};
use gitnext_index::{Dep, Graph, LibUse};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type DepCache = Mutex<HashMap<PathBuf, (std::time::Instant, Vec<Dep>)>>;

/// Dependencies declared in the repo's manifests (cached per repo for 10 s: the TUI asks on every move).
pub fn deps(root: &Path, g: &Graph) -> Vec<Dep> {
    static CACHE: OnceLock<DepCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((t, d)) = cache.lock().ok().and_then(|c| c.get(root).cloned()) {
        if t.elapsed().as_secs() < 10 {
            return d;
        }
    }
    let files: Vec<(String, String)> = g
        .files
        .iter()
        .filter(|f| gitnext_index::libs::is_manifest(&f.path))
        .filter_map(|f| Some((f.path.clone(), std::fs::read_to_string(root.join(&f.path)).ok()?)))
        .collect();
    let d = manifest_deps(&files);
    if let Ok(mut c) = cache.lock() {
        c.insert(root.to_path_buf(), (std::time::Instant::now(), d.clone()));
    }
    d
}

fn version(v: &str) -> String {
    v.trim_start_matches(['^', '~', '=', '>', '<', 'v', ' ']).split([',', ' ']).next().unwrap_or("").to_string()
}

/// `react 18.2.0`, `json (std)`, `left-pad (not declared)`.
pub fn label(l: &LibUse, deps: &[Dep]) -> String {
    if l.stdlib {
        return format!("{} (std)", l.name);
    }
    match match_dep(&l.name, deps) {
        Some(d) => {
            let v = version(&d.version);
            if v.is_empty() { l.name.clone() } else { format!("{} {v}", l.name) }
        }
        None if deps.is_empty() => l.name.clone(),
        None => format!("{} (not declared)", l.name),
    }
}

/// One line: the libraries a symbol calls, e.g. `react 18.2.0: useState, useEffect · node:fs (std): readFile`.
pub fn describe(root: &Path, g: &Graph, i: usize) -> Option<String> {
    let libs = g.libraries(i);
    if libs.is_empty() {
        return None;
    }
    let deps = deps(root, g);
    let parts: Vec<String> = libs
        .iter()
        .take(6)
        .map(|l| {
            let m: Vec<&str> = l.members.iter().take(4).map(String::as_str).collect();
            let more = l.members.len().saturating_sub(4);
            format!("{}: {}{}", label(l, &deps), m.join(", "), if more > 0 { format!(" +{more}") } else { String::new() })
        })
        .collect();
    Some(format!("{}{}", parts.join(" · "), if libs.len() > 6 { format!(" · +{} more", libs.len() - 6) } else { String::new() }))
}

/// The project's libraries: declared ones with versions, and how much the code uses each.
pub fn render_libraries(root: &Path, g: &Graph, limit: usize) -> String {
    let deps = deps(root, g);
    let usage = g.library_usage();
    let mut s = String::new();
    let (third, std): (Vec<_>, Vec<_>) = usage.iter().partition(|(l, _)| !l.stdlib);
    if !third.is_empty() {
        let _ = writeln!(s, "libraries used in code (calls):");
        for (l, n) in third.iter().take(limit) {
            let m: Vec<&str> = l.members.iter().take(5).map(String::as_str).collect();
            let _ = writeln!(s, "  {:<34} {n:>4}  {}", label(l, &deps), m.join(", "));
        }
        if third.len() > limit {
            let _ = writeln!(s, "  +{} more", third.len() - limit);
        }
    }
    if !std.is_empty() {
        let names: Vec<String> = std.iter().take(12).map(|(l, n)| format!("{} {n}", l.name)).collect();
        let _ = writeln!(s, "standard library: {}{}", names.join(", "), if std.len() > 12 { format!(" +{}", std.len() - 12) } else { String::new() });
    }
    // declared but never called from code: often build tools, types, plugins
    let used: Vec<&LibUse> = usage.iter().map(|(l, _)| l).collect();
    let idle: Vec<&Dep> = deps.iter().filter(|d| !d.dev && !used.iter().any(|l| match_dep(&l.name, std::slice::from_ref(d)).is_some())).collect();
    if !deps.is_empty() {
        let mut by_eco: std::collections::BTreeMap<&str, usize> = Default::default();
        for d in &deps {
            *by_eco.entry(d.ecosystem).or_default() += 1;
        }
        let eco: Vec<String> = by_eco.iter().map(|(e, n)| format!("{n} {e}")).collect();
        let _ = writeln!(s, "declared: {} ({} dev)", eco.join(", "), deps.iter().filter(|d| d.dev).count());
        if !idle.is_empty() {
            let names: Vec<String> = idle.iter().take(10).map(|d| d.name.clone()).collect();
            let _ = writeln!(s, "declared, not called from code: {}{}", names.join(", "), if idle.len() > 10 { format!(" +{}", idle.len() - 10) } else { String::new() });
        }
    }
    if s.is_empty() {
        s.push_str("no external libraries found\n");
    }
    s
}
