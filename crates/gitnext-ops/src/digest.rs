//! What a build or a test run said, in a few lines: what failed, where, and the message —
//! instead of the tool's whole output. A failing test suite prints hundreds of lines (one
//! vitest run with 35 failures: 843); an agent needs the first few failures, each with its
//! location and one or two lines of message, and how many more there are.
//!
//! Understood: rustc / cargo test, pytest, Python tracebacks, go build / go test (with panics),
//! tsc, vitest, jest, javac / maven (surefire), and `file:line: message` of gcc / clang and others.
//! Anything else falls back to the last lines of the output.

use std::fmt::Write as _;

/// Lines of message kept per failure, and characters per line.
const MESSAGE_LINES: usize = 4;
const LINE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Failure {
    /// The test's name, or the compiler's error code (`E0609`, `TS2551`), or `error`.
    pub what: String,
    /// Where it failed, as the tool printed the path.
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: Vec<String>,
    /// How many times this same failure came (the same compile error in many places).
    pub count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Digest {
    /// `tests`, `build`, or empty when the output was not recognised.
    pub kind: &'static str,
    pub failures: Vec<Failure>,
    /// The tool's own totals (`1 failed, 68 passed in 0.16s`).
    pub summary: Option<String>,
    /// Lines of the original output.
    pub lines: usize,
    /// The end of the output, kept when nothing was recognised.
    pub tail: Vec<String>,
}

fn clip(s: &str) -> String {
    let s = s.trim_end();
    if s.chars().count() > LINE_CHARS {
        format!("{}…", s.chars().take(LINE_CHARS).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Terminal colours and cursor moves out of a line.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' && it.peek() == Some(&'[') {
            it.next();
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

fn is_code_path(p: &str) -> bool {
    let Some((stem, ext)) = p.rsplit_once('.') else { return false };
    !stem.is_empty()
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
        && p.chars().all(|c| c.is_alphanumeric() || "/._-+@$[]~ \\".contains(c))
        && !p.contains("://")
}

/// `path.ext:12`, `path.ext:12:5`, `path.ext(12,5)`, `path.ext:[12,5]` at the start of `s`:
/// the path, the line, and the rest of the text.
pub(crate) fn location(s: &str) -> Option<(String, u32, &str)> {
    let digits = |t: &str| -> Option<(u32, usize)> {
        let n: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!n.is_empty()).then(|| n.parse().ok().map(|v| (v, n.len()))).flatten()
    };
    // the path ends where `:<digit>`, `(<digit>` or `:[<digit>` begins
    for (i, c) in s.char_indices() {
        if c != ':' && c != '(' {
            continue;
        }
        let path = &s[..i];
        if !is_code_path(path.trim()) {
            continue;
        }
        let mut rest = &s[i + 1..];
        let bracket = c == ':' && rest.starts_with('[');
        if bracket {
            rest = &rest[1..];
        }
        let Some((line, used)) = digits(rest) else { continue };
        let mut rest = &rest[used..];
        // column
        if let Some(r) = rest.strip_prefix([':', ',']) {
            if let Some((_, u)) = digits(r) {
                rest = &r[u..];
            }
        }
        if (c == '(' || bracket) && !rest.starts_with([')', ']']) {
            continue;
        }
        let rest = rest.trim_start_matches([')', ']']);
        return Some((path.trim().to_string(), line, rest));
    }
    None
}

fn push(out: &mut Vec<Failure>, what: &str, loc: Option<(String, u32)>, message: Vec<String>) {
    let message: Vec<String> = message.iter().map(|m| clip(m.trim_end())).filter(|m| !m.trim().is_empty()).take(MESSAGE_LINES).collect();
    let (file, line) = match loc {
        Some((f, l)) => (Some(f), Some(l)),
        None => (None, None),
    };
    out.push(Failure { what: what.trim().to_string(), file, line, message, count: 1 });
}

// ───────────────────────────── per tool ─────────────────────────────

fn rustc(lines: &[String], out: &mut Vec<Failure>) {
    for (i, l) in lines.iter().enumerate() {
        let Some(rest) = l.strip_prefix("error") else { continue };
        let (code, msg) = match rest.strip_prefix('[') {
            Some(r) => match r.split_once("]: ") {
                Some((c, m)) => (c, m),
                None => continue,
            },
            None => match rest.strip_prefix(": ") {
                Some(m) => ("error", m),
                None => continue,
            },
        };
        if ["could not compile", "aborting due to", "test failed", "process didn't exit", "failed to run custom build"].iter().any(|p| msg.starts_with(p)) {
            continue;
        }
        let loc = lines[i + 1..].iter().take(6).find_map(|n| n.trim_start().strip_prefix("--> ")).and_then(|t| location(t.trim())).map(|(f, n, _)| (f, n));
        if loc.is_none() && code == "error" {
            continue; // not rustc's: some other tool's line that starts with `error:`
        }
        push(out, code, loc, vec![msg.to_string()]);
    }
}

fn cargo_test(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.starts_with("test result:") && (summary.is_none() || l.contains("FAILED")) {
            *summary = Some(l.trim_start_matches("test result:").trim().to_string());
        }
        let name = l.strip_prefix("---- ").and_then(|r| r.strip_suffix(" stdout ----").or_else(|| r.strip_suffix(" stderr ----")));
        let Some(name) = name else {
            i += 1;
            continue;
        };
        let end = lines[i + 1..].iter().position(|n| n.starts_with("---- ") || n.trim() == "failures:").map_or(lines.len(), |p| i + 1 + p);
        let block = &lines[i + 1..end];
        let mut loc = None;
        let mut msg: Vec<String> = Vec::new();
        if let Some(p) = block.iter().position(|b| b.contains("panicked at ")) {
            let after = block[p].split_once("panicked at ").map_or("", |x| x.1);
            if let Some(q) = after.strip_prefix('\'') {
                // before 1.73: panicked at 'message', src/x.rs:1:2
                if let Some((m, at)) = q.rsplit_once("', ") {
                    msg.push(m.to_string());
                    loc = location(at.trim()).map(|(f, n, _)| (f, n));
                }
            } else {
                loc = location(after.trim().trim_end_matches(':')).map(|(f, n, _)| (f, n));
                msg.extend(block[p + 1..].iter().take_while(|b| !b.trim().is_empty() && !b.starts_with("stack backtrace") && !b.starts_with("note:")).cloned());
            }
        } else {
            msg.extend(block.iter().filter(|b| !b.trim().is_empty()).take(3).cloned());
        }
        push(out, name, loc, msg);
        i = end;
    }
}

fn pytest(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    let header = |l: &str| -> Option<String> {
        // `____ test_x ____`; a long name leaves one underscore on each side: `_ test_x[a long id] _`
        let t = l.trim();
        let inner = t.trim_matches('_');
        (t.starts_with('_') && t.ends_with('_') && inner.len() > 2 && inner.starts_with(' ') && inner.ends_with(' ')).then(|| inner.trim().to_string())
    };
    let e_line = |l: &str| l.strip_prefix('E').filter(|r| r.is_empty() || r.starts_with(' ')).map(|r| r.trim().to_string());
    let py_loc = |l: &str| location(l).filter(|(f, _, rest)| f.ends_with(".py") && rest.starts_with(": ")).map(|(f, n, _)| (f, n));
    let mut i = 0;
    let mut sections = 0;
    while i < lines.len() {
        let l = &lines[i];
        let t = l.trim().trim_matches('=').trim();
        if (t.contains(" passed") || t.contains(" failed") || t.contains(" error")) && t.contains(" in ") && t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            *summary = Some(t.to_string());
        }
        let Some(name) = header(l) else {
            i += 1;
            continue;
        };
        let end = lines[i + 1..].iter().position(|n| header(n).is_some() || n.starts_with("====")).map_or(lines.len(), |p| i + 1 + p);
        let block = &lines[i + 1..end];
        let mut msg: Vec<String> = block.iter().filter_map(|b| e_line(b)).filter(|m| !m.is_empty()).collect();
        // the source line that failed, marked `>`
        if let Some(src) = block.iter().rev().find_map(|b| b.strip_prefix('>').map(|r| r.trim().to_string())) {
            msg.insert(0, format!("> {src}"));
        }
        let loc = block.iter().rev().find_map(|b| py_loc(b));
        push(out, &name, loc, msg);
        sections += 1;
        i = end;
    }
    if sections > 0 {
        return;
    }
    // no sections: `--tb=no`, or the collection itself failed (a conftest that does not import)
    for l in lines {
        if let Some(r) = l.strip_prefix("FAILED ").or_else(|| l.strip_prefix("ERROR ")) {
            let (id, msg) = r.split_once(" - ").unwrap_or((r, ""));
            if id.contains(".py") {
                let name = id.rsplit("::").next().unwrap_or(id);
                push(out, name, id.split("::").next().filter(|f| f.ends_with(".py")).map(|f| (f.to_string(), 0)), vec![msg.to_string()]);
            }
        }
    }
    if out.is_empty() {
        let es: Vec<String> = lines.iter().filter_map(|l| e_line(l)).filter(|m| !m.is_empty()).collect();
        if let Some(first) = lines.iter().find(|l| l.contains("Error while loading") || l.contains("error while loading") || l.starts_with("ERROR collecting")) {
            // `File "/abs/x.py", line 457` inside the E lines is where the import broke
            let loc = es.iter().rev().find_map(|e| {
                let r = e.strip_prefix("File \"")?;
                let (p, tail) = r.split_once('"')?;
                let n: String = tail.trim_start_matches(", line ").chars().take_while(|c| c.is_ascii_digit()).collect();
                Some((p.to_string(), n.parse().ok()?))
            });
            let loc = loc.or_else(|| lines.iter().rev().find_map(|l| py_loc(l)));
            let keep = es.len().saturating_sub(3);
            push(out, first.trim().trim_end_matches('.'), loc, es[keep..].to_vec());
        }
    }
    for f in out.iter_mut() {
        if f.line == Some(0) {
            f.line = None;
        }
    }
}

fn python_traceback(lines: &[String], out: &mut Vec<Failure>) {
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].trim_start().starts_with("Traceback (most recent call last)") {
            i += 1;
            continue;
        }
        let mut loc = None;
        let mut j = i + 1;
        while j < lines.len() && (lines[j].starts_with(' ') || lines[j].starts_with('\t')) {
            if let Some(r) = lines[j].trim_start().strip_prefix("File \"") {
                if let Some((p, tail)) = r.split_once('"') {
                    let n: String = tail.trim_start_matches(", line ").chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = n.parse() {
                        loc = Some((p.to_string(), n));
                    }
                }
            }
            j += 1;
        }
        if let Some(last) = lines.get(j) {
            let (kind, msg) = last.split_once(": ").unwrap_or((last.as_str(), ""));
            push(out, kind, loc, vec![msg.to_string()]);
        }
        i = j + 1;
    }
}

