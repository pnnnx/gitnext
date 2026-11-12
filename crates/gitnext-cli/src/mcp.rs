//! MCP server over stdio (JSON-RPC 2.0, one message per line).
//!
//! Exposes the same compact queries as the CLI as tools, so an agent asks
//! "outline this file" / "read this symbol" instead of reading whole files.
//! The index stays open; each call does a cheap incremental refresh and
//! reloads the graph only when files changed.

use anyhow::Result;
use gitnext_ops::gitnext_index::{Graph, Index};
use gitnext_ops::edit::{self, EditKind};
use gitnext_ops::render::{self, Opts};
use gitnext_ops::agents::{self, Profile};
use gitnext_ops::{DiffMode, Snapshot, Workspace};
use std::collections::HashMap;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const DEFAULT_PROTOCOL: &str = "2025-06-18";

struct Server {
    ws: Workspace,
    index: Index,
    graph: Graph,
    /// (file, qualified, start line) → body hash of code already sent by `read`.
    sent: HashMap<(String, String, u32), String>,
    /// Code state at session start / last `changes` call.
    snapshot: Snapshot,
    session: String,
    profile: Profile,
}

impl Server {
    fn new(ws: Workspace, profile: Profile) -> Result<Self> {
        let mut index = Index::open(&ws.root)?;
        index.refresh()?;
        let graph = index.graph()?;
        let snapshot = Snapshot::of(&graph);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let session = format!("{:x}", t ^ (std::process::id() as u64) << 32).chars().rev().take(6).collect();
        gitnext_ops::oplog::set_actor(format!("mcp:{session}"));
        Ok(Server { ws, index, graph, sent: HashMap::new(), snapshot, session, profile })
    }

    fn refresh(&mut self) -> Result<()> {
        let st = self.index.refresh()?;
        if st.files_reparsed > 0 || st.files_removed > 0 {
            self.graph = self.index.graph()?;
        }
        Ok(())
    }
}

