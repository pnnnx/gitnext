//! Symbol-level edits: the agent sends only the new code of one symbol (or a new
//! name), gitnext finds the exact range from the index, re-indents, checks that
//! the file still parses and writes it. No need to read the file first.

use anyhow::{anyhow, bail, Result};
use gitnext_index::semdiff::diff_sources_in;
use gitnext_index::Graph;
use gitnext_parse::{last_segment, syntax_errors, EdgeKind};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct EditResult {
    pub file: String,
    /// New line range of the edited / inserted code.
    pub lines: (usize, usize),
    /// Symbol-level effect, e.g. `~ method Task::complete: … → …`.
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Replace,
    InsertAfter,
    Delete,
}

fn read_lines(root: &Path, file: &str) -> Result<(String, Vec<String>)> {
    let text = std::fs::read_to_string(root.join(file)).map_err(|e| anyhow!("{file}: {e}"))?;
    let lines = text.lines().map(str::to_string).collect();
    Ok((text, lines))
}

/// Start line (1-based) including doc comments / attributes / decorators above the item.
pub fn doc_start(lines: &[String], start: usize) -> usize {
    let mut a = start;
    while a > 1 {
        let prev = lines[a - 2].trim_start();
        // the end of a block comment: the whole comment belongs to the item, whatever its lines
        // start with (a Javadoc without a `*` on every line)
        if prev.trim_end().ends_with("*/") && !prev.contains("/*") {
            match (1..a - 1).rev().take(400).find(|&k| lines[k - 1].contains("/*")) {
                Some(open) if lines[open - 1].trim_start().starts_with("/*") => {
                    a = open;
                    continue;
                }
                _ => break, // the comment begins after code on its line: not this item's
            }
        }
        let is_doc = prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("#[") || prev.starts_with('@')
            || prev.starts_with("/**") || prev.starts_with("* ") || prev == "*" || prev.starts_with("*/")
            || (prev.starts_with("//") && !prev.is_empty());
        if is_doc {
            a -= 1
        } else {
            break;
        }
    }
    a
}

fn starts_with_doc(code: &str) -> bool {
    let first = code.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start();
    first.starts_with("///") || first.starts_with("//") || first.starts_with("#[") || first.starts_with('@') || first.starts_with("/*")
}

/// Move `code` so that its first line stands at `indent` (agents send code flush-left); the other
/// lines keep their place relative to the first — also one that is indented less than it, so that
/// a symbol replaced with its own code is the same text.
fn reindent(code: &str, indent: &str) -> Vec<String> {
    let lines: Vec<&str> = code.trim_end_matches('\n').lines().collect();
    let own = |l: &str| l.len() - l.trim_start().len();
    let first_line = lines.iter().find(|l| !l.trim().is_empty()).copied().unwrap_or("");
    let first = own(first_line);
    // already where it belongs (the code as `read` showed it): not a character is touched — a file
    // that mixes tabs and spaces has no "less indented" to compute
    if &first_line[..first] == indent {
        return lines.iter().map(|l| l.to_string()).collect();
    }
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else if own(l) >= first {
                format!("{indent}{}", &l[first..])
            } else {
                let keep = indent.len().saturating_sub(first - own(l));
                format!("{}{}", indent.get(..keep).unwrap_or(indent), l.trim_start())
            }
        })
        .collect()
}

fn write_checked(root: &Path, file: &str, old: &str, new: &str, force: bool) -> Result<Vec<String>> {
    let before = syntax_errors(Path::new(file), old).len();
    let errs = syntax_errors(Path::new(file), new);
    if errs.len() > before && !force {
        let (l, c) = errs[0];
        bail!("edit rejected: {file} would not parse (first error at {l}:{c}); nothing written. Fix the code or pass force.");
    }
    std::fs::write(root.join(file), new)?;
    let mut read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let hint = gitnext_parse::find_crate(file, &mut read);
    let d = diff_sources_in(file, Some(old), Some(new), hint.as_ref());
    Ok(d.changes.iter().map(|c| c.line()).collect())
}

fn join(lines: &[String], trailing_newline: bool) -> String {
    let mut s = lines.join("\n");
    if trailing_newline {
        s.push('\n');
    }
    s
}