fn go_test(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    let goroot = |p: &str| p.contains("/src/runtime/") || p.contains("/src/testing/") || p.contains("/pkg/mod/") || p.contains("/src/reflect/");
    let failed: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("FAIL\t")).filter_map(|r| r.split('\t').next()).collect();
    if !failed.is_empty() {
        *summary = Some(format!("FAIL {}", failed.join(", ")));
    }
    let mut i = 0;
    while i < lines.len() {
        let Some(name) = lines[i].trim_start().strip_prefix("--- FAIL: ") else {
            i += 1;
            continue;
        };
        let name = name.split(" (").next().unwrap_or(name);
        let mut loc = None;
        let mut msg: Vec<String> = Vec::new();
        let mut j = i + 1;
        // `    x_test.go:15: got 1, want 2`, possibly several, and their continuation lines
        while j < lines.len() && lines[j].starts_with([' ', '\t']) && !lines[j].trim_start().starts_with("--- ") {
            let t = lines[j].trim_start();
            match location(t).filter(|(f, _, rest)| f.ends_with(".go") && rest.starts_with(": ")) {
                Some((f, n, rest)) => {
                    if loc.is_none() {
                        loc = Some((f, n));
                    }
                    msg.push(rest[2..].to_string());
                }
                None => msg.push(t.to_string()),
            }
            j += 1;
        }
        if msg.is_empty() && lines.get(j).is_some_and(|l| l.starts_with("panic:")) {
            // a panic: the message, and the first frame that is not Go's own
            msg.push(lines[j].trim_start_matches("panic:").trim().trim_end_matches("[recovered]").trim().to_string());
            let mut k = j + 1;
            while k < lines.len() && !lines[k].starts_with("FAIL") && !lines[k].starts_with("--- ") {
                if lines[k].starts_with('\t') && lines[k].contains(".go:") && !goroot(&lines[k]) {
                    let t = lines[k].trim().split(" +0x").next().unwrap_or("");
                    if let Some((f, n, _)) = location(t) {
                        loc = Some((f, n));
                        break;
                    }
                }
                k += 1;
            }
            j = k;
        }
        push(out, name, loc, msg);
        i = j.max(i + 1);
    }
}