fn tools() -> Value {
    let sym = json!({ "type": "string", "description": "Symbol: Name, Type::method, Type.method or fully qualified" });
    let tool = |name: &str, desc: &str, props: Value, req: &[&str]| {
        json!({ "name": name, "description": desc,
                "inputSchema": { "type": "object", "properties": props, "required": req } })
    };
    json!([
        tool("look", "Use this first. One call answers whatever you need next. what = a function/class/method name → its code, who calls it, what it calls, the tests that reach it with the command to run them, and what a change would affect; a file or folder → every signature with line numbers; an error/traceback → the code where it happened; words → where they appear. Pass several targets at once to save steps.",
             json!({ "what": { "type": "string" }, "targets": { "type": "array", "items": { "type": "string" }, "description": "several targets in one call" } }), &[]),
        tool("overview", "Start here in an unfamiliar repo: languages, areas with purposes, entry points (main, routes, CLI commands), area dependencies, recent activity, hotspots, health — one screen.",
             json!({}), &[]),
        tool("brief", "Resuming work? Branch, uncommitted symbol changes, what the last agent session edited and looked at, recent commits — a few hundred tokens.",
             json!({}), &[]),
        tool("entrypoints", "Where execution starts: main functions, HTTP routes, CLI commands, __main__ scripts.",
             json!({}), &[]),
        tool("map", "Directory map: purpose, files, public API with signatures and one-line docs, dependencies. Start here to orient. Empty dir = repository root.",
             json!({ "dir": { "type": "string" } }), &[]),
        tool("outline", "Skeleton of a file or directory: every signature with line number and one-line doc, no bodies (~10-20% of the file's tokens). Use before reading code.",
             json!({ "path": { "type": "string", "description": "file or directory, repo-relative" }, "all": { "type": "boolean", "description": "directories: include private items" } }), &["path"]),
        tool("read", "Exact source of the given symbols only (with doc comments and file:line range for editing). Much cheaper than reading the whole file. Symbols you already read in this session and that did not change are answered with one line; pass force=true to get the code again.",
             json!({ "symbols": { "type": "array", "items": { "type": "string" } }, "force": { "type": "boolean" } }), &["symbols"]),
        tool("changes", "What changed in the code since your last `changes` call (or since the session started): edits by you, the user or other tools. Symbol-level, no file reads needed.",
             json!({}), &[]),
        tool("since", "Symbol-level diff between a git revision (HEAD~3, a branch, a commit) and the working tree.",
             json!({ "rev": { "type": "string" }, "full": { "type": "boolean" } }), &["rev"]),
        tool("show", "Facts about one symbol: signature, docs, callers, callees, used types, base classes / implementors / subclasses.",
             json!({ "symbol": sym, "full_doc": { "type": "boolean" } }), &["symbol"]),
        tool("search", "Find symbols by name (exact, prefix, substring) or doc text.",
             json!({ "query": { "type": "string" }, "limit": { "type": "integer" } }), &["query"]),
        tool("callers", "Who calls this function/method or instantiates this class (approximate, name-based). Tests are counted, not listed, unless tests=true.",
             json!({ "symbol": sym, "tests": { "type": "boolean" } }), &["symbol"]),
        tool("callees", "What this function/method calls: resolved repo symbols, ambiguous method calls, external calls.",
             json!({ "symbol": sym }), &["symbol"]),
        tool("impact", "What may break if this symbol changes: reverse-dependency tree (calls, type uses, subclassing), files to re-check, tests to run.",
             json!({ "symbol": sym, "depth": { "type": "integer", "description": "default 3" } }), &["symbol"]),
        tool("hierarchy", "Type relations: base classes / supertraits, implemented traits, implementors, subclasses, methods.",
             json!({ "symbol": sym }), &["symbol"]),
        tool("deps", "Directory-level dependencies: what this directory uses and what uses it.",
             json!({ "dir": { "type": "string" } }), &[]),
        tool("context", "The code a task is about, in one block: definitions of the names and files the task mentions, where they are used, the tests that reach them.",
             json!({ "task": { "type": "string" }, "tokens": { "type": "integer", "description": "default 2000" } }), &["task"]),
        tool("replace_symbol", "Replace one symbol's source with new code. Send only the new code of that symbol (start with its doc comment to replace the doc too); indentation is fixed automatically; the edit is rejected if the file would not parse. Use Name@LINE to pick an overload.",
             json!({ "symbol": sym, "code": { "type": "string" } }), &["symbol", "code"]),
        tool("insert_after", "Insert new code (e.g. a new function or method) right after an existing symbol.",
             json!({ "symbol": sym, "code": { "type": "string" } }), &["symbol", "code"]),
        tool("delete_symbol", "Delete a symbol together with its doc comment.",
             json!({ "symbol": sym }), &["symbol"]),
        tool("rename_symbol", "Rename a symbol at its definition and every unambiguous reference (calls, type uses, imports). Ambiguous references are listed, not changed. dry_run to preview.",
             json!({ "symbol": sym, "new_name": { "type": "string" }, "dry_run": { "type": "boolean" }, "text": { "type": "boolean", "description": "also rename it in comments and strings" } }), &["symbol", "new_name"]),
        tool("annotate", "Store one-line summaries for symbols (or `dir:<path>`) you understood that have no docs. They are cached against the code hash and shown in map/outline/show for every future session, so you never pay to re-read that code to learn what it does.",
             json!({ "items": { "type": "array", "items": { "type": "object", "properties": { "symbol": { "type": "string" }, "summary": { "type": "string" } }, "required": ["symbol", "summary"] } } }), &["items"]),
        tool("locate", "Paste a compiler error, panic, test failure or traceback: returns the repo symbols at those file:line locations (innermost function/method) and the code of the top one.",
             json!({ "text": { "type": "string" } }), &["text"]),
        tool("tests_for", "Tests that exercise a symbol: the 10 most likely to fail if it breaks (direct calls and tests named after it first) with the exact command to run them, plus one command for the whole test files.",
             json!({ "symbol": sym, "depth": { "type": "integer" }, "all": { "type": "boolean", "description": "name every test, not only the top 10" } }), &["symbol"]),
        tool("history", "Commits that changed one symbol (added / signature / body / docs), newest first — the 'why is it like this' view without reading git logs.",
             json!({ "symbol": sym, "limit": { "type": "integer" } }), &["symbol"]),
        tool("changelog", "Semantic changelog of a git range (e.g. main..HEAD): breaking API changes with external refs, added symbols by area, changed bodies, commits.",
             json!({ "range": { "type": "string" } }), &[]),
        tool("diff", "Symbol-level diff of uncommitted changes: added/removed symbols, changed signatures, changed bodies/docs.",
             json!({ "staged": { "type": "boolean" }, "full": { "type": "boolean", "description": "list every symbol of new/deleted files" } }), &[]),
    ])
}

