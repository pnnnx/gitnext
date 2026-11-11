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

/// How many tests are named by default: the ones most likely to fail when the symbol
/// breaks. The rest is covered by one command per test file.
pub const TOP_TESTS: usize = 10;

#[derive(Debug, Clone)]
pub struct TestRef {
    pub symbol: usize,
    pub how: String,
}

/// What is known about one candidate test (see [`tests_for`] for the order it gives).
#[derive(Default, Clone, Copy)]
struct Evidence {
    /// call-graph distance (1 = the test calls the symbol itself)
    depth: Option<usize>,
    /// the test's name contains the symbol's name
    named: bool,
    /// … and that name is more than one common word (`Print` is in many unrelated test names)
    distinctive: bool,
    /// it lives in the test file that mirrors the symbol's file (`command_test.go`)
    mirror: bool,
    /// words of the symbol's name that the test's name has too
    overlap: usize,
}

impl Evidence {
    /// Smaller = more likely to fail when the symbol breaks.
    fn tier(&self) -> u8 {
        match (self.depth, self.named) {
            (Some(1), true) => 0,
            (Some(1), false) => 1,
            (_, true) if self.distinctive => 2,
            (Some(_), _) | (None, true) => 3,
            (None, false) => 4,
        }
    }

    fn how(&self) -> String {
        let mut v: Vec<String> = Vec::new();
        match self.depth {
            Some(1) => v.push("direct".into()),
            Some(d) => v.push(format!("{d} hops")),
            None => {}
        }
        if self.named {
            v.push("named after it".into());
        }
        if self.mirror && (v.is_empty() || self.depth.is_some_and(|d| d > 1)) {
            v.push("its test file".into());
        }
        v.join(", ")
    }
}

/// Tests that exercise `i`, the most likely to fail first:
/// 1. tests that call it directly (those named after it before the others),
/// 2. tests named after it (the graph misses templates, decorators, reflection) — when its
///    name is distinctive: two words or more,
/// 3. tests that reach it through other functions (up to `depth` hops) — the fewer hops
///    and the more of its name they share, the earlier,
/// 4. the rest of its mirror test file, when all of the above found little.
pub fn tests_for(g: &Graph, i: usize, depth: usize) -> Vec<TestRef> {
    let mut found: BTreeMap<usize, Evidence> = BTreeMap::new();
    let lookup = |n: &gitnext_index::ImpactNode| g.find(&n.symbol.qualified).into_iter().find(|&x| g.syms[x].file == n.symbol.file && g.syms[x].sym.start_line == n.symbol.line);
    let mut reach = |idx: usize, d: usize| {
        let e = found.entry(idx).or_default();
        e.depth = Some(e.depth.map_or(d, |old| old.min(d)));
    };
    for n in g.impact(i, depth) {
        if !n.test || !n.certain {
            continue;
        }
        let Some(idx) = lookup(&n) else { continue };
        if !g.syms[idx].sym.kind.is_callable() {
            continue;
        }
        reach(idx, n.depth);
        // a test helper at the depth limit (`fn version(s)` in tests/util): the tests calling it
        // are past the limit but run the code just the same
        if n.depth == depth {
            for m in g.impact(idx, 3) {
                if !m.test || !m.certain {
                    continue;
                }
                if let Some(j) = lookup(&m) {
                    if g.syms[j].sym.kind.is_callable() {
                        reach(j, depth + m.depth);
                    }
                }
            }
        }
    }
    // the call graph misses dynamic calls (templates, decorators, trait objects, reflection);
    // conventions catch many of those: a test named after the symbol, a test file named after its file
    let r = &g.syms[i];
    let name = r.sym.name.to_lowercase().replace('_', "");
    let words = name_words(&r.sym.name);
    let mirror = mirror_test_files(&r.file);
    let by_name = name.len() >= 4 && !r.sym.test;
    let mut in_mirror = Vec::new();
    for (k, s) in g.syms.iter().enumerate() {
        if !s.sym.test || !s.sym.kind.is_callable() {
            continue;
        }
        let known = found.contains_key(&k);
        if !known && !(by_name && is_runnable_test(s)) {
            continue;
        }
        let tw = name_words(&s.sym.name);
        let named = by_name && contains_words(&tw, &words);
        let is_mirror = mirror.iter().any(|m| s.file.ends_with(m.as_str()));
        // (the type's name is no help: every method of `HelpFormatter` shares it)
        let overlap = words.iter().filter(|w| w.len() >= 3 && w.as_str() != "test" && tw.contains(w)).count();
        if known || named {
            let e = found.entry(k).or_default();
            (e.named, e.distinctive, e.mirror, e.overlap) = (named, words.len() > 1, is_mirror, overlap);
        } else if is_mirror {
            in_mirror.push((k, Evidence { depth: None, named: false, distinctive: false, mirror: true, overlap }));
        }
    }
    // the whole mirror file when the graph and names found little (dynamic dispatch: a CLI
    // runner, templates, decorators reach the code without a call the graph can see)
    if found.len() < 5 {
        found.extend(in_mirror);
    }
    let mut out: Vec<(usize, Evidence)> = found.into_iter().collect();
    // helpers in test files are not run by name: the tests that call them are in the list too
    out.retain(|(k, _)| is_runnable_test(&g.syms[*k]));
    out.sort_by_key(|(k, e)| (e.tier(), e.depth.unwrap_or(usize::MAX), std::cmp::Reverse(e.overlap), !e.mirror, g.syms[*k].file.as_str(), g.syms[*k].sym.start_line));
    out.into_iter().map(|(symbol, e)| TestRef { symbol, how: e.how() }).collect()
}

