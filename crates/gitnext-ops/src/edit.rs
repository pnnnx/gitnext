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

/// Re-indent `code` to `indent` if it was sent flush-left (common for agents).
fn reindent(code: &str, indent: &str) -> Vec<String> {
    let lines: Vec<&str> = code.trim_end_matches('\n').lines().collect();
    let min = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| if l.trim().is_empty() { String::new() } else { format!("{indent}{}", &l[min.min(l.len() - l.trim_start().len())..]) })
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
    pub files: usize,
    pub dry_run: bool,
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

/// Rename a symbol at its definition(s) and every unambiguous reference
/// (calls, type uses, base classes, imports). Ambiguous ones are reported.
pub fn rename(root: &Path, g: &Graph, i: usize, new_name: &str, dry_run: bool) -> Result<RenameReport> {
    let target = &g.syms[i];
    let old = target.sym.name.clone();
    if new_name.is_empty() || !new_name.bytes().all(is_ident) || new_name.as_bytes()[0].is_ascii_digit() {
        bail!("`{new_name}` is not a valid identifier");
    }
    // the symbol and its prototype / definition twins (C/C++ header + source)
    let twins: Vec<usize> = (0..g.syms.len()).filter(|&j| g.syms[j].sym.qualified == target.sym.qualified).collect();
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

    // overriding methods in subclasses / trait impls keep the old name: warn
    if target.sym.kind == gitnext_parse::Kind::Method {
        for o in g.overrides(i) {
            let s = &g.syms[o];
            report.skipped.push(format!("{}:{} override {} (rename it too to keep dispatch)", s.file, s.sym.start_line, g.short_name(o)));
        }
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
