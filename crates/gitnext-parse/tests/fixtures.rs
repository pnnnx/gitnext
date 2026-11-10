//! Golden tests for the extractors: every file under `tests/fixtures/` is parsed
//! and rendered as a readable list of symbols and edges, compared with
//! `<file>.expected`. After an intended change, regenerate with
//! `GITNEXT_BLESS=1 cargo test -p gitnext-parse --test fixtures` and review the diff.

use gitnext_parse::{analyze, Symbol};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn render_symbol(s: &Symbol, out: &mut String) {
    let vis = s.visibility.as_deref().map(|v| format!(" [{v}]")).unwrap_or_default();
    let mut flags = Vec::new();
    if s.test {
        flags.push("test");
    }
    if s.decl {
        flags.push("decl");
    }
    let flags = if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) };
    let _ = writeln!(out, "L{}-{} {} {}{vis}{flags}", s.start_line, s.end_line, s.kind.as_str(), s.qualified);
    let _ = writeln!(out, "    sig: {}", s.signature);
    if let Some(p) = &s.parent {
        let _ = writeln!(out, "    parent: {p}");
    }
    if let Some(t) = &s.trait_impl {
        let _ = writeln!(out, "    implements: {t}");
    }
    if !s.params.is_empty() {
        let ps: Vec<String> = s
            .params
            .iter()
            .map(|p| match &p.ty {
                t if t.is_empty() => p.name.clone(),
                t if t.starts_with('=') => format!("{} {t}", p.name),
                t => format!("{}: {t}", p.name),
            })
            .collect();
        let _ = writeln!(out, "    in: {}", ps.join(", "));
    }
    if let Some(r) = &s.returns {
        let _ = writeln!(out, "    out: {r}");
    }
    if let Some(d) = &s.summary {
        let _ = writeln!(out, "    doc: {d}");
    }
}

fn render(path: &Path, rel: &str) -> String {
    let src = std::fs::read_to_string(path).unwrap();
    let a = analyze(Path::new(rel), &src).unwrap().expect("language not recognized");
    let mut out = String::new();
    if let Some(d) = &a.module_doc {
        let _ = writeln!(out, "module doc: {}", d.lines().next().unwrap_or(""));
    }
    let _ = writeln!(out, "symbols:");
    for s in &a.symbols {
        render_symbol(s, &mut out);
    }
    let _ = writeln!(out, "edges:");
    let mut edges: Vec<(u32, String)> = a.edges.iter().map(|e| (e.line, format!("L{} {} {} -> {}", e.line, e.kind.as_str(), e.from, e.to))).collect();
    edges.sort();
    edges.dedup();
    let edges: Vec<String> = edges.into_iter().map(|e| e.1).collect();
    for e in edges {
        let _ = writeln!(out, "  {e}");
    }
    out
}

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut v = Vec::new();
    for lang in std::fs::read_dir(&root).unwrap() {
        for f in std::fs::read_dir(lang.unwrap().path()).unwrap() {
            let p = f.unwrap().path();
            if p.extension().is_some_and(|e| e != "expected") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

#[test]
fn fixtures_match_expected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bless = std::env::var_os("GITNEXT_BLESS").is_some();
    let mut failed = Vec::new();
    for p in fixtures() {
        let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let got = render(&p, &rel);
        let exp_path = PathBuf::from(format!("{}.expected", p.display()));
        if bless {
            std::fs::write(&exp_path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&exp_path).unwrap_or_default();
        if got != want {
            let diff: Vec<String> = diff_lines(&want, &got);
            failed.push(format!("{rel}:\n{}", diff.join("\n")));
        }
    }
    assert!(failed.is_empty(), "fixtures differ (GITNEXT_BLESS=1 to accept):\n\n{}", failed.join("\n\n"));
}

/// Minimal line diff for the failure message.
fn diff_lines(a: &str, b: &str) -> Vec<String> {
    let a: Vec<&str> = a.lines().collect();
    let b: Vec<&str> = b.lines().collect();
    let mut out = Vec::new();
    for l in &a {
        if !b.contains(l) {
            out.push(format!("- {l}"));
        }
    }
    for l in &b {
        if !a.contains(l) {
            out.push(format!("+ {l}"));
        }
    }
    out.truncate(60);
    out
}