fn tsc(lines: &[String], out: &mut Vec<Failure>) {
    for l in lines {
        let Some((f, n, rest)) = location(l.trim_start()) else { continue };
        let Some(r) = rest.strip_prefix(": error ") else { continue };
        let (code, msg) = r.split_once(": ").unwrap_or(("error", r));
        push(out, code, Some((f, n)), vec![msg.to_string()]);
    }
}

fn vitest(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    let mut found: Vec<Failure> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if let Some(r) = t.strip_prefix("Tests ") {
            if r.contains("failed") || r.contains("passed") {
                *summary = Some(r.trim().to_string());
            }
        }
        let Some(r) = t.strip_prefix("FAIL ").map(str::trim).filter(|r| r.contains(" > ")) else {
            i += 1;
            continue;
        };
        let (file, name) = r.split_once(" > ").unwrap_or((r, r));
        let end = lines[i + 1..].iter().position(|n| n.trim().starts_with("FAIL ") || n.contains("⎯⎯⎯⎯")).map_or(lines.len(), |p| i + 1 + p);
        let block = &lines[i + 1..end];
        let msg: Vec<String> = block.iter().map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).take(1).collect();
        let loc = block.iter().find_map(|b| b.trim().strip_prefix("❯ ").and_then(|t| location(t.trim()))).map(|(f, n, _)| (f, n));
        let had_loc = loc.is_some();
        push(&mut found, name, loc.or_else(|| is_code_path(file).then(|| (file.to_string(), 0))), msg);
        if !had_loc {
            if let Some(f) = found.last_mut() {
                f.line = None;
            }
        }
        i = end;
    }
    // tests that fail with the same error are printed as several `FAIL` lines over one message
    for k in (0..found.len().saturating_sub(1)).rev() {
        if found[k].message.is_empty() {
            found[k].message = found[k + 1].message.clone();
            if found[k].line.is_none() && found[k].file == found[k + 1].file {
                found[k].line = found[k + 1].line;
            }
        }
    }
    out.extend(found);
}

