//! Semantic diff: compares two versions of a file at the symbol level
//! ("signature of `parse` changed", "method `Lexer::peek` added").

use gitnext_parse::{analyze_in, CrateHint, Kind, Symbol};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Removed,
    Signature,
    Body,
    Doc,
    Renamed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymChange {
    pub change: Change,
    pub kind: Kind,
    pub qualified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trait_impl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_signature: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub test: bool,
    /// Line range (1-based, inclusive) in the old / new version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_lines: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_lines: Option<(u32, u32)>,
}

impl SymChange {
    /// Compact one-line form used in commit trailers and terminal output.
    pub fn line(&self) -> String {
        let name = match &self.trait_impl {
            Some(t) => format!("{} (impl {t})", self.qualified),
            None => self.qualified.clone(),
        };
        let k = self.kind.as_str();
        match self.change {
            Change::Added => format!("+ {k} {name}"),
            Change::Removed => format!("- {k} {name}"),
            Change::Signature => format!(
                "~ {k} {name}: `{}` → `{}`",
                self.old_signature.as_deref().unwrap_or(""),
                self.new_signature.as_deref().unwrap_or("")
            ),
            Change::Body => format!("* {k} {name} (body)"),
            Change::Renamed => format!(
                "→ {k} {} renamed to {}",
                self.old_signature.as_deref().unwrap_or("?"),
                self.qualified
            ),
            Change::Doc => format!("* {k} {name} (docs)"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    /// added | modified | deleted
    pub status: &'static str,
    /// Whether a semantic parser exists for this file type.
    pub semantic: bool,
    pub changes: Vec<SymChange>,
}

pub fn diff_sources(path: &str, old: Option<&str>, new: Option<&str>) -> FileChange {
    diff_sources_in(path, old, new, None)
}

/// [`diff_sources`] with the owning package known, so names match the index.
pub fn diff_sources_in(path: &str, old: Option<&str>, new: Option<&str>, krate: Option<&CrateHint>) -> FileChange {
    let status = match (old, new) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        _ => "modified",
    };
    let parse = |s: Option<&str>| -> Option<Vec<Symbol>> {
        let a = analyze_in(Path::new(path), s.unwrap_or(""), krate).ok()??;
        Some(a.symbols.into_iter().filter(|s| !(s.kind == Kind::Module && s.start_line == 1)).collect())
    };
    let (Some(old_syms), Some(new_syms)) = (parse(old), parse(new)) else {
        return FileChange { path: path.into(), status, semantic: false, changes: vec![] };
    };
    let key = |s: &Symbol| (s.qualified.clone(), s.trait_impl.clone());
    let old_map: BTreeMap<_, _> = old_syms.iter().map(|s| (key(s), s)).collect();
    let new_map: BTreeMap<_, _> = new_syms.iter().map(|s| (key(s), s)).collect();

    let mut changes = Vec::new();
    let mk = |change, s: &Symbol, o: Option<&Symbol>, n: Option<&Symbol>| SymChange {
        change,
        kind: s.kind,
        qualified: s.qualified.clone(),
        trait_impl: s.trait_impl.clone(),
        old_signature: o.map(|x| x.signature.clone()),
        new_signature: n.map(|x| x.signature.clone()),
        test: s.test,
        old_lines: None,
        new_lines: None,
    };
    for (k, n) in &new_map {
        match old_map.get(k) {
            None => changes.push(mk(Change::Added, n, None, Some(n))),
            Some(o) if o.signature != n.signature => changes.push(mk(Change::Signature, n, Some(o), Some(n))),
            Some(o) if o.body_hash != n.body_hash && n.kind == Kind::Module => {}
            Some(o) if o.body_hash != n.body_hash && !n.kind.is_type() => {
                changes.push(mk(Change::Body, n, None, None))
            }
            // types: only a change of fields/variants is interesting; methods report themselves
            Some(o) if o.body_hash != n.body_hash && o.params != n.params => changes.push(mk(Change::Signature, n, Some(o), Some(n))),
            Some(o) if o.doc != n.doc => changes.push(mk(Change::Doc, n, None, None)),
            _ => {}
        }
    }
    for (k, o) in &old_map {
        if !new_map.contains_key(k) {
            changes.push(mk(Change::Removed, o, Some(o), None));
        }
    }
    // removed + added with the same shape = rename
    let removed: Vec<usize> = (0..changes.len()).filter(|&i| changes[i].change == Change::Removed).collect();
    let mut drop = std::collections::HashSet::new();
    for ri in removed {
        let r = &changes[ri];
        let Some(o) = old_map.get(&(r.qualified.clone(), r.trait_impl.clone())) else { continue };
        let hit = changes.iter().position(|c| {
            c.change == Change::Added && c.kind == o.kind && !drop.contains(&c.qualified) && {
                let n = new_map.get(&(c.qualified.clone(), c.trait_impl.clone())).unwrap();
                n.parent == o.parent && n.params == o.params && n.signature.replacen(&n.name, &o.name, 1) == o.signature
            }
        });
        if let Some(ai) = hit {
            let q = changes[ai].qualified.clone();
            drop.insert(q.clone());
            changes[ri].change = Change::Renamed;
            changes[ri].old_signature = Some(o.qualified.clone());
            changes[ri].qualified = q;
        }
    }
    changes.retain(|c| !(c.change == Change::Added && drop.contains(&c.qualified)));
    for c in &mut changes {
        let old_q = if c.change == Change::Renamed { c.old_signature.clone().unwrap_or_default() } else { c.qualified.clone() };
        c.old_lines = old_map.get(&(old_q, c.trait_impl.clone())).map(|s| (s.start_line, s.end_line));
        c.new_lines = new_map.get(&(c.qualified.clone(), c.trait_impl.clone())).map(|s| (s.start_line, s.end_line));
    }
    changes.sort_by(|a, b| a.change.cmp(&b.change).then(a.qualified.cmp(&b.qualified)));
    FileChange { path: path.into(), status, semantic: true, changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_changes() {
        let old = "pub fn a(x: u32) -> u32 { x }\nfn b() {}\n/// doc\nfn c() {}";
        let new = "pub fn a(x: u64) -> u64 { x }\nfn b() { println!(); }\n/// new doc\nfn c() {}\nfn d() {}";
        let fc = diff_sources("src/lib.rs", Some(old), Some(new));
        let lines: Vec<String> = fc.changes.iter().map(|c| c.line()).collect();
        assert!(lines.contains(&"+ function crate::d".to_string()), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("~ function crate::a")));
        assert!(lines.contains(&"* function crate::b (body)".to_string()));
        assert!(lines.contains(&"* function crate::c (docs)".to_string()));
    }
}
