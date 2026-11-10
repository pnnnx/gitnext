//! Error / trace → code. Paste a compiler error, panic or traceback and get the
//! symbols it points at (and the code of the top frame) instead of grepping.
//! Plus `tests-for`: which tests exercise a symbol and how to run just those.

use gitnext_index::Graph;
use gitnext_parse::Kind;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Frame {
    pub file: String,
    pub line: u32,
    /// innermost symbol containing the line
    pub symbol: Option<usize>,
}

const CODE_EXT: &[&str] = &["rs", "py", "pyi", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx", "ipp", "inl"];

/// Map a path as printed by a tool (absolute, ./relative, site-packages…) to an indexed file.
fn match_file(g: &Graph, root: &Path, raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches(['"', '\'', '(', ')', '`']);
    let p = raw.strip_prefix(&format!("{}/", root.display())).unwrap_or(raw).trim_start_matches("./");
    if g.files.iter().any(|f| f.path == p) {
        return Some(p.to_string());
    }
    // longest indexed path that is a suffix of the printed one
    g.files
        .iter()
        .filter(|f| p.ends_with(&format!("/{}", f.path)) || p.ends_with(&f.path) && p.len() == f.path.len())
        .max_by_key(|f| f.path.len())
        .map(|f| f.path.clone())
}

fn innermost(g: &Graph, file: &str, line: u32) -> Option<usize> {
    g.in_file(file)
        .into_iter()
        .filter(|&i| {
            let s = &g.syms[i].sym;
            !(s.kind == Kind::Module && s.start_line == 1) && s.start_line <= line && line <= s.end_line
        })
        .min_by_key(|&i| g.syms[i].sym.end_line - g.syms[i].sym.start_line)
}

/// Extract `path:line` locations from free text (rustc, gcc/clang, pytest, Python tracebacks, panics).
pub fn frames(g: &Graph, root: &Path, text: &str) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::new();
    let mut push = |file: String, line: u32| {
        if !out.iter().any(|f| f.file == file && f.line == line) {
            let symbol = innermost(g, &file, line);
            out.push(Frame { file, line, symbol });
        }
    };
    for l in text.lines() {
        // Python: File "x.py", line 12, in f
        if let Some(rest) = l.trim_start().strip_prefix("File \"") {
            if let Some((path, tail)) = rest.split_once('"') {
                let n: String = tail.trim_start_matches(", line ").chars().take_while(|c| c.is_ascii_digit()).collect();
                if let (Some(f), Ok(n)) = (match_file(g, root, path), n.parse()) {
                    push(f, n);
                }
                continue;
            }
        }
        // generic: some/path.ext:LINE[:COL]
        for token in l.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '\'' || c == '"') {
            let mut parts = token.split(':');
            let Some(path) = parts.next() else { continue };
            let ext = path.rsplit('.').next().unwrap_or("");
            if !CODE_EXT.contains(&ext) {
                continue;
            }
            let Some(n) = parts.next().and_then(|x| x.parse::<u32>().ok()) else { continue };
            if let Some(f) = match_file(g, root, path) {
                push(f, n);
            }
        }
    }
    out
}

/// Names in `backticks` or 'quotes' that are symbols of this repo.
pub fn mentioned(g: &Graph, text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (open, close) in [('`', '`'), ('\'', '\''), ('"', '"')] {
        let mut rest = text;
        while let Some(a) = rest.find(open) {
            let after = &rest[a + 1..];
            let Some(b) = after.find(close) else { break };
            let word = after[..b].trim().trim_end_matches("()").trim_start_matches('&');
            if word.len() >= 3 && word.len() < 80 && word.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '.') {
                for i in g.find(word).into_iter().take(3) {
                    if !out.contains(&i) && !g.syms[i].sym.test {
                        out.push(i);
                    }
                }
            }
            rest = &after[b + 1..];
        }
    }
    out
}

pub fn render_locate(g: &Graph, root: &Path, text: &str, with_code: bool) -> String {
    let fr = frames(g, root, text);
    let men = mentioned(g, text);
    let mut s = String::new();
    if fr.is_empty() && men.is_empty() {
        return "no repo locations or symbols found in the text\n".into();
    }
    if !fr.is_empty() {
        let _ = writeln!(s, "locations ({}):", fr.len());
        for f in &fr {
            match f.symbol {
                Some(i) => {
                    let r = &g.syms[i];
                    let _ = writeln!(s, "{}:{} in {} {} (L{}-{})", f.file, f.line, r.sym.kind.as_str(), g.short_name(i), r.sym.start_line, r.sym.end_line);
                }
                None => {
                    let _ = writeln!(s, "{}:{} (top level)", f.file, f.line);
                }
            }
        }
    }
    let men: Vec<usize> = men.into_iter().filter(|i| !fr.iter().any(|f| f.symbol == Some(*i))).collect();
    if !men.is_empty() {
        let v: Vec<String> = men.iter().map(|&i| format!("{} ({}:{})", g.short_name(i), g.syms[i].file, g.syms[i].sym.start_line)).collect();
        let _ = writeln!(s, "mentioned: {}", v.join(", "));
    }
    if with_code {
        if let Some(i) = fr.iter().find_map(|f| f.symbol).or(men.first().copied()) {
            let _ = writeln!(s, "\ntop symbol:");
            s.push_str(&crate::render::read(g, root, &[i]));
        }
    }
    s
}