/// Lite profile: one universal `look` and two edit tools, each with plain
/// string arguments — small models get arrays and optional knobs wrong.
fn lite_tools() -> Value {
    let str_ = |d: &str| json!({ "type": "string", "description": d });
    json!([
        { "name": "look",
          "description": "Look at code without reading whole files. what = a file or folder (lists functions/classes with lines), a function or class name (shows its code), an error message (shows where it happened), or words (finds them).",
          "inputSchema": { "type": "object", "properties": { "what": str_("file, folder, name, error text or words") }, "required": ["what"] } },
        { "name": "replace",
          "description": "Replace one whole function or class with new code. No need to read the file first; indentation is fixed for you.",
          "inputSchema": { "type": "object", "properties": { "symbol": str_("function or class name, e.g. parse_date or Task.reopen"), "code": str_("the complete new definition") }, "required": ["symbol", "code"] } },
        { "name": "add_after",
          "description": "Add new code (e.g. a new function) right after an existing function or class.",
          "inputSchema": { "type": "object", "properties": { "symbol": str_("existing function or class"), "code": str_("the new code") }, "required": ["symbol", "code"] } },
    ])
}

/// First string argument, whatever the model called it.
fn any_str<'a>(a: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| a.get(*k).and_then(Value::as_str)).or_else(|| a.as_object()?.values().find_map(Value::as_str))
}

fn s_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

