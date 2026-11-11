//! Persistent, incremental code index stored in `.gitnext/index.db` (SQLite).
//!
//! [`Index::refresh`] re-parses only files whose size/mtime/content changed.
//! [`Index::graph`] loads everything into a [`Graph`] for cross-reference queries.

mod graph;
pub mod libs;
pub mod notes;
pub mod semdiff;

pub use graph::{flatten_use, Callees, DirDeps, Graph, Hierarchy, ImpactNode, SymRef};
pub use libs::{Dep, LibUse};

use anyhow::{Context, Result};
use gitnext_parse::{analyze_in, find_crate, file_type_label, hash_hex, lang_for, Edge, EdgeKind, FileAnalysis, Kind, Param, Symbol};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Bump whenever the parser output or the schema changes (forces a full re-index).
const SCHEMA_VERSION: &str = "7";
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const GITNEXT_DIR: &str = ".gitnext";

/// Build output, caches and virtualenvs: copies of the code, never the code itself.
/// Skipped even without a .gitignore. `build` and `dist` are kept when git tracks
/// files in them; `.gitnext/config` can add (`index_skip = gen, vendor`) or
/// keep (`index_keep = build`) folders.
pub const DEFAULT_SKIP: &[&str] = &[
    "target", "node_modules", "__pycache__", "build", "dist", "*.egg-info", ".eggs", ".venv", "venv", "env",
    ".tox", ".nox", ".mypy_cache", ".pytest_cache", ".ruff_cache", "site-packages", "cmake-build-*", "_build",
];

pub fn dir_matches(pattern: &str, name: &str) -> bool {
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => name.ends_with(suffix),
        (_, Some(prefix)) => name.starts_with(prefix),
        _ => name == pattern,
    }
}