fn jest(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    if let Some(r) = lines.iter().find_map(|l| l.trim().strip_prefix("Tests:")) {
        *summary = Some(r.trim().to_string());
    }
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let Some(name) = t.strip_prefix("● ").filter(|n| n.contains(" › ")) else {
            i += 1;
            continue;
        };
        let end = lines[i + 1..].iter().position(|n| n.trim().starts_with("● ")).map_or(lines.len(), |p| i + 1 + p);
        let block = &lines[i + 1..end];
        let msg: Vec<String> = block.iter().map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).take(2).collect();
        let loc = block
            .iter()
            .filter(|b| b.trim_start().starts_with("at ") && !b.contains("node_modules"))
            .find_map(|b| b.rsplit_once('(').and_then(|(_, p)| location(p)).or_else(|| location(b.trim_start().trim_start_matches("at ").trim())))
            .map(|(f, n, _)| (f, n));
        push(out, name, loc, msg);
        i = end;
    }
}

fn maven(lines: &[String], out: &mut Vec<Failure>, summary: &mut Option<String>) {
    let body = |l: &str| l.strip_prefix("[ERROR]").map(|r| r.strip_prefix(' ').unwrap_or(r).to_string());
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.contains("Tests run:") && l.contains("Failures:") && !l.contains(" -- in ") && !l.contains("Time elapsed") {
            *summary = Some(l.trim_start_matches("[ERROR]").trim_start_matches("[INFO]").trim_start_matches("[WARNING]").trim().to_string());
        }
        let Some(b) = body(l) else {
            i += 1;
            continue;
        };
        // a compile error: /abs/X.java:[12,5] message
        if let Some((f, n, rest)) = location(b.trim()).filter(|(f, _, _)| f.ends_with(".java") || f.ends_with(".kt")) {
            let mut msg = vec![rest.trim().to_string()];
            let mut j = i + 1;
            while let Some(more) = lines.get(j).and_then(|x| body(x)).filter(|m| m.starts_with("  ") && (m.contains("symbol:") || m.contains("location:"))) {
                msg.push(more.trim().to_string());
                j += 1;
            }
            push(out, "error", Some((f, n)), msg);
            i = j;
            continue;
        }
        // a failed test: `pkg.Class.method -- Time elapsed: 0.01 s <<< FAILURE!` (or `method(pkg.Class)  Time elapsed…`)
        if b.contains("<<< FAILURE!") || b.contains("<<< ERROR!") {
            if b.trim_start().starts_with("Tests run:") {
                i += 1;
                continue;
            }
            let head = b.split(" -- ").next().unwrap_or(&b).split("  Time elapsed").next().unwrap_or(&b).trim();
            let (class, name) = match head.split_once('(') {
                Some((m, c)) => (c.trim_end_matches(')').rsplit('.').next().unwrap_or("").to_string(), m.to_string()),
                None => {
                    let mut it = head.rsplitn(3, '.');
                    let m = it.next().unwrap_or(head).to_string();
                    (it.next().unwrap_or("").to_string(), m)
                }
            };
            let msg: Vec<String> = lines.get(i + 1).filter(|m| !m.starts_with('\t') && !m.trim().is_empty()).map(|m| vec![m.trim().to_string()]).unwrap_or_default();
            let frame = lines[i + 1..]
                .iter()
                .take_while(|x| !x.starts_with("[ERROR]") && !x.starts_with("[INFO]"))
                .filter(|x| x.trim_start().starts_with("at "))
                .filter_map(|x| x.rsplit_once('(').and_then(|(_, p)| location(p.trim_end_matches(')'))))
                .map(|(f, n, _)| (f, n))
                .collect::<Vec<_>>();
            // the frame in the test's own class, else the first one
            let loc = frame.iter().find(|(f, _)| !class.is_empty() && f.trim_end_matches(".java") == class).or(frame.first()).cloned();
            push(out, &if class.is_empty() { name } else { format!("{class}.{name}") }, loc, msg);
        }
        i += 1;
    }
}