impl Server {
    fn call(&mut self, name: &str, a: &Value) -> Result<String, String> {
        self.refresh().map_err(|e| format!("index: {e:#}"))?;
        let g = &self.graph;
        let root = self.ws.root.clone();
        let opts = Opts {
            verbose: false,
            tests: a.get("tests").and_then(Value::as_bool).unwrap_or(false),
            all: a.get("all").and_then(Value::as_bool).unwrap_or(false),
            full_doc: a.get("full_doc").and_then(Value::as_bool).unwrap_or(false),
            lite: matches!(self.profile, Profile::Lite),
        };
        let sym = |k: &str| -> Result<Vec<usize>, String> {
            let q = s_arg(a, k).ok_or_else(|| format!("missing `{k}`"))?;
            find(g, q)
        };
        let path = |k: &str| s_arg(a, k).unwrap_or("").trim_start_matches("./").trim_end_matches('/').to_string();
        Ok(match name {
            "map" => {
                let d = render::dir_for(g, &path("dir"));
                let ctx = gitnext_ops::gitnext_docs::Ctx::new(&root, g);
                gitnext_ops::gitnext_docs::render_dir(&ctx, &d)
            }
            "outline" => render::outline(g, &root, &path("path"), opts),
            "read" => {
                let list: Vec<String> = match a.get("symbols") {
                    Some(Value::Array(v)) => v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                    Some(Value::String(s)) => vec![s.clone()],
                    _ => s_arg(a, "symbol").map(|s| vec![s.to_string()]).unwrap_or_default(),
                };
                if list.is_empty() {
                    return Err("give `symbols`".into());
                }
                let mut ids = Vec::new();
                for q in &list {
                    let found = find_many(g, q)?;
                    let first = &g.syms[found[0]].sym.qualified;
                    if found.iter().any(|&i| &g.syms[i].sym.qualified != first) {
                        return Err(format!("{}name one of them, e.g. {}", render::candidates(g, q, &found), g.short_name(found[0])));
                    }
                    ids.extend(found);
                }
                let force = a.get("force").and_then(Value::as_bool).unwrap_or(false);
                let mut out = String::new();
                let mut fresh = Vec::new();
                for i in ids {
                    let r = &g.syms[i];
                    let key = (r.file.clone(), r.sym.qualified.clone(), r.sym.start_line);
                    if !force && self.sent.get(&key) == Some(&r.sym.body_hash) {
                        out.push_str(&format!("{}:{}-{} {} unchanged since you read it\n", r.file, r.sym.start_line, r.sym.end_line, g.short_name(i)));
                    } else {
                        self.sent.insert(key, r.sym.body_hash.clone());
                        fresh.push(i);
                    }
                }
                out.push_str(&render::read(g, &root, &fresh));
                out
            }
            "changes" => {
                let lines = self.snapshot.diff(g);
                self.snapshot = Snapshot::of(g);
                if lines.is_empty() {
                    "no changes since last check\n".to_string()
                } else {
                    format!("{} changes:\n{}\n", lines.len(), lines.join("\n"))
                }
            }
            "since" => {
                let repo = self.ws.repo().map_err(|e| e.to_string())?;
                let rev = s_arg(a, "rev").ok_or("missing `rev`")?;
                let full = a.get("full").and_then(Value::as_bool).unwrap_or(false);
                gitnext_ops::render_diff(&gitnext_ops::since(repo, rev).map_err(|e| format!("{e:#}"))?, full)
            }
            "show" => sym("symbol")?.iter().map(|&i| render::show(g, i, opts)).collect::<Vec<_>>().join("\n"),
            "search" => {
                let q = s_arg(a, "query").ok_or("missing `query`")?;
                let n = a.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
                render::search(g, q, n, opts)
            }
            "callers" => sym("symbol")?.iter().map(|&i| render::callers(g, i, opts)).collect(),
            "callees" => sym("symbol")?.iter().map(|&i| render::callees(g, i, opts)).collect(),
            "impact" => {
                let d = a.get("depth").and_then(Value::as_u64).unwrap_or(3) as usize;
                sym("symbol")?.iter().map(|&i| render::impact(g, i, d, opts)).collect()
            }
            "hierarchy" => {
                let ids: Vec<usize> = sym("symbol")?.into_iter().filter(|&i| g.syms[i].sym.kind.is_type()).collect();
                if ids.is_empty() {
                    return Err("not a type, class or trait".into());
                }
                ids.iter().map(|&i| render::hierarchy(g, i, opts)).collect()
            }
            "deps" => render::deps(g, &render::dir_for(g, &path("dir"))),
            "context" => {
                let t = s_arg(a, "task").ok_or("missing `task`")?;
                let n = a.get("tokens").and_then(Value::as_u64).map_or(gitnext_ops::context::DEFAULT_BUDGET, |n| n as usize);
                let s = gitnext_ops::context::context(&self.ws, g, t, n);
                if s.is_empty() { "nothing in the index matches this task; try `look` with a name or a file".into() } else { s }
            }
            "diff" => {
                let repo = self.ws.repo().map_err(|e| e.to_string())?;
                let mode = if a.get("staged").and_then(Value::as_bool).unwrap_or(false) { DiffMode::Staged } else { DiffMode::Worktree };
                gitnext_ops::render_diff(&gitnext_ops::semantic_diff(repo, mode).map_err(|e| e.to_string())?, a.get("full").and_then(Value::as_bool).unwrap_or(false))
            }
            "replace_symbol" | "insert_after" | "delete_symbol" => {
                let i = find_one(g, s_arg(a, "symbol").ok_or("missing `symbol`")?)?;
                let code = s_arg(a, "code").unwrap_or("");
                let kind = match name {
                    "replace_symbol" => EditKind::Replace,
                    "insert_after" => EditKind::InsertAfter,
                    _ => EditKind::Delete,
                };
                let r = edit::edit_symbol(&root, g, i, kind, code, false).map_err(|e| format!("{e:#}"))?;
                edit::render_edit(&r)
            }
            "annotate" => {
                let items = a.get("items").and_then(Value::as_array).ok_or("give `items`")?;
                let mut n = 0;
                let mut errs = Vec::new();
                for it in items {
                    let (Some(sym), Some(sum)) = (it.get("symbol").and_then(Value::as_str), it.get("summary").and_then(Value::as_str)) else { continue };
                    let r = if let Some(d) = sym.strip_prefix("dir:") {
                        gitnext_ops::explain::annotate_dir(&root, g, d.trim_end_matches('/'), sum, "agent").map_err(|e| e.to_string())
                    } else {
                        find_one(g, sym).and_then(|i| gitnext_ops::explain::annotate(&root, g, i, sum, "agent").map_err(|e| e.to_string()))
                    };
                    match r {
                        Ok(()) => n += 1,
                        Err(e) => errs.push(format!("{sym}: {}", e.lines().next().unwrap_or(""))),
                    }
                }
                // notes are applied at graph load
                self.graph = self.index.graph().map_err(|e| e.to_string())?;
                let mut s = format!("stored {n} notes\n");
                for e in errs {
                    s.push_str(&e);
                    s.push('\n');
                }
                s
            }
            "locate" => gitnext_ops::locate::render_locate(g, &root, s_arg(a, "text").ok_or("missing `text`")?, true),
            "tests_for" => {
                let d = a.get("depth").and_then(Value::as_u64).unwrap_or(4) as usize;
                let all = a.get("all").and_then(Value::as_bool).unwrap_or(false);
                sym("symbol")?.iter().map(|&i| gitnext_ops::locate::render_tests_for(g, &root, i, d, all)).collect()
            }
            "history" => {
                let repo = self.ws.repo().map_err(|e| e.to_string())?;
                let i = find_one(g, s_arg(a, "symbol").ok_or("missing `symbol`")?)?;
                let n = a.get("limit").and_then(Value::as_u64).unwrap_or(300) as usize;
                let ev = gitnext_ops::history::history(repo, g, i, n).map_err(|e| format!("{e:#}"))?;
                gitnext_ops::history::render_history(g, i, &ev)
            }
            "changelog" => {
                let repo = self.ws.repo().map_err(|e| e.to_string())?;
                let r = s_arg(a, "range").unwrap_or("HEAD~10..HEAD");
                gitnext_ops::history::render_changelog(&gitnext_ops::history::changelog(repo, g, r).map_err(|e| format!("{e:#}"))?)
            }
            "brief" => agents::brief(&self.ws, g),
            "look" => {
                let targets: Vec<String> = match a.get("targets").or_else(|| a.get("what")) {
                    Some(Value::Array(v)) => v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                    _ => vec![any_str(a, &["what", "path", "symbol", "name", "query", "text"]).unwrap_or("").trim().to_string()],
                };
                if targets.len() > 1 {
                    gitnext_ops::look::look_many(&self.ws, &self.graph, &targets, self.profile == Profile::Lite)
                } else {
                    self.look(&targets[0])?
                }
            }
            "replace" | "add_after" => {
                let q = any_str(a, &["symbol", "name", "function"]).ok_or("give `symbol`")?;
                let code = a.get("code").and_then(Value::as_str).unwrap_or("");
                let i = find_one(&self.graph, q)?;
                let kind = if name == "replace" { EditKind::Replace } else { EditKind::InsertAfter };
                let r = edit::edit_symbol(&root, &self.graph, i, kind, code, false).map_err(|e| format!("{e:#}"))?;
                edit::render_edit(&r)
            }
            "overview" => gitnext_ops::insight::overview(self.ws.repo.as_ref(), g, &root),
            "entrypoints" => gitnext_ops::insight::render_entrypoints(&gitnext_ops::insight::entrypoints(g, &root)),
            "rename_symbol" => {
                let i = find_one(g, s_arg(a, "symbol").ok_or("missing `symbol`")?)?;
                let new = s_arg(a, "new_name").ok_or("missing `new_name`")?;
                let dry = a.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
                let _op = gitnext_ops::oplog::Group::start(&root, "rename", &format!("{} → {new}", g.syms[i].sym.qualified));
                let mut r = edit::rename(&root, g, i, new, dry).map_err(|e| format!("{e:#}"))?;
                if a.get("text").and_then(Value::as_bool).unwrap_or(false) {
                    edit::rename_leftover_text(&root, &mut r).map_err(|e| format!("{e:#}"))?;
                }
                edit::render_rename(&r)
            }
            other => return Err(format!("unknown tool `{other}`")),
        })
    }
}