fn config_list(root: &Path, key: &str) -> Vec<String> {
    std::fs::read_to_string(root.join(GITNEXT_DIR).join("config"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .flat_map(|(_, v)| v.split(',').map(|x| x.trim().trim_end_matches('/').to_string()).collect::<Vec<_>>())
        .filter(|x| !x.is_empty())
        .collect()
}

/// Folder names the walker skips for this repository.
pub fn skipped_dirs(root: &Path) -> Vec<String> {
    let keep = config_list(root, "index_keep");
    let mut v: Vec<String> = DEFAULT_SKIP.iter().map(|s| s.to_string()).filter(|s| !keep.contains(s)).collect();
    // `build/` or `dist/` committed to git is real content (e.g. a build-system package)
    if root.join(".git").exists() {
        if let Ok(o) = std::process::Command::new("git").arg("-C").arg(root).args(["ls-files", "--", "build", "dist"]).output() {
            let out = String::from_utf8_lossy(&o.stdout);
            for d in ["build", "dist"] {
                if out.lines().any(|l| l.starts_with(&format!("{d}/"))) {
                    v.retain(|x| x != d);
                }
            }
        }
    }
    v.extend(config_list(root, "index_skip"));
    v
}

/// LSP answer: (file, line, col) → (target file, target line).
pub type Link = ((String, u32, u32), (String, u32));

pub struct Index {
    conn: Connection,
    root: PathBuf,
    /// folder names to skip (computed once per open: asks git which ones are tracked)
    skip: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub path: String,
    pub file_type: String,
    pub lines: u32,
    pub parsed: bool,
    pub module_doc: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RefreshStats {
    pub files_total: usize,
    pub files_reparsed: usize,
    pub files_removed: usize,
    pub symbols_total: usize,
    pub edges_total: usize,
}

/// A symbol together with the file it lives in.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolRow {
    pub file: String,
    #[serde(flatten)]
    pub sym: Symbol,
}

impl Index {
    /// Open (or create) the index for a repository root.
    pub fn open(root: &Path) -> Result<Self> {
        let dir = root.join(GITNEXT_DIR);
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        ensure_gitignore(&dir)?;
        let conn = Connection::open(dir.join("index.db"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let mut idx = Index { conn, root: root.to_path_buf(), skip: skipped_dirs(root) };
        idx.migrate()?;
        Ok(idx)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE IF NOT EXISTS lsp_links (
               file TEXT, line INTEGER, col INTEGER, file_hash TEXT, target_file TEXT, target_line INTEGER,
               PRIMARY KEY (file, line, col));",
        )?;
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0))
            .optional()?;
        let want = format!("{SCHEMA_VERSION}.{}", gitnext_parse::PARSER_VERSION);
        if v.as_deref() == Some(want.as_str()) {
            return Ok(());
        }
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS edges; DROP TABLE IF EXISTS symbols; DROP TABLE IF EXISTS files;
             CREATE TABLE files (
               path TEXT PRIMARY KEY, mtime INTEGER, size INTEGER, hash TEXT,
               file_type TEXT, lines INTEGER, parsed INTEGER, module_doc TEXT);
             CREATE TABLE symbols (
               id INTEGER PRIMARY KEY, file TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
               kind TEXT, name TEXT, qualified TEXT, parent TEXT, trait_impl TEXT,
               start_line INTEGER, end_line INTEGER, signature TEXT, params TEXT, returns TEXT,
               doc TEXT, summary TEXT, visibility TEXT, body_hash TEXT, test INTEGER, decl INTEGER);
             CREATE INDEX symbols_name ON symbols(name);
             CREATE INDEX symbols_file ON symbols(file);
             CREATE TABLE edges (
               file TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
               src TEXT, dst TEXT, kind TEXT, line INTEGER, col INTEGER);
             CREATE INDEX edges_src ON edges(src);",
        )?;
        self.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('schema', ?1)", [want])?;
        Ok(())
    }

    /// Bring the index up to date with the working tree. Cheap when nothing changed.
    pub fn refresh(&mut self) -> Result<RefreshStats> {
        let known: HashMap<String, (i64, i64, String)> = {
            let mut st = self.conn.prepare("SELECT path, mtime, size, hash FROM files")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
            rows.collect::<rusqlite::Result<_>>()?
        };

        // 1. Walk the tree (respects .gitignore), collect candidate files.
        let mut found: Vec<(String, PathBuf, i64, i64)> = Vec::new();
        let skip = self.skip.clone();
        let walker = ignore::WalkBuilder::new(&self.root)
            .hidden(true)
            .require_git(false)
            .filter_entry(move |e| {
                let n = e.file_name().to_string_lossy();
                let is_dir = e.file_type().is_some_and(|t| t.is_dir());
                !(n == GITNEXT_DIR || n == ".git" || n == ".DS_Store" || (is_dir && skip.iter().any(|p| dir_matches(p, &n))))
            })
            .build();
        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let Ok(rel) = entry.path().strip_prefix(&self.root) else { continue };
            let rel_s = to_slash(rel);
            found.push((rel_s, entry.path().to_path_buf(), mtime, meta.len() as i64));
        }

        // 2. Decide what to (re)parse. Content hash guards against touch-only changes.
        struct Parsed {
            path: String,
            mtime: i64,
            size: i64,
            hash: String,
            lines: u32,
            analysis: Option<FileAnalysis>,
        }
        // Owning package of each source file (nearest Cargo.toml), cached per directory.
        let root = self.root.clone();
        let mut manifests: HashMap<String, Option<String>> = HashMap::new();
        let mut read = |p: &str| -> Option<String> {
            manifests.entry(p.to_string()).or_insert_with(|| std::fs::read_to_string(root.join(p)).ok()).clone()
        };
        let hints: HashMap<&str, gitnext_parse::CrateHint> = found
            .iter()
            .filter(|f| lang_for(Path::new(&f.0)).is_some_and(|l| !l.is_c_family()))
            .filter_map(|f| find_crate(&f.0, &mut read).map(|h| (f.0.as_str(), h)))
            .collect();

        let to_parse: Vec<Parsed> = found
            .par_iter()
            .filter_map(|(rel, abs, mtime, size)| {
                if let Some((m, s, _)) = known.get(rel) {
                    if m == mtime && s == size {
                        return None;
                    }
                }
                let bytes = std::fs::read(abs).ok()?;
                let text = String::from_utf8(bytes).ok()?; // skip binaries
                let hash = hash_hex(text.as_bytes());
                if known.get(rel).is_some_and(|(_, _, h)| *h == hash) {
                    // Content identical (only touched): just record the new mtime.
                    return Some(Parsed { path: rel.clone(), mtime: *mtime, size: *size, hash: "=".into(), lines: 0, analysis: None });
                }
                let lines = text.lines().count() as u32;
                let analysis = analyze_in(Path::new(rel), &text, hints.get(rel.as_str())).unwrap_or(None);
                Some(Parsed { path: rel.clone(), mtime: *mtime, size: *size, hash, lines, analysis })
            })
            .collect();

        let found_set: std::collections::HashSet<&str> = found.iter().map(|f| f.0.as_str()).collect();
        let removed: Vec<&String> = known.keys().filter(|k| !found_set.contains(k.as_str())).collect();

        // 3. Write in one transaction.
        let tx = self.conn.transaction()?;
        for r in &removed {
            tx.execute("DELETE FROM files WHERE path=?1", [r])?;
        }
        let mut reparsed = 0;
        for p in &to_parse {
            if p.hash == "=" {
                tx.execute("UPDATE files SET mtime=?2, size=?3 WHERE path=?1", params![p.path, p.mtime, p.size])?;
                continue;
            }
            reparsed += 1;
            tx.execute("DELETE FROM files WHERE path=?1", [&p.path])?;
            let parsed = p.analysis.is_some();
            let module_doc = p.analysis.as_ref().and_then(|a| a.module_doc.clone());
            tx.execute(
                "INSERT INTO files(path, mtime, size, hash, file_type, lines, parsed, module_doc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    p.path, p.mtime, p.size, p.hash,
                    file_type_label(Path::new(&p.path)), p.lines, parsed, module_doc
                ],
            )?;
            if let Some(a) = &p.analysis {
                insert_analysis(&tx, &p.path, a)?;
            }
        }
        tx.commit()?;

        let count = |t: &str| -> Result<usize> {
            Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get::<_, i64>(0))? as usize)
        };
        Ok(RefreshStats {
            files_total: found.len(),
            files_reparsed: reparsed,
            files_removed: removed.len(),
            symbols_total: count("symbols")?,
            edges_total: count("edges")?,
        })
    }

    pub fn files(&self) -> Result<Vec<FileRow>> {
        let mut st = self
            .conn
            .prepare("SELECT path, file_type, lines, parsed, module_doc FROM files ORDER BY path")?;
        let rows = st.query_map([], |r| {
            Ok(FileRow {
                path: r.get(0)?,
                file_type: r.get(1)?,
                lines: r.get(2)?,
                parsed: r.get(3)?,
                module_doc: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn symbols(&self) -> Result<Vec<SymbolRow>> {
        let mut st = self.conn.prepare(
            "SELECT file, kind, name, qualified, parent, trait_impl, start_line, end_line, signature,
                    params, returns, doc, summary, visibility, body_hash, test, decl
             FROM symbols ORDER BY file, start_line",
        )?;
        let rows = st.query_map([], |r| {
            let params_json: String = r.get(9)?;
            Ok(SymbolRow {
                file: r.get(0)?,
                sym: Symbol {
                    kind: Kind::parse(&r.get::<_, String>(1)?).unwrap_or(Kind::Function),
                    name: r.get(2)?,
                    qualified: r.get(3)?,
                    parent: r.get(4)?,
                    trait_impl: r.get(5)?,
                    start_line: r.get(6)?,
                    end_line: r.get(7)?,
                    signature: r.get(8)?,
                    params: serde_json::from_str::<Vec<Param>>(&params_json).unwrap_or_default(),
                    returns: r.get(10)?,
                    doc: r.get(11)?,
                    summary: r.get(12)?,
                    visibility: r.get(13)?,
                    body_hash: r.get(14)?,
                    test: r.get(15)?,
                    decl: r.get(16)?,
                    inferred: false,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn edges(&self) -> Result<Vec<(String, Edge)>> {
        let mut st = self.conn.prepare("SELECT file, src, dst, kind, line, col FROM edges")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Edge {
                    from: r.get(1)?,
                    to: r.get(2)?,
                    kind: EdgeKind::parse(&r.get::<_, String>(3)?).unwrap_or(EdgeKind::Uses),
                    line: r.get(4)?,
                    col: r.get(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Cache language-server answers: call site → definition (`""` target = outside the repo).
    pub fn store_links(&mut self, links: &[(String, u32, u32, String, u32)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare(
                "INSERT OR REPLACE INTO lsp_links(file, line, col, file_hash, target_file, target_line)
                 VALUES (?1, ?2, ?3, (SELECT hash FROM files WHERE path = ?1), ?4, ?5)",
            )?;
            for (f, l, c, t, tl) in links {
                st.execute(params![f, l, c, t, tl])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Links still valid for the current file contents.
    pub fn links(&self) -> Result<Vec<Link>> {
        let mut st = self.conn.prepare(
            "SELECT l.file, l.line, l.col, l.target_file, l.target_line FROM lsp_links l
             JOIN files f ON f.path = l.file AND f.hash = l.file_hash",
        )?;
        let rows = st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?), (r.get(3)?, r.get(4)?))))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Load the whole index into memory for cross-reference queries.
    pub fn graph(&self) -> Result<Graph> {
        let mut g = Graph::new(self.files()?, self.symbols()?, self.edges()?);
        g.apply_notes(&notes::load(&self.root));
        g.links = self.links()?.into_iter().collect();
        Ok(g)
    }
}

fn insert_analysis(tx: &rusqlite::Transaction, path: &str, a: &FileAnalysis) -> Result<()> {
    let mut ins = tx.prepare_cached(
        "INSERT INTO symbols(file, kind, name, qualified, parent, trait_impl, start_line, end_line,
                             signature, params, returns, doc, summary, visibility, body_hash, test, decl)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
    for s in &a.symbols {
        ins.execute(params![
            path, s.kind.as_str(), s.name, s.qualified, s.parent, s.trait_impl, s.start_line,
            s.end_line, s.signature, serde_json::to_string(&s.params)?, s.returns, s.doc,
            s.summary, s.visibility, s.body_hash, s.test, s.decl
        ])?;
    }
    let mut ins = tx.prepare_cached("INSERT INTO edges(file, src, dst, kind, line, col) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
    for e in &a.edges {
        ins.execute(params![path, e.from, e.to, e.kind.as_str(), e.line, e.col])?;
    }
    Ok(())
}

fn ensure_gitignore(dir: &Path) -> Result<()> {
    let gi = dir.join(".gitignore");
    let mut text = std::fs::read_to_string(&gi).unwrap_or_else(|_| "# local state — map/ and notes.tsv are meant to be committed\n".into());
    let mut changed = !gi.exists();
    for line in ["index.db", "index.db-*", "journal.jsonl", "hook-seen.tsv", "tui-welcomed", ".fuse_hidden*"] {
        if !text.lines().any(|l| l.trim() == line) {
            text.push_str(line);
            text.push('\n');
            changed = true;
        }
    }
    if changed {
        std::fs::write(&gi, text)?;
    }
    Ok(())
}

pub fn to_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Directory part of a slash path ("" for the root).
pub fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Whether the path has a semantic parser (re-exported for callers).
pub fn is_parsed_lang(path: &str) -> bool {
    lang_for(Path::new(path)).is_some()
}