/// `path:line[:col]: [error: ]message` — gcc, clang, go build, javac and many others.
fn generic(lines: &[String], out: &mut Vec<Failure>) {
    for l in lines {
        let t = l.trim_start();
        let Some((f, n, rest)) = location(t) else { continue };
        let Some(msg) = rest.strip_prefix(": ").or_else(|| rest.strip_prefix(' ')) else { continue };
        let lower = msg.to_ascii_lowercase();
        if lower.starts_with("warning") || lower.starts_with("note") || lower.starts_with("in ") || msg.trim().is_empty() {
            continue;
        }
        let msg = msg.trim_start_matches("fatal ").trim_start_matches("error: ").trim_start_matches("error ");
        push(out, "error", Some((f, n)), vec![msg.to_string()]);
    }
}

// ───────────────────────────── the digest ─────────────────────────────

pub fn digest(text: &str) -> Digest {
    let lines: Vec<String> = text.lines().map(plain).collect();
    let mut d = Digest { lines: lines.len(), ..Default::default() };
    let mut tests: Vec<Failure> = Vec::new();
    let mut build: Vec<Failure> = Vec::new();
    cargo_test(&lines, &mut tests, &mut d.summary);
    pytest(&lines, &mut tests, &mut d.summary);
    go_test(&lines, &mut tests, &mut d.summary);
    vitest(&lines, &mut tests, &mut d.summary);
    jest(&lines, &mut tests, &mut d.summary);
    maven(&lines, &mut tests, &mut d.summary);
    // maven's compile errors came through the test pass: they are the build's
    let (mvn_build, rest): (Vec<Failure>, Vec<Failure>) = tests.into_iter().partition(|f| f.what == "error");
    tests = rest;
    build.extend(mvn_build);
    rustc(&lines, &mut build);
    tsc(&lines, &mut build);
    if tests.is_empty() && build.is_empty() {
        python_traceback(&lines, &mut tests);
    }
    if tests.is_empty() && build.is_empty() {
        generic(&lines, &mut build);
    }
    // the same compile error in many places is one line with a count
    let mut merged: Vec<Failure> = Vec::new();
    for f in build {
        let key = |x: &Failure| (x.what.clone(), x.message.first().map(|m| m.chars().filter(|c| !c.is_ascii_digit()).collect::<String>()));
        match merged.iter_mut().find(|m| key(m) == key(&f)) {
            Some(m) => m.count += 1,
            None => merged.push(f),
        }
    }
    // one test reported twice (a summary line and its section) is one failure, and the cases of
    // one parametrized test (`test_x[1-2]`, `test_x[2-3]`) are one failure with a count
    let mut seen: Vec<Failure> = Vec::new();
    let mut ids: Vec<Vec<String>> = Vec::new();
    for mut f in tests {
        let id = f.what.clone();
        if let Some((base, _)) = f.what.split_once('[').filter(|(b, _)| !b.is_empty() && f.what.ends_with(']')) {
            f.what = base.to_string();
        }
        match seen.iter().position(|s| s.what == f.what && s.file == f.file) {
            Some(k) => {
                if !ids[k].contains(&id) {
                    ids[k].push(id);
                    seen[k].count += 1;
                }
            }
            None => {
                seen.push(f);
                ids.push(vec![id]);
            }
        }
    }
    if !seen.is_empty() {
        d.kind = "tests";
        d.failures = seen;
        d.failures.extend(merged);
    } else if !merged.is_empty() {
        d.kind = "build";
        d.failures = merged;
    } else {
        let kept: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        d.tail = kept[kept.len().saturating_sub(12)..].iter().map(|l| clip(l)).collect();
    }
    d
}