impl Server {
    /// `look`: one call for any target; shared with `gitnext look`.
    fn look(&mut self, what: &str) -> Result<String, String> {
        Ok(gitnext_ops::look::look(&self.ws, &self.graph, what, self.profile == Profile::Lite))
    }

    /// Record the call in .gitnext/journal.jsonl with a token-savings estimate.
    fn journal(&self, tool: &str, a: &Value, res: &Result<String, String>) -> (usize, usize) {
        let g = &self.graph;
        let text = match res {
            Ok(t) | Err(t) => t,
        };
        let mut symbols: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        let mut targets: Vec<String> = Vec::new();
        for k in ["symbol", "path", "dir", "query", "task", "rev", "range", "what"] {
            if let Some(v) = s_arg(a, k) {
                targets.push(v.chars().take(60).collect());
            }
        }
        if let Some(Value::Array(v)) = a.get("symbols") {
            targets.extend(v.iter().filter_map(|x| x.as_str().map(str::to_string)));
        }
        for t in targets.iter().filter(|_| !matches!(tool, "search" | "context" | "since" | "changelog" | "locate")) {
            let ids = g.find(t);
            if ids.is_empty() {
                // a path: files under it
                let p = t.trim_end_matches('/');
                files.extend(g.files.iter().filter(|f| f.parsed && (f.path == p || p.is_empty() || f.path.starts_with(&format!("{p}/")))).map(|f| f.path.clone()));
            }
            for i in ids.into_iter().take(5) {
                symbols.push(g.syms[i].sym.qualified.clone());
                files.push(g.syms[i].file.clone());
            }
        }
        files.sort();
        files.dedup();
        let file_tok: usize = files.iter().filter_map(|f| std::fs::metadata(self.ws.root.join(f)).ok()).map(|m| (m.len() as usize).div_ceil(4)).sum();
        let out_tokens = text.len().div_ceil(4);
        let edit = matches!(tool, "replace_symbol" | "insert_after" | "delete_symbol" | "rename_symbol" | "replace" | "add_after");
        // naive agent: read the involved files (and rewrite them for edits)
        let naive = match tool {
            "read" | "outline" | "show" | "map" | "callers" | "callees" | "impact" | "hierarchy" | "tests_for" | "history" | "look" => file_tok,
            _ if edit => file_tok * 2,
            _ => 0,
        };
        let e = gitnext_ops::journal::Entry {
            ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
            session: self.session.clone(),
            tool: tool.to_string(),
            target: targets.join(", ").chars().take(80).collect(),
            symbols: if edit { vec![] } else { symbols.clone() },
            files: files.clone(),
            edited: if edit && res.is_ok() { symbols } else { vec![] },
            out_tokens,
            naive_tokens: naive,
            error: res.is_err(),
            op: if edit && res.is_ok() { gitnext_ops::oplog::take_last() } else { None },
        };
        gitnext_ops::journal::append(&self.ws.root, &e);
        (out_tokens, naive)
    }
}

