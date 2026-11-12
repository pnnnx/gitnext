//! Agent journal: every MCP tool call is appended to `.gitnext/journal.jsonl`
//! (local, git-ignored) so a human can see what the agent looked at and changed,
//! and how many tokens gitnext saved compared to reading whole files.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// unix seconds
    pub ts: i64,
    pub session: String,
    pub tool: String,
    /// main argument (symbol / path / query), shortened
    #[serde(default)]
    pub target: String,
    /// qualified names the call returned code/facts for
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    /// symbols changed by edit tools
    #[serde(default)]
    pub edited: Vec<String>,
    pub out_tokens: usize,
    /// ≈ tokens the agent would have spent reading the involved files whole
    pub naive_tokens: usize,
    #[serde(default)]
    pub error: bool,
    /// operation recorded by this call (`gitnext ops`, `gitnext undo --op N`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<u64>,
}

pub fn path(root: &Path) -> PathBuf {
    root.join(gitnext_index::GITNEXT_DIR).join("journal.jsonl")
}

pub fn append(root: &Path, e: &Entry) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path(root)) {
        let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
    }
}

pub fn load(root: &Path) -> Vec<Entry> {
    std::fs::read_to_string(path(root))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Sessions newest first: session id → entries.
pub fn sessions(entries: &[Entry]) -> Vec<(String, Vec<&Entry>)> {
    let mut by: BTreeMap<(i64, String), Vec<&Entry>> = BTreeMap::new();
    let mut first: HashMap<&str, i64> = HashMap::new();
    for e in entries {
        first.entry(e.session.as_str()).or_insert(e.ts);
    }
    for e in entries {
        by.entry((first[e.session.as_str()], e.session.clone())).or_default().push(e);
    }
    by.into_iter().rev().map(|((_, s), v)| (s, v)).collect()
}

pub(crate) fn hm(ts: i64) -> String {
    let d = crate::history_date(ts);
    let secs = ts.rem_euclid(86_400);
    format!("{d} {:02}:{:02} UTC", secs / 3600, (secs % 3600) / 60)
}

pub fn render(entries: &[Entry], last_sessions: usize) -> String {
    if entries.is_empty() {
        return "no agent activity recorded yet (the journal is written by `gitnext mcp`)\n".into();
    }
    let mut s = String::new();
    for (sid, es) in sessions(entries).into_iter().take(last_sessions) {
        let out: usize = es.iter().map(|e| e.out_tokens).sum();
        let naive: usize = es.iter().map(|e| e.naive_tokens).sum();
        let _ = writeln!(
            s,
            "session {sid} · {} → {} · {} calls · {} tokens served · ≈{} saved vs reading files",
            hm(es[0].ts),
            hm(es.last().unwrap().ts),
            es.len(),
            gitnext_docs::fmt_k(out),
            gitnext_docs::fmt_k(naive.saturating_sub(out))
        );
        let mut tools: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &es {
            *tools.entry(e.tool.as_str()).or_default() += 1;
        }
        let _ = writeln!(s, "tools: {}", tools.iter().map(|(t, n)| format!("{t} ×{n}")).collect::<Vec<_>>().join(", "));
        let mut edited: Vec<&str> = es.iter().flat_map(|e| e.edited.iter().map(String::as_str)).collect();
        edited.sort_unstable();
        edited.dedup();
        if !edited.is_empty() {
            let _ = writeln!(s, "edited: {}", edited.join(", "));
        }
        let mut read: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &es {
            for x in &e.symbols {
                *read.entry(x.as_str()).or_default() += 1;
            }
        }
        if !read.is_empty() {
            let mut v: Vec<(&str, usize)> = read.into_iter().collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.1));
            let _ = writeln!(s, "looked at: {}", v.iter().take(15).map(|(k, n)| if *n > 1 { format!("{k} ×{n}") } else { k.to_string() }).collect::<Vec<_>>().join(", "));
        }
        let errs = es.iter().filter(|e| e.error).count();
        if errs > 0 {
            let _ = writeln!(s, "errors: {errs}");
        }
        let _ = writeln!(s, "timeline:");
        for e in es.iter().rev().take(25).rev() {
            let secs = e.ts.rem_euclid(86_400);
            let mark = if !e.edited.is_empty() { "✎" } else if e.error { "✗" } else { "·" };
            let _ = writeln!(s, "  {:02}:{:02} {mark} {} {}", secs / 3600, (secs % 3600) / 60, e.tool, e.target);
        }
        s.push('\n');
    }
    s
}

/// qualified name → 'e' (edited) / 'r' (read) for the latest session.
pub fn last_session_marks(entries: &[Entry]) -> HashMap<String, char> {
    let mut m = HashMap::new();
    if let Some((_, es)) = sessions(entries).into_iter().next() {
        for e in es {
            for x in &e.symbols {
                m.entry(x.clone()).or_insert('r');
            }
            for x in &e.edited {
                m.insert(x.clone(), 'e');
            }
        }
    }
    m
}