/// Replace / insert after / delete one symbol.
pub fn edit_symbol(root: &Path, g: &Graph, i: usize, kind: EditKind, code: &str, force: bool) -> Result<EditResult> {
    let r = &g.syms[i];
    let (old_text, mut lines) = read_lines(root, &r.file)?;
    let nl = old_text.ends_with('\n');
    let (start, end) = (r.sym.start_line as usize, r.sym.end_line as usize);
    if start == 0 || end > lines.len() || start > end {
        bail!("index is stale for {} (lines {start}-{end}); re-run `gitnext index`", r.file);
    }
    let indent: String = lines[start - 1].chars().take_while(|c| c.is_whitespace()).collect();
    let dstart = doc_start(&lines, start);
    let new_range = match kind {
        EditKind::Replace => {
            if code.trim().is_empty() {
                bail!("empty code; use delete to remove a symbol");
            }
            let from = if starts_with_doc(code) { dstart } else { start };
            let new_lines = reindent(code, &indent);
            let n = new_lines.len();
            lines.splice(from - 1..end, new_lines);
            (from, from + n - 1)
        }
        EditKind::InsertAfter => {
            let mut new_lines = vec![String::new()];
            new_lines.extend(reindent(code, &indent));
            let n = new_lines.len();
            lines.splice(end..end, new_lines);
            (end + 2, end + n)
        }
        EditKind::Delete => {
            let mut to = end;
            // swallow one blank line after the item
            if to < lines.len() && lines[to].trim().is_empty() {
                to += 1;
            }
            lines.drain(dstart - 1..to);
            (dstart, dstart.saturating_sub(1))
        }
    };
    let new_text = join(&lines, nl);
    let changes = write_checked(root, &r.file, &old_text, &new_text, force)?;
    let tool = match kind { EditKind::Replace => "replace", EditKind::InsertAfter => "insert_after", EditKind::Delete => "delete" };
    crate::oplog::record(root, tool, &r.sym.qualified, &[(r.file.clone(), old_text, new_text)]);
    Ok(EditResult { file: r.file.clone(), lines: new_range, changes })
}

#[derive(Debug, Serialize, Default)]
pub struct RenameReport {
    pub old: String,
    pub new: String,
    /// `file:line` of every rewritten occurrence.
    pub changed: Vec<String>,
    /// References that might be this symbol but resolved ambiguously — left untouched.
    pub skipped: Vec<String>,
    /// Lines that still contain the old name as a word after the rename (strings,
    /// comments, dynamic access): to check by hand.
    pub leftover: Vec<String>,
    /// Overriding methods in subclasses, renamed with it.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub overrides: Vec<String>,
    /// `--text`: leftover lines that were rewritten too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text_changed: Vec<String>,
    pub files: usize,
    pub dry_run: bool,
}