fn find_many(g: &Graph, q: &str) -> Result<Vec<usize>, String> {
    let ids = g.find(q);
    if ids.is_empty() {
        let hints: Vec<String> = g.search(q, 5).into_iter().map(|i| g.syms[i].sym.qualified.clone()).collect();
        return Err(if hints.is_empty() {
            format!("symbol `{q}` not found")
        } else {
            format!("symbol `{q}` not found. Did you mean: {}", hints.join(", "))
        });
    }
    Ok(ids)
}

fn find_one(g: &Graph, q: &str) -> Result<usize, String> {
    let ids = find_many(g, q)?;
    if ids.len() > 1 {
        let mut m = render::candidates(g, q, &ids);
        m.push_str(&format!("pin one with Name@LINE, e.g. {}@{}\n", g.short_name(ids[0]), g.syms[ids[0]].sym.start_line));
        return Err(m);
    }
    Ok(ids[0])
}

fn find(g: &Graph, q: &str) -> Result<Vec<usize>, String> {
    let ids = find_many(g, q)?;
    let first = &g.syms[ids[0]].sym.qualified;
    if ids.len() > 3 && ids.iter().any(|&i| &g.syms[i].sym.qualified != first) {
        return Err(render::candidates(g, q, &ids));
    }
    Ok(ids)
}

pub fn serve(ws: Workspace, profile: Profile) -> Result<()> {
    let mut server = Server::new(ws, profile)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(&mut stdout, &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } }))?;
                continue;
            }
        };
        if let Some(reply) = handle(&mut server, &msg) {
            write_msg(&mut stdout, &reply)?;
        }
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, v: &Value) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string(v)?)?;
    out.flush()?;
    Ok(())
}