// ───────────────────────────── tests-for ─────────────────────────────

#[derive(Debug, Clone)]
pub struct TestRef {
    pub symbol: usize,
    pub how: String,
}

/// Tests that reach `i` through calls / type uses (up to `depth` hops).
pub fn tests_for(g: &Graph, i: usize, depth: usize) -> Vec<TestRef> {
    let mut out: Vec<TestRef> = Vec::new();
    for n in g.impact(i, depth) {
        if !n.test || !n.certain {
            continue;
        }
        let Some(idx) = g.find(&n.symbol.qualified).into_iter().find(|&x| g.syms[x].file == n.symbol.file && g.syms[x].sym.start_line == n.symbol.line) else { continue };
        if !g.syms[idx].sym.kind.is_callable() {
            continue;
        }
        let how = if n.depth == 1 { "direct".to_string() } else { format!("{} hops", n.depth) };
        out.push(TestRef { symbol: idx, how });
    }
    out
}

/// Ready-to-run commands per language/package.
pub fn run_commands(g: &Graph, root: &Path, tests: &[TestRef]) -> Vec<String> {
    let mut cargo: BTreeMap<(String, Option<String>), Vec<String>> = BTreeMap::new();
    let mut pytest: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    for t in tests {
        let r = &g.syms[t.symbol];
        if r.file.ends_with(".rs") {
            let pkg = gitnext_parse::find_crate(&r.file, &mut read).map(|h| h.name.replace('_', "-"));
            let integ = r.file.contains("tests/").then(|| r.file.rsplit('/').next().unwrap_or("").trim_end_matches(".rs").to_string());
            let segs: Vec<&str> = r.sym.qualified.split("::").collect();
            // unit tests: path inside the crate; integration tests: path inside the test file
            let filter = match &integ {
                Some(stem) => segs.iter().position(|s| s == stem).map(|p| segs[p + 1..].join("::")).unwrap_or_else(|| r.sym.name.clone()),
                None => segs[1..].join("::"),
            };
            cargo.entry((pkg.unwrap_or_default(), integ)).or_default().push(filter);
        } else if r.file.ends_with(".py") {
            let class = r.sym.parent.as_deref().filter(|p| p.rsplit("::").next().is_some_and(|c| c.starts_with("Test"))).map(|p| p.rsplit("::").next().unwrap().to_string());
            pytest.push(match class {
                Some(c) => format!("{}::{c}::{}", r.file, r.sym.name),
                None => format!("{}::{}", r.file, r.sym.name),
            });
        } else {
            other.push(format!("{}:{}", r.file, r.sym.start_line));
        }
    }
    let mut cmds = Vec::new();
    for ((pkg, integ), filters) in cargo {
        let mut c = String::from("cargo test");
        if !pkg.is_empty() {
            c.push_str(&format!(" -p {pkg}"));
        }
        if let Some(t) = integ {
            c.push_str(&format!(" --test {t}"));
        }
        c.push_str(&format!(" -- {}", filters.join(" ")));
        cmds.push(c);
    }
    if !pytest.is_empty() {
        cmds.push(format!("pytest {}", pytest.join(" ")));
    }
    if !other.is_empty() {
        cmds.push(format!("# C/C++ tests: {}", other.join(", ")));
    }
    cmds
}

pub fn render_tests_for(g: &Graph, root: &Path, i: usize, depth: usize) -> String {
    let t = tests_for(g, i, depth);
    if t.is_empty() {
        return format!("no tests reach {} (depth {depth}) — it is untested by the call graph\n", g.short_name(i));
    }
    let mut s = format!("tests reaching {} ({}):\n", g.short_name(i), t.len());
    for x in &t {
        let _ = writeln!(s, "{}  {}:{} ({})", g.short_name(x.symbol), g.syms[x.symbol].file, g.syms[x.symbol].sym.start_line, x.how);
    }
    let _ = writeln!(s, "run:");
    for c in run_commands(g, root, &t) {
        let _ = writeln!(s, "  {c}");
    }
    s
}