/// `rename --text`: also rewrite the old name where it is only text (comments,
/// strings, docs) — on the lines the rename reported as leftover. Whole words only.
pub fn rename_leftover_text(root: &Path, r: &mut RenameReport) -> Result<()> {
    let old = r.old.rsplit("::").next().unwrap_or(&r.old).rsplit('.').next().unwrap_or(&r.old).to_string();
    let new = r.new.rsplit("::").next().unwrap_or(&r.new).to_string();
    let mut by_file: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for x in &r.leftover {
        let mut it = x.splitn(3, ':');
        if let (Some(f), Some(Ok(l))) = (it.next(), it.next().map(str::parse::<usize>)) {
            by_file.entry(f.to_string()).or_default().push(l);
        }
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    // `empty`, `release`, `close` are also English: in prose only where written as code
    // (`empty()`, `Type::empty`, `#empty`, `` `empty` ``); `coerce_path_result` / `isAsciiLetter` anywhere
    let distinctive = old.contains('_') || old.chars().skip(1).any(|c| c.is_uppercase() || c.is_ascii_digit());
    let mut done = Vec::new();
    for (f, lines) in by_file {
        let path = root.join(&f);
        let text = std::fs::read_to_string(&path)?;
        // only inside comments and string literals: a word in code is some other symbol
        // (`fmt.Print` is not `Command.Print`), and that is not ours to change
        let spans = gitnext_parse::text_spans(Path::new(&f), &text);
        let dynamic = f.ends_with(".py") || gitnext_parse::family_of(&f) == gitnext_parse::family_of("x.js");
        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let mut out: Vec<String> = text.split('\n').map(str::to_string).collect();
        for l in lines {
            let line_start = starts.get(l - 1).copied().unwrap_or(0);
            let Some(line) = out.get_mut(l - 1) else { continue };
            let mut res = String::with_capacity(line.len());
            let mut rest = line.as_str();
            let mut changed = false;
            let mut consumed = 0usize;
            while let Some(k) = rest.find(&old) {
                let before = if k > 0 { rest[..k].chars().next_back() } else { res.chars().next_back() };
                let after = rest[k + old.len()..].chars().next();
                let at = line_start + consumed + k;
                let in_text = spans.as_ref().is_none_or(|v| v.iter().any(|&(a, b)| a <= at && at + old.len() <= b));
                let pre = &rest[..k];
                let code_shaped = distinctive
                    || matches!(after, Some('(' | '`'))
                    || matches!(before, Some('`' | '#' | '.'))
                    // the whole literal is the name: `getattr(x, 'detect')`, `obj["detect"]` — in Python / JS;
                    // in Go / Java / Rust that is data (`cmd.Print("Print")` prints a word)
                    || (dynamic && matches!(before, Some('"' | '\'')) && before == after)
                    || pre.ends_with("::")
                    || pre.ends_with("->");
                let in_text = in_text && code_shaped;
                res.push_str(&rest[..k]);
                consumed += k + old.len();
                if !word(before) && !word(after) && in_text {
                    res.push_str(&new);
                    changed = true;
                } else {
                    res.push_str(&old);
                }
                rest = &rest[k + old.len()..];
            }
            res.push_str(rest);
            if changed {
                *line = res;
                done.push(format!("{f}:{l}"));
            }
        }
        if !r.dry_run {
            let new_text = out.join("\n");
            std::fs::write(&path, &new_text)?;
            crate::oplog::record(root, "rename --text", &format!("{} → {}", r.old, r.new), &[(f.clone(), text, new_text)]);
        }
    }
    r.leftover.retain(|x| !done.iter().any(|d| x.starts_with(&format!("{d}:"))));
    r.text_changed = done;
    Ok(())
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Byte offsets of whole-word occurrences of `word` in `line`.
fn word_hits(line: &str, word: &str) -> Vec<usize> {
    let b = line.as_bytes();
    line.match_indices(word)
        .map(|(i, _)| i)
        .filter(|&i| (i == 0 || !is_ident(b[i - 1])) && (i + word.len() >= b.len() || !is_ident(b[i + word.len()])))
        .collect()
}

/// Line numbers (1-based) of a Python `__all__ = [...]` / `(...)` assignment.
fn dunder_all_lines(lines: &[String]) -> std::collections::HashSet<usize> {
    let mut out = std::collections::HashSet::new();
    let mut depth: i32 = 0;
    let mut inside = false;
    for (n, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if !inside && (t.starts_with("__all__ =") || t.starts_with("__all__=") || t.starts_with("__all__ +=")) {
            inside = true;
            depth = 0;
        }
        if inside {
            out.insert(n + 1);
            depth += l.matches(['[', '(']).count() as i32 - l.matches([']', ')']).count() as i32;
            if depth <= 0 {
                inside = false;
            }
        }
    }
    out
}

/// Rename a symbol at its definition(s) and every unambiguous reference
/// (calls, type uses, base classes, imports). Ambiguous ones are reported.
pub fn rename(root: &Path, g: &Graph, i: usize, new_name: &str, dry_run: bool) -> Result<RenameReport> {
    let target = &g.syms[i];
    let old = target.sym.name.clone();
    // a JS / TS private member (`#count`) stays private: the `#` is part of the name on both sides
    let private = old.starts_with('#');
    let bare = if private { new_name.strip_prefix('#').unwrap_or(new_name) } else { new_name };
    if bare.is_empty() || !bare.bytes().all(is_ident) || bare.as_bytes()[0].is_ascii_digit() {
        bail!("`{new_name}` is not a valid identifier");
    }
    let new_name = &if private { format!("#{bare}") } else { bare.to_string() };
    // the symbol and its prototype / definition twins (C/C++ header + source); a method's
    // overrides in subclasses / implementations are renamed with it (dispatch must keep working)
    let mut twins: Vec<usize> = (0..g.syms.len()).filter(|&j| g.syms[j].sym.qualified == target.sym.qualified).collect();
    if target.sym.kind == gitnext_parse::Kind::Method {
        for o in g.overrides(i) {
            if !twins.contains(&o) {
                twins.push(o);
            }
        }
    }
    // file → line → set of byte columns to replace
    let mut edits: BTreeMap<String, BTreeMap<usize, Vec<usize>>> = BTreeMap::new();
    let mut report = RenameReport { old: old.clone(), new: new_name.into(), dry_run, ..Default::default() };
    let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut lines_of = |file: &str| -> Vec<String> {
        cache.entry(file.to_string()).or_insert_with(|| read_lines(root, file).map(|x| x.1).unwrap_or_default()).clone()
    };

    // 1. definitions: first whole-word hit in the declaration head
    for &t in &twins {
        let s = &g.syms[t];
        let lines = lines_of(&s.file);
        let head_end = (s.sym.start_line as usize + s.sym.signature.lines().count().max(1) + 2).min(s.sym.end_line as usize);
        let found = (s.sym.start_line as usize..=head_end).find_map(|l| word_hits(lines.get(l - 1)?, &old).first().map(|&c| (l, c)));
        match found {
            Some((l, c)) => edits.entry(s.file.clone()).or_default().entry(l).or_default().push(c),
            None => report.skipped.push(format!("{}:{} definition name not found", s.file, s.sym.start_line)),
        }
    }
    // 2. references
    for (file, e) in &g.edges {
        if last_segment(&e.to) != old {
            continue;
        }
        let res = g.resolve(file, e);
        if !res.iter().any(|x| twins.contains(x)) {
            continue;
        }
        let only_target = res.iter().all(|x| twins.contains(x));
        let loc = format!("{file}:{}", e.line);
        if g.foreign_receiver_for_rename(file, e) {
            // `c.Flags().HasFlags()` with `Flags() *pflag.FlagSet`: the library's method of the same name
            report.skipped.push(format!("{loc} `{}` (the receiver is a library type)", e.to));
            continue;
        }
        if !only_target {
            report.skipped.push(format!("{loc} `{}` (ambiguous: {} candidates)", e.to, res.len()));
            continue;
        }
        let lines = lines_of(file);
        let Some(text) = lines.get(e.line as usize - 1) else { continue };
        let hits = word_hits(text, &old);
        let col = if e.kind == EdgeKind::Imports || !hits.contains(&(e.col as usize)) {
            // imports / imprecise columns: every whole-word hit on that line
            if hits.is_empty() {
                continue;
            }
            hits
        } else {
            vec![e.col as usize]
        };
        edits.entry(file.clone()).or_default().entry(e.line as usize).or_default().extend(col);
    }

    // whatever the graph did not reach: `__all__` entries are renamed, everything
    // else that still spells the old name (strings, comments, dynamic access) is reported
    // an import / export of this name: followed when the name is ours alone (no other
    // exported definition in the same language family spells it)
    let family = gitnext_parse::family_of(&target.file);
    let ours_alone = g.find(&old).iter().all(|j| twins.contains(j) || g.syms[*j].sym.test || gitnext_parse::family_of(&g.syms[*j].file) != family || g.syms[*j].sym.visibility.is_none());
    for f in g.files.iter().filter(|f| f.parsed) {
        let lines = lines_of(&f.path);
        let all_lines: std::collections::HashSet<usize> = if f.path.ends_with(".py") { dunder_all_lines(&lines) } else { Default::default() };
        let import_spans = if ours_alone && gitnext_parse::family_of(&f.path) == family && lines.iter().any(|t| !word_hits(t, &old).is_empty()) {
            let text = lines.join("\n");
            let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
            let mut spans = gitnext_parse::import_name_spans(Path::new(&f.path), &text);
            if f.path.ends_with(".rs") {
                // `use crate::util::{third, slice_eq};` — a name inside a group, from this repository
                spans.extend(
                    gitnext_parse::rust_use_names(&text)
                        .into_iter()
                        .filter(|(_, _, first)| matches!(first.as_str(), "crate" | "self" | "super") || g.is_repo_root(first))
                        .map(|(a, b, _)| (a, b)),
                );
            }
            Some((starts, spans))
        } else {
            None
        };
        for (n, text) in lines.iter().enumerate() {
            let l = n + 1;
            for c in word_hits(text, &old) {
                let done = edits.get(&f.path).and_then(|m| m.get(&l)).is_some_and(|cols| cols.contains(&c));
                if done {
                    continue;
                }
                let quoted = c > 0 && matches!(text.as_bytes()[c - 1], b'"' | b'\'') && text.as_bytes().get(c + old.len()).is_some_and(|b| matches!(b, b'"' | b'\''));
                let in_import = import_spans.as_ref().is_some_and(|(starts, spans)| {
                    let at = starts[n] + c;
                    spans.iter().any(|&(a, b)| a == at && b == at + old.len())
                });
                if in_import || (all_lines.contains(&l) && quoted) {
                    edits.entry(f.path.clone()).or_default().entry(l).or_default().push(c);
                } else if !twins.iter().any(|&t| g.syms[t].file == f.path && l >= g.syms[t].sym.start_line as usize && l <= g.syms[t].sym.start_line as usize + 1) {
                    report.leftover.push(format!("{}:{l}: {}", f.path, text.trim().chars().take(100).collect::<String>()));
                }
            }
        }
    }
    report.leftover.sort();
    report.leftover.dedup();

    // overrides are renamed with the method (see `twins`); say so
    for &o in twins.iter().filter(|&&o| g.syms[o].sym.qualified != target.sym.qualified) {
        let s = &g.syms[o];
        report.overrides.push(format!("{} {}:{}", g.short_name(o), s.file, s.sym.start_line));
    }
    // 3. apply (all files validated before anything is written)
    let mut outputs: Vec<(String, String, String)> = Vec::new();
    for (file, by_line) in &edits {
        let (old_text, mut lines) = read_lines(root, file)?;
        for (&l, cols) in by_line {
            let mut cols = cols.clone();
            cols.sort_unstable();
            cols.dedup();
            let line = &mut lines[l - 1];
            for &c in cols.iter().rev() {
                if line.get(c..c + old.len()) == Some(old.as_str()) {
                    line.replace_range(c..c + old.len(), new_name);
                }
            }
            report.changed.push(format!("{file}:{l}"));
        }
        let new_text = join(&lines, old_text.ends_with('\n'));
        let before = syntax_errors(Path::new(file), &old_text).len();
        if syntax_errors(Path::new(file), &new_text).len() > before {
            bail!("rename would break {file}; nothing written");
        }
        outputs.push((file.clone(), old_text, new_text));
    }
    report.files = outputs.len();
    if !dry_run {
        for (file, _, new_text) in &outputs {
            std::fs::write(root.join(file), new_text)?;
        }
        crate::oplog::record(root, "rename", &format!("{} → {}", report.old, report.new), &outputs);
    }
    Ok(report)
}

pub fn render_edit(r: &EditResult) -> String {
    let mut s = format!("ok {}:{}-{}\n", r.file, r.lines.0, r.lines.1);
    for c in &r.changes {
        s.push_str(c);
        s.push('\n');
    }
    s
}

pub fn render_rename(r: &RenameReport) -> String {
    let mut s = format!(
        "{} {} → {}: {} places in {} files\n",
        if r.dry_run { "would rename" } else { "renamed" },
        r.old,
        r.new,
        r.changed.len(),
        r.files
    );
    if !r.changed.is_empty() {
        s.push_str(&r.changed.join(", "));
        s.push('\n');
    }
    if !r.overrides.is_empty() {
        s.push_str(&format!("overrides renamed too ({}): {}\n", r.overrides.len(), r.overrides.join(", ")));
    }
    if !r.text_changed.is_empty() {
        s.push_str(&format!("also as text ({}): {}\n", r.text_changed.len(), r.text_changed.join(", ")));
    }
    if !r.leftover.is_empty() {
        s.push_str(&format!("the old name still appears as text ({}) — strings, comments or dynamic access; change by hand if they mean this symbol:\n", r.leftover.len()));
        for x in r.leftover.iter().take(30) {
            s.push_str("  ");
            s.push_str(x);
            s.push('\n');
        }
    }
    if !r.skipped.is_empty() {
        s.push_str(&format!("not changed ({}): check manually\n", r.skipped.len()));
        for x in &r.skipped {
            s.push_str("  ");
            s.push_str(x);
            s.push('\n');
        }
    }
    s
}