/// `testParseURL_basic` → ["test", "parse", "url", "basic"]: camelCase, snake_case, digits apart.
fn name_words(n: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let cs: Vec<char> = n.chars().collect();
    let mut cur = String::new();
    for (i, &c) in cs.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let prev = i.checked_sub(1).map(|j| cs[j]);
        let next = cs.get(i + 1).copied();
        let boundary = c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit() || (p.is_uppercase() && next.is_some_and(char::is_lowercase)));
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `hay` has `needle` as consecutive words.
fn contains_words(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Test files that by convention test `file`: `command.go` → `command_test.go`,
/// `Foo.java` → `FooTest.java`, `x.py` → `test_x.py`, `x.ts` → `x.test.ts`.
fn mirror_test_files(file: &str) -> Vec<String> {
    let base = file.rsplit('/').next().unwrap_or(file);
    let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
    match ext {
        "go" => vec![format!("/{stem}_test.go"), format!("{stem}_test.go")],
        "java" | "kt" => vec![format!("/{stem}Test.{ext}"), format!("/{stem}Tests.{ext}")],
        "py" => vec![format!("/test_{stem}.py"), format!("/{stem}_test.py")],
        "ts" | "tsx" | "js" | "jsx" | "mts" | "mjs" => ["test", "spec"].iter().flat_map(|k| [format!("/{stem}.{k}.ts"), format!("/{stem}.{k}.tsx"), format!("/{stem}.{k}.js")]).collect(),
        "rs" => vec![format!("/{stem}.rs"), format!("tests/{stem}.rs")],
        _ => vec![],
    }
}

/// A test the runner executes on its own (not a helper inside a test file).
fn is_runnable_test(s: &gitnext_index::SymbolRow) -> bool {
    let n = &s.sym.name;
    if s.file.ends_with(".go") {
        return n.starts_with("Test") || n.starts_with("Fuzz");
    }
    if s.file.ends_with(".py") {
        return n.starts_with("test");
    }
    if s.file.ends_with(".rs") {
        // a `#[test]` function takes no arguments; the helpers under tests/ do (`fn version(s: &str)`)
        return s.sym.params.is_empty();
    }
    if s.file.ends_with(".java") || s.file.ends_with(".kt") {
        // JUnit 4/5 annotations are part of the signature; JUnit 3 goes by name
        return n.starts_with("test") || ["@Test", "@ParameterizedTest", "@RepeatedTest", "@TestFactory", "@TestTemplate", "@Property"].iter().any(|a| s.sym.signature.contains(a));
    }
    true
}

/// Ready-to-run commands per language/package.
pub fn run_commands(g: &Graph, root: &Path, tests: &[TestRef]) -> Vec<String> {
    let mut cargo: BTreeMap<(String, Option<String>), Vec<String>> = BTreeMap::new();
    let mut pytest: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    let mut go: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut js: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut java: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
            // a test method needs its class in the node id (unittest.TestCase classes need not start with "Test")
            let class = r.sym.parent.as_deref().filter(|_| r.sym.kind == gitnext_parse::Kind::Method).and_then(|p| p.rsplit("::").next()).map(str::to_string);
            pytest.push(match class {
                Some(c) => format!("{}::{c}::{}", r.file, r.sym.name),
                None => format!("{}::{}", r.file, r.sym.name),
            });
        } else if r.file.ends_with(".go") {
            // only `func TestX(t *testing.T)` runs; helpers in _test.go files are reached through them
            if r.sym.name.starts_with("Test") || r.sym.name.starts_with("Fuzz") {
                let dir = r.file.rsplit_once('/').map(|(d, _)| format!("./{d}")).unwrap_or_else(|| ".".into());
                go.entry(dir).or_default().push(r.sym.name.clone());
            }
        } else if r.file.ends_with(".java") {
            // `Class#method` for a @Test method, the class otherwise
            let class = r.sym.parent.as_deref().and_then(|p| p.rsplit("::").next()).unwrap_or(&r.sym.name);
            let class = if r.sym.kind == gitnext_parse::Kind::Method { class } else { r.sym.name.as_str() };
            let entry = java.entry(class.to_string()).or_default();
            if r.sym.kind == gitnext_parse::Kind::Method {
                entry.push(r.sym.name.clone());
            }
        } else if gitnext_parse::family_of(&r.file) == Some("js") {
            let runner = js_runner(&r.file, &mut read);
            let files = js.entry(runner).or_default();
            if !files.contains(&r.file) {
                files.push(r.file.clone());
            }
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
    for (dir, names) in go {
        let mut names = names;
        names.sort();
        names.dedup();
        cmds.push(if names.len() > 15 { format!("go test {dir}") } else { format!("go test {dir} -run '^({})$'", names.join("|")) });
    }
    for (runner, files) in js {
        let shown: Vec<&str> = files.iter().take(10).map(String::as_str).collect();
        cmds.push(format!("{runner} {}", shown.join(" ")));
    }
    if !java.is_empty() {
        let gradle = ["build.gradle", "build.gradle.kts"].iter().any(|f| root.join(f).exists());
        let parts: Vec<String> = java
            .iter()
            .take(12)
            .map(|(class, ms)| {
                let mut ms = ms.clone();
                ms.sort();
                ms.dedup();
                match (gradle, ms.len()) {
                    (true, 0) | (true, 6..) => format!("--tests '*{class}'"),
                    (true, _) => ms.iter().map(|m| format!("--tests '*{class}.{m}'")).collect::<Vec<_>>().join(" "),
                    (false, 0) | (false, 6..) => class.clone(),
                    (false, _) => format!("{class}#{}", ms.join("+")),
                }
            })
            .collect();
        cmds.push(if gradle { format!("./gradlew test {}", parts.join(" ")) } else { format!("mvn -q test -Dtest='{}'", parts.join(",")) });
    }
    if !other.is_empty() {
        cmds.push(format!("# C/C++ tests: {}", other.join(", ")));
    }
    cmds
}

/// The test runner of the nearest package.json: `npx vitest run`, `npx jest`, `node --test`…
fn js_runner(file: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let mut dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    loop {
        let manifest = if dir.is_empty() { "package.json".to_string() } else { format!("{dir}/package.json") };
        if let Some(text) = read(&manifest) {
            let has = |w: &str| text.contains(&format!("\"{w}\"")) || text.contains(&format!("{w} "));
            let prefix = if dir.is_empty() { String::new() } else { format!("cd {dir} && ") };
            let r = if has("vitest") {
                "npx vitest run"
            } else if has("jest") {
                "npx jest"
            } else if has("ava") {
                "npx ava"
            } else if has("mocha") {
                "npx mocha"
            } else if has("@playwright/test") {
                "npx playwright test"
            } else if text.contains("node --test") {
                "node --test"
            } else {
                "npm test --"
            };
            return format!("{prefix}{r}");
        }
        if dir.is_empty() {
            return "npm test --".into();
        }
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
}

/// One command per test file / package holding `tests`: "run them all" without naming each.
pub fn file_commands(g: &Graph, root: &Path, tests: &[TestRef]) -> Vec<String> {
    const MAX: usize = 8; // more files than this: the plain suite command
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let mut cargo: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pytest: Vec<String> = Vec::new();
    let mut go: Vec<String> = Vec::new();
    let mut java: Vec<String> = Vec::new();
    let mut js: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut other: Vec<String> = Vec::new();
    let add = |v: &mut Vec<String>, x: String| {
        if !v.contains(&x) {
            v.push(x);
        }
    };
    for t in tests {
        let r = &g.syms[t.symbol];
        if r.file.ends_with(".rs") {
            let pkg = gitnext_parse::find_crate(&r.file, &mut read).map(|h| h.name.replace('_', "-")).unwrap_or_default();
            let segs: Vec<&str> = r.sym.qualified.split("::").collect();
            let what = if r.file.contains("tests/") {
                format!("--test {}", r.file.rsplit('/').next().unwrap_or("").trim_end_matches(".rs"))
            } else {
                // unit tests: everything in the test's module
                format!("-- {}::", segs[1..segs.len().saturating_sub(1)].join("::"))
            };
            add(cargo.entry(pkg).or_default(), what);
        } else if r.file.ends_with(".py") {
            add(&mut pytest, r.file.clone());
        } else if r.file.ends_with(".go") {
            add(&mut go, r.file.rsplit_once('/').map(|(d, _)| format!("./{d}")).unwrap_or_else(|| ".".into()));
        } else if r.file.ends_with(".java") {
            let class = if r.sym.kind == Kind::Method { r.sym.parent.as_deref().map(gitnext_parse::last_segment).unwrap_or(&r.sym.name) } else { r.sym.name.as_str() };
            add(&mut java, class.to_string());
        } else if gitnext_parse::family_of(&r.file) == Some("js") {
            add(js.entry(js_runner(&r.file, &mut read)).or_default(), r.file.clone());
        } else {
            add(&mut other, r.file.clone());
        }
    }
    let mut cmds = Vec::new();
    for (pkg, whats) in cargo {
        let p = if pkg.is_empty() { String::new() } else { format!(" -p {pkg}") };
        if whats.len() > MAX {
            cmds.push(format!("cargo test{p}"));
        } else {
            cmds.extend(whats.iter().map(|w| format!("cargo test{p} {w}")));
        }
    }
    if !pytest.is_empty() {
        cmds.push(if pytest.len() > MAX { "pytest".to_string() } else { format!("pytest {}", pytest.join(" ")) });
    }
    if !go.is_empty() {
        cmds.push(if go.len() > MAX { "go test ./...".to_string() } else { format!("go test {}", go.join(" ")) });
    }
    for (runner, files) in js {
        cmds.push(if files.len() > MAX { runner } else { format!("{runner} {}", files.join(" ")) });
    }
    if !java.is_empty() {
        let gradle = ["build.gradle", "build.gradle.kts"].iter().any(|f| root.join(f).exists());
        cmds.push(match (gradle, java.len() > MAX) {
            (true, true) => "./gradlew test".to_string(),
            (true, false) => format!("./gradlew test {}", java.iter().map(|c| format!("--tests '*{c}'")).collect::<Vec<_>>().join(" ")),
            (false, true) => "mvn -q test".to_string(),
            (false, false) => format!("mvn -q test -Dtest='{}'", java.join(",")),
        });
    }
    if !other.is_empty() {
        cmds.push(format!("# C/C++ tests: {}", other.join(", ")));
    }
    cmds
}

/// The tests to run first and the command for them, plus one command for every file with
/// the rest (`all`: every test by name instead).
pub fn render_tests_for(g: &Graph, root: &Path, i: usize, depth: usize, all: bool) -> String {
    let t = tests_for(g, i, depth);
    if t.is_empty() {
        return format!("no tests reach {} (depth {depth}) — it is untested by the call graph\n", g.short_name(i));
    }
    let top = if all { &t[..] } else { &t[..t.len().min(TOP_TESTS)] };
    let mut s = if top.len() < t.len() {
        format!("tests reaching {} ({}), the {} most likely to fail first:\n", g.short_name(i), t.len(), top.len())
    } else {
        format!("tests reaching {} ({}), most likely to fail first:\n", g.short_name(i), t.len())
    };
    for x in top {
        let _ = writeln!(s, "{}  {}:{} ({})", g.short_name(x.symbol), g.syms[x.symbol].file, g.syms[x.symbol].sym.start_line, x.how);
    }
    let _ = writeln!(s, "run:");
    for c in run_commands(g, root, top) {
        let _ = writeln!(s, "  {c}");
    }
    if top.len() < t.len() {
        let _ = writeln!(s, "all {} (whole test files; `--all` names each test):", t.len());
        for c in file_commands(g, root, &t) {
            let _ = writeln!(s, "  {c}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{contains_words, name_words};

    #[test]
    fn test_names_match_whole_words() {
        assert_eq!(name_words("testParseURL_basic2"), ["test", "parse", "url", "basic2"]);
        assert_eq!(name_words("TestCommandExecute"), ["test", "command", "execute"]);
        let w = |s: &str| name_words(s);
        assert!(contains_words(&w("test_coerce_path_result"), &w("coerce_path_result")));
        assert!(contains_words(&w("TestNamePadding"), &w("NamePadding")));
        assert!(!contains_words(&w("testClosest"), &w("close")));
        assert!(!contains_words(&w("closeAuthedConnections"), &w("closest")));
    }
}