/// The digest as text: the totals, the first `max` failures (name, place, message), and how
/// many more there are — by name for tests.
pub fn render(d: &Digest, max: usize) -> String {
    let mut s = String::new();
    if d.failures.is_empty() {
        if d.tail.is_empty() {
            return "no output\n".into();
        }
        let _ = writeln!(s, "not recognised as build or test output; its last lines ({} of {}):", d.tail.len(), d.lines);
        for l in &d.tail {
            let _ = writeln!(s, "  {l}");
        }
        return s;
    }
    let total: usize = d.failures.iter().map(|f| f.count).sum();
    let head = match d.kind {
        "tests" => format!("tests: {total} failed"),
        _ => format!("build: {}", crate::changes::plural(total, "error")),
    };
    match &d.summary {
        Some(sum) => {
            let _ = writeln!(s, "{head} — {sum}");
        }
        None => {
            let _ = writeln!(s, "{head}");
        }
    }
    for f in d.failures.iter().take(max) {
        let at = match (&f.file, f.line) {
            (Some(p), Some(n)) => format!("  {p}:{n}"),
            (Some(p), None) => format!("  {p}"),
            _ => String::new(),
        };
        let times = match (f.count, d.kind) {
            (0 | 1, _) => String::new(),
            (n, "tests") => format!("  ({n} cases)"),
            (n, _) => format!("  (×{n})"),
        };
        let _ = writeln!(s, "✗ {}{at}{times}", f.what);
        for m in &f.message {
            let _ = writeln!(s, "    {m}");
        }
    }
    let rest = &d.failures[d.failures.len().min(max)..];
    if !rest.is_empty() {
        let names: Vec<String> = rest.iter().take(6).map(|f| if f.what.chars().count() > 48 { format!("{}…", f.what.chars().take(48).collect::<String>()) } else { f.what.clone() }).collect();
        let more = if rest.len() > names.len() { format!(" (+{})", rest.len() - names.len()) } else { String::new() };
        let _ = writeln!(s, "… and {} more: {}{more}", rest.len(), names.join(", "));
    }
    s
}