/// Handle one JSON-RPC message; `None` for notifications.
fn handle(server: &mut Server, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = id?; // notifications (initialized, cancelled…) get no reply
    let result = match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "gitnext", "version": env!("CARGO_PKG_VERSION") },
                "instructions": match server.profile {
                    Profile::Lite => "Code tools: look(what) for any file, function, error or words; replace / add_after to change code. Don't read whole files.",
                    Profile::Full => "Code navigation and editing for this repository. Start with look: one call gives a symbol's code, callers, callees, tests and impact (pass several targets at once). New here? overview. \
                                 Edit with replace_symbol / insert_after / rename_symbol instead of rewriting files. \
                                 Use impact / tests_for before changing signatures; locate turns errors into code. \
                                 Resuming? brief, then changes. After understanding undocumented code, annotate it. Output is compact text with file:line locations.",
                }
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": match server.profile { Profile::Lite => lite_tools(), Profile::Full => tools() } })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let res = if server.profile == Profile::Lite && !agents::LITE_TOOLS.contains(&name) {
                Err(format!("unknown tool `{name}`; available: {}", agents::LITE_TOOLS.join(", ")))
            } else {
                server.call(name, &args)
            };
            // context packs have their own budget; everything else is capped
            let budget = server.profile.output_budget();
            let res = res.map(|t| if name == "context" { t } else { agents::cap_tokens(&t, budget) });
            let (out, naive) = server.journal(name, &args, &res);
            // savings go to _meta: visible to harnesses and humans, not paid for by the model
            let meta = json!({ "gitnext/tokens": out, "gitnext/naive_tokens": naive });
            match res {
                Ok(text) => Ok(json!({ "content": [{ "type": "text", "text": text }], "_meta": meta })),
                Err(e) => Ok(json!({ "content": [{ "type": "text", "text": e }], "isError": true, "_meta": meta })),
            }
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

/// Write (or merge into) `.mcp.json` at the repository root.
pub fn install(root: &Path) -> Result<PathBuf> {
    let path = root.join(".mcp.json");
    let mut cfg: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if !cfg.get("mcpServers").is_some_and(Value::is_object) {
        cfg["mcpServers"] = json!({});
    }
    cfg["mcpServers"]["gitnext"] = json!({ "command": "gitnext", "args": ["mcp"] });
    std::fs::write(&path, serde_json::to_string_pretty(&cfg)? + "\n")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_roundtrip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut s = Server::new(Workspace::open(&root).unwrap(), Profile::Full).unwrap();
        let init = handle(&mut s, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&mut s, &json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let list = handle(&mut s, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 16);
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"outline","arguments":{"path":"crates/gitnext-cli/src/mcp.rs"}}})).unwrap();
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("fn serve(ws: Workspace, profile: Profile)"), "{text}");
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read","arguments":{"symbols":["mcp::install"]}}})).unwrap();
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("```rust"));
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"show","arguments":{"symbol":"NoSuchThing123"}}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":6,"method":"bogus"})).unwrap();
        assert_eq!(r["error"]["code"], -32601);
        assert!(r.get("result").is_none());
    }

    #[test]
    fn lite_profile_is_small_and_capped() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut s = Server::new(Workspace::open(&root).unwrap(), Profile::Lite).unwrap();
        let list = handle(&mut s, &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), agents::LITE_TOOLS.len());
        let size = serde_json::to_string(tools).unwrap().len();
        assert!(size < 1400, "lite tool list is {size} bytes");
        let call = |s: &mut Server, what: &str| -> String {
            let r = handle(s, &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"look","arguments":{"what":what}}})).unwrap();
            r["result"]["content"][0]["text"].as_str().unwrap().to_string()
        };
        assert!(call(&mut s, "mcp.rs").contains("fn lite_tools"));
        assert!(call(&mut s, "lite_tools").contains("to change it: replace"));
        assert!(call(&mut s, "universal look").contains("text:"));
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"look","arguments":{"what":"crates"}}})).unwrap();
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.len() <= Profile::Lite.output_budget() * 4 + 200, "{}", text.len());
        assert!(r["result"]["_meta"]["gitnext/naive_tokens"].as_u64().unwrap() > 0);
        let r = handle(&mut s, &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"overview","arguments":{}}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
    }
}
