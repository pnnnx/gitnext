//! Inferred summaries ("notes") for symbols and directories without docs.
//!
//! Stored in `.gitnext/notes.tsv` (meant to be committed, so the whole team and
//! every agent shares them). Each note is pinned to the symbol's body hash: when
//! the code changes the note goes stale and is no longer shown.
//!
//! Format, one per line: `key \t hash \t source \t summary` where key is a
//! qualified symbol name or `dir:<path>`.

use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub hash: String,
    /// `agent`, `llm:<model>`, `user`…
    pub source: String,
    pub summary: String,
}

pub type Notes = BTreeMap<String, Note>;

pub fn path(root: &Path) -> PathBuf {
    root.join(crate::GITNEXT_DIR).join("notes.tsv")
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn load(root: &Path) -> Notes {
    let mut out = Notes::new();
    let Ok(text) = std::fs::read_to_string(path(root)) else { return out };
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(4, '\t');
        if let (Some(k), Some(h), Some(src), Some(sum)) = (it.next(), it.next(), it.next(), it.next()) {
            out.insert(k.to_string(), Note { hash: h.to_string(), source: src.to_string(), summary: sum.to_string() });
        }
    }
    out
}

pub fn save(root: &Path, notes: &Notes) -> Result<()> {
    let mut s = String::from("# gitnext notes: inferred one-line summaries (key, body hash, source, summary). Safe to commit.\n");
    for (k, n) in notes {
        s.push_str(&format!("{}\t{}\t{}\t{}\n", clean(k), n.hash, clean(&n.source), clean(&n.summary)));
    }
    std::fs::create_dir_all(path(root).parent().unwrap())?;
    std::fs::write(path(root), s)?;
    Ok(())
}

/// Insert / replace notes and persist.
pub fn upsert(root: &Path, items: impl IntoIterator<Item = (String, Note)>) -> Result<usize> {
    let mut notes = load(root);
    let mut n = 0;
    for (k, v) in items {
        if v.summary.trim().is_empty() {
            continue;
        }
        notes.insert(k, Note { summary: clean(&v.summary), ..v });
        n += 1;
    }
    save(root, &notes)?;
    Ok(n)
}
