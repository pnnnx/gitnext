//! Agent integrations.
//!
//! - [`setup`]: one command that wires gitnext into the agents used in a repo —
//!   MCP config (`.mcp.json`, `.cursor/mcp.json`, `.loca/config.json`), a short
//!   instruction block (CLAUDE.md, `.cursor/rules/gitnext.mdc`, LOCA.md) and,
//!   optionally, Claude Code hooks.
//! - [`brief`]: a short "where were we" handoff for the start of a session.
//! - [`hook_read`]: Claude Code `PreToolUse` hook for `Read` — a whole read of a
//!   big code file is answered with its outline first (the agent can repeat the
//!   read to get the file anyway).
//!
//! The instruction texts here are the "system prompts" gitnext gives agents.
//! They are short on purpose: every line is paid for on every request.

use crate::{journal, semantic_diff, DiffMode, Workspace};
use anyhow::Result;
use gitnext_index::{Graph, GITNEXT_DIR};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const BLOCK_START: &str = "<!-- gitnext:agent:start — written by `gitnext setup`, safe to delete -->";
pub const BLOCK_END: &str = "<!-- gitnext:agent:end -->";

/// MCP tool set offered to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Every tool, full descriptions (Claude Code, Cursor, Codex…).
    Full,
    /// Few tools, terse descriptions, small outputs: 3–14B local models with 8–32k context.
    Lite,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        match s {
            "full" => Some(Profile::Full),
            "lite" | "local" | "small" => Some(Profile::Lite),
            _ => None,
        }
    }
    /// Max tokens of one tool result (`GITNEXT_MCP_MAX_TOKENS` overrides).
    pub fn output_budget(self) -> usize {
        std::env::var("GITNEXT_MCP_MAX_TOKENS").ok().and_then(|v| v.parse().ok()).unwrap_or(match self {
            Profile::Full => 10_000,
            Profile::Lite => 1_500,
        })
    }
}

/// Tools of the lite profile: one way to look at anything, two ways to change code.
pub const LITE_TOOLS: &[&str] = &["look", "replace", "add_after"];

/// Instruction block for capable agents (CLAUDE.md, Cursor rules).
pub fn instructions_full() -> String {
    "## Code navigation: gitnext\n\n\
     This repo is indexed by gitnext (MCP server `gitnext`, or the `gitnext` command). \
     Every extra step re-sends the whole conversation, so prefer one call that answers everything:\n\n\
     1. `look` first. For a function / class / method it returns the code, who calls it, what it calls, \
     the tests that reach it with the command to run them, and what a change would affect — one call \
     instead of read + grep + callers + test search. Pass several targets at once \
     (`look` with `targets: [\"Agent.execute\", \"brief\", \"loca/config.py\"]`, or `gitnext look A B C`).\n\
     2. A file or folder in `look` gives every signature with line numbers; an error or traceback gives the code where it happened.\n\
     3. Edit by symbol: `replace_symbol` / `insert_after` / `rename_symbol` (rename also fixes imports, `__all__`, tests and \
     `module.func` uses, and lists any text it did not change) — no need to read the file first.\n\
     4. New repo: `overview` once. Resuming: `brief`, then `changes`.\n\
     5. Worked out what undocumented code does? `annotate` it in one line; later sessions get it for free.\n\n\
     Plain reads are fine for non-code files and small files. Run the tests yourself — gitnext does not run them.\n"
        .into()
}

/// Instruction block for small local models: imperative, a few lines, exact calls.
pub fn instructions_lite() -> String {
    "## Code tools (gitnext)\n\n\
     Use `mcp__gitnext__look` before reading files. It takes one string `what`:\n\
     - a file or folder → its functions and classes with line numbers: {\"what\": \"utils.py\"}\n\
     - a function or class → its code: {\"what\": \"parse_date\"} or {\"what\": \"Task.reopen\"}\n\
     - an error or traceback → the code where it happened\n\
     - words → where they appear: {\"what\": \"retry delay\"}\n\
     To change one function: `mcp__gitnext__replace` {\"symbol\": \"parse_date\", \"code\": \"<the whole new function>\"}.\n\
     To add a new function after another: `mcp__gitnext__add_after` with the same fields.\n"
        .into()
}

/// Replace (or append) the gitnext block in a text file, keeping the rest.
pub fn splice(path: &Path, block: &str) -> Result<bool> {
    let full = format!("{BLOCK_START}\n{}\n{BLOCK_END}", block.trim_end());
    let old = std::fs::read_to_string(path).ok();
    let key = BLOCK_START.split(" —").next().unwrap();
    let new = match &old {
        Some(t) => match (t.find(key), t.find(BLOCK_END)) {
            (Some(a), Some(b)) if b > a => format!("{}{full}{}", &t[..a], &t[b + BLOCK_END.len()..]),
            _ if t.trim().is_empty() => format!("{full}\n"),
            _ => format!("{}\n\n{full}\n", t.trim_end()),
        },
        None => format!("{full}\n"),
    };
    if old.as_deref() == Some(new.as_str()) {
        return Ok(false);
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, new)?;
    Ok(true)
}

fn load_json(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn save_json(path: &Path, v: &Value) -> Result<bool> {
    let new = serde_json::to_string_pretty(v)? + "\n";
    if std::fs::read_to_string(path).ok().as_deref() == Some(new.as_str()) {
        return Ok(false);
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, new)?;
    Ok(true)
}

/// Merge `server` as `<key>.gitnext` into a JSON config file.
fn merge_server(path: &Path, key: &str, server: Value) -> Result<bool> {
    let mut cfg = load_json(path);
    if !cfg.get(key).is_some_and(Value::is_object) {
        cfg[key] = json!({});
    }
    cfg[key]["gitnext"] = server;
    save_json(path, &cfg)
}

/// Agents `setup` can configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Agent {
    Claude,
    Cursor,
    Codex,
    Loca,
}

impl Agent {
    pub const ALL: [Agent; 4] = [Agent::Claude, Agent::Cursor, Agent::Codex, Agent::Loca];
    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "claude" | "claude-code" => Some(Agent::Claude),
            "cursor" => Some(Agent::Cursor),
            "codex" => Some(Agent::Codex),
            "loca" | "local" | "llamacpp" | "llama.cpp" => Some(Agent::Loca),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Cursor => "Cursor",
            Agent::Codex => "Codex",
            Agent::Loca => "loca (llama.cpp / Ollama)",
        }
    }
    /// Is there a sign this agent is used in the repo?
    pub fn detected(self, root: &Path) -> bool {
        let e = |p: &str| root.join(p).exists();
        match self {
            Agent::Claude => e("CLAUDE.md") || e(".claude") || e(".mcp.json"),
            Agent::Cursor => e(".cursor") || e(".cursorrules"),
            Agent::Codex => e(".codex"),
            Agent::Loca => e(".loca") || e("LOCA.md"),
        }
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SetupReport {
    /// (agent, files written or updated)
    pub configured: Vec<(String, Vec<String>)>,
    /// files already up to date
    pub unchanged: Vec<String>,
    /// things the user has to do by hand
    pub manual: Vec<String>,
}

pub struct SetupOpts {
    pub agents: Vec<Agent>,
    /// Claude Code: install the Read / SessionStart hooks
    pub hooks: bool,
    /// command that starts gitnext (default `gitnext`)
    pub command: String,
    /// loca: also add the MCP tools. Off by default: in tests a 4B model never
    /// called them (it has its own repo_map / read_symbol); the map is what helps.
    pub tools: bool,
}

/// Wire gitnext into the given agents. Idempotent; only touches gitnext's own entries/blocks.
pub fn setup(root: &Path, o: &SetupOpts) -> Result<SetupReport> {
    let mut r = SetupReport::default();
    let cmd = o.command.as_str();
    for &a in &o.agents {
        let mut written: Vec<String> = Vec::new();
        let mut track = |rel: &str, changed: bool, r: &mut SetupReport| {
            if changed {
                written.push(rel.to_string())
            } else {
                r.unchanged.push(rel.to_string())
            }
        };
        match a {
            Agent::Claude => {
                let c = merge_server(&root.join(".mcp.json"), "mcpServers", json!({ "command": cmd, "args": ["mcp"] }))?;
                track(".mcp.json", c, &mut r);
                let c = splice(&root.join("CLAUDE.md"), &instructions_full())?;
                track("CLAUDE.md", c, &mut r);
                if o.hooks {
                    let c = install_claude_hooks(&root.join(".claude/settings.json"), cmd)?;
                    track(".claude/settings.json", c, &mut r);
                }
            }
            Agent::Cursor => {
                let c = merge_server(&root.join(".cursor/mcp.json"), "mcpServers", json!({ "command": cmd, "args": ["mcp"] }))?;
                track(".cursor/mcp.json", c, &mut r);
                let rule = format!(
                    "---\ndescription: Navigate and edit code through the gitnext symbol index\nalwaysApply: true\n---\n\n{}",
                    instructions_full()
                );
                let p = root.join(".cursor/rules/gitnext.mdc");
                let c = std::fs::read_to_string(&p).ok().as_deref() != Some(rule.as_str());
                if c {
                    std::fs::create_dir_all(p.parent().unwrap())?;
                    std::fs::write(&p, &rule)?;
                }
                track(".cursor/rules/gitnext.mdc", c, &mut r);
            }
            Agent::Codex => {
                // Codex reads AGENTS.md (gitnext's map block is there); MCP servers live in the global config.
                r.manual.push(format!(
                    "Codex: add to ~/.codex/config.toml:\n  [mcp_servers.gitnext]\n  command = \"{cmd}\"\n  args = [\"mcp\"]\n  cwd = \"{}\"",
                    root.display()
                ));
            }
            Agent::Loca => {
                if o.tools {
                    let c = merge_server(
                        &root.join(".loca/config.json"),
                        "mcp_servers",
                        json!({ "command": cmd, "args": ["mcp", "--profile", "lite"], "tools": LITE_TOOLS }),
                    )?;
                    track(".loca/config.json", c, &mut r);
                    let c = splice(&root.join("LOCA.md"), &instructions_lite())?;
                    track("LOCA.md", c, &mut r);
                } else {
                    r.manual.push("loca: the code map goes into AGENTS.md (loca reads it into the prompt). MCP tools not added — small models use loca's own repo_map/read_symbol instead; add them with `gitnext setup loca --tools`".into());
                }
                // loca also reads AGENTS.md into a small context: keep gitnext's block there short
                let c = gitnext_docs::set_config_value(root, "agents_md", "compact")?;
                track(".gitnext/config", c, &mut r);
            }
        }
        if !written.is_empty() {
            r.configured.push((a.name().to_string(), written));
        }
    }
    Ok(r)
}

/// Add gitnext's hooks to a Claude Code settings file (keeps every other hook).
fn install_claude_hooks(path: &Path, cmd: &str) -> Result<bool> {
    let mut cfg = load_json(path);
    if !cfg.get("hooks").is_some_and(Value::is_object) {
        cfg["hooks"] = json!({});
    }
    let mut put = |event: &str, matcher: Option<&str>, command: String| {
        let list = cfg["hooks"].get(event).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut list: Vec<Value> = list
            .into_iter()
            .filter(|h| !h["hooks"].as_array().is_some_and(|a| a.iter().any(|x| x["command"].as_str().is_some_and(|c| c.contains("gitnext hook")))))
            .collect();
        let mut entry = json!({ "hooks": [{ "type": "command", "command": command }] });
        if let Some(m) = matcher {
            entry["matcher"] = json!(m);
        }
        list.push(entry);
        cfg["hooks"][event] = Value::Array(list);
    };
    put("PreToolUse", Some("Read"), format!("{cmd} hook read"));
    put("SessionStart", None, format!("{cmd} hook session-start"));
    save_json(path, &cfg)
}

pub fn render_setup(r: &SetupReport) -> String {
    let mut s = String::new();
    for (a, files) in &r.configured {
        let _ = writeln!(s, "✓ {a}: {}", files.join(", "));
    }
    if r.configured.is_empty() && !r.unchanged.is_empty() {
        let _ = writeln!(s, "already set up ({})", r.unchanged.join(", "));
    }
    for m in &r.manual {
        let _ = writeln!(s, "→ {m}");
    }
    let mcp = r.configured.iter().flat_map(|(_, f)| f).any(|f| f.ends_with("mcp.json") || f == ".loca/config.json");
    if mcp {
        let _ = writeln!(s, "next: gitnext doctor · then restart the agent so it starts the gitnext MCP server");
    } else if !r.configured.is_empty() {
        let _ = writeln!(s, "next: gitnext doctor");
    }
    s
}

// ------------------------------------------------------------------ brief

/// "Where were we": branch, uncommitted symbol changes, the last agent session,
/// recent commits. A few hundred tokens — meant for the start of a session.
pub fn brief(ws: &Workspace, g: &Graph) -> String {
    let mut s = String::new();
    let repo = ws.repo.as_ref();
    let branch = repo.map(|r| r.branch()).unwrap_or_else(|| "(no git)".into());
    let parsed = g.files.iter().filter(|f| f.parsed).count();
    let _ = writeln!(s, "# brief · {} · branch {branch} · {parsed} code files", ws.root.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    if let Some(repo) = repo {
        let has_head = repo.inner.head().ok().and_then(|h| h.peel_to_commit().ok()).is_some();
        match semantic_diff(repo, DiffMode::Worktree) {
            Ok(d) if !d.is_empty() && !has_head => {
                let _ = writeln!(s, "no commits yet · {} files to commit", d.len());
            }
            Ok(mut d) if !d.is_empty() => {
                let n: usize = d.iter().map(|f| f.changes.len()).sum();
                let _ = writeln!(s, "uncommitted: {} files, {n} symbol changes", d.len());
                // code changes first, biggest first; other files only counted
                d.sort_by_key(|f| std::cmp::Reverse(f.changes.iter().filter(|c| !c.test).count()));
                let (code, other): (Vec<_>, Vec<_>) = d.iter().partition(|f| f.changes.iter().any(|c| !c.test));
                for f in code.iter().take(10) {
                    let lines: Vec<String> = f.changes.iter().filter(|c| !c.test).take(4).map(|c| c.line()).collect();
                    let more = f.changes.iter().filter(|c| !c.test).count().saturating_sub(lines.len());
                    let _ = writeln!(s, "  {}: {}{}", f.path, lines.join("; "), if more > 0 { format!(" (+{more})") } else { String::new() });
                }
                if code.len() > 10 {
                    let _ = writeln!(s, "  … {} more code files", code.len() - 10);
                }
                let (cosmetic, rest): (Vec<_>, Vec<_>) = other.into_iter().partition(|f| f.semantic && f.status == "modified");
                for (label, v) in [("comments/formatting only", cosmetic), ("other files", rest)] {
                    if !v.is_empty() {
                        let names: Vec<&str> = v.iter().take(5).map(|f| f.path.as_str()).collect();
                        let _ = writeln!(s, "  {label}: {}{}", names.join(", "), if v.len() > 5 { format!(" (+{})", v.len() - 5) } else { String::new() });
                    }
                }
            }
            Ok(_) => {
                let _ = writeln!(s, "working tree clean");
            }
            Err(_) => {}
        }
    }
    let entries = journal::load(&ws.root);
    if let Some((sid, es)) = journal::sessions(&entries).into_iter().next() {
        let mut edited: Vec<&str> = es.iter().flat_map(|e| e.edited.iter().map(String::as_str)).collect();
        edited.sort_unstable();
        edited.dedup();
        let mut read: Vec<&str> = es.iter().flat_map(|e| e.symbols.iter().map(String::as_str)).collect();
        read.sort_unstable();
        read.dedup();
        let ago = now().saturating_sub(es.last().map(|e| e.ts).unwrap_or(0));
        let _ = writeln!(s, "last agent session {sid}: {} calls, {} ago", es.len(), human_ago(ago));
        if !edited.is_empty() {
            let _ = writeln!(s, "  edited: {}", edited.iter().take(10).copied().collect::<Vec<_>>().join(", "));
        }
        if !read.is_empty() {
            let _ = writeln!(s, "  looked at: {}{}", read.iter().take(10).copied().collect::<Vec<_>>().join(", "), if read.len() > 10 { " …" } else { "" });
        }
        if let Some(e) = es.iter().rev().find(|e| e.error) {
            let _ = writeln!(s, "  last error: {} {}", e.tool, e.target);
        }
    }
    if let Some(repo) = repo {
        if let Ok(log) = repo.log(3) {
            if !log.is_empty() {
                let _ = writeln!(s, "recent commits:");
                for c in log {
                    let _ = writeln!(s, "  {} {} ({}, {})", c.short, c.summary, c.author, human_ago(now().saturating_sub(c.time)) + " ago");
                }
            }
        }
    }
    s
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn human_ago(secs: i64) -> String {
    match secs {
        s if s < 90 => format!("{s}s"),
        s if s < 5400 => format!("{} min", s / 60),
        s if s < 2 * 86_400 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

// ------------------------------------------------------------------ hooks

/// What a hook decided.
#[derive(Debug, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    /// Block with this message (shown to the agent).
    Block(String),
}

fn seen_path(root: &Path) -> PathBuf {
    root.join(GITNEXT_DIR).join("hook-seen.tsv")
}

/// Claude Code `PreToolUse` hook for `Read`. `input` is the hook's JSON payload.
///
/// A whole read (no offset/limit) of an indexed code file above `min_tokens`
/// is blocked once per session with the file's outline, so the agent can read
/// just the lines it needs. Repeating the same read is allowed.
pub fn hook_read(input: &Value, min_tokens: usize) -> HookDecision {
    let ti = &input["tool_input"];
    if ti.get("offset").is_some_and(|v| !v.is_null()) || ti.get("limit").is_some_and(|v| !v.is_null()) {
        return HookDecision::Allow;
    }
    let Some(file) = ti["file_path"].as_str() else { return HookDecision::Allow };
    let file = Path::new(file);
    let cwd = input["cwd"].as_str().map(PathBuf::from).or_else(|| file.parent().map(Path::to_path_buf)).unwrap_or_default();
    let Ok(ws) = Workspace::open(&cwd) else { return HookDecision::Allow };
    let abs = if file.is_absolute() { file.to_path_buf() } else { cwd.join(file) };
    let Ok(abs) = abs.canonicalize() else { return HookDecision::Allow };
    let Ok(rel) = abs.strip_prefix(&ws.root) else { return HookDecision::Allow };
    let rel = gitnext_index::to_slash(rel);
    let bytes = std::fs::metadata(&abs).map(|m| m.len() as usize).unwrap_or(0);
    let tokens = bytes.div_ceil(4);
    if tokens < min_tokens || !gitnext_parse::lang_for(Path::new(&rel)).is_some() {
        return HookDecision::Allow;
    }
    // only repos that use gitnext
    if !ws.root.join(GITNEXT_DIR).is_dir() {
        return HookDecision::Allow;
    }
    let session = input["session_id"].as_str().unwrap_or("-");
    let key = format!("{session}\t{rel}");
    let seen = std::fs::read_to_string(seen_path(&ws.root)).unwrap_or_default();
    if seen.lines().any(|l| l == key) {
        return HookDecision::Allow;
    }
    let Ok(g) = ws.graph() else { return HookDecision::Allow };
    let outline = crate::render::outline(&g, &ws.root, &rel, crate::render::Opts::default());
    let outline = cap_tokens(&outline, 2_500);
    // remember (keep the file small)
    let mut lines: Vec<&str> = seen.lines().collect();
    if lines.len() > 500 {
        lines.drain(..lines.len() - 400);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&key);
    out.push('\n');
    let _ = std::fs::write(seen_path(&ws.root), out);
    HookDecision::Block(format!(
        "gitnext: {rel} is ~{tokens} tokens. Its outline (~{} tokens) is below — read only what you need: \
         Read with offset/limit at these line numbers, or the gitnext MCP `read` tool with symbol names. \
         Repeat this exact Read if you really need the whole file.\n\n{outline}",
        outline.len().div_ceil(4)
    ))
}

/// Cut text to about `max` tokens at a line boundary.
pub fn cap_tokens(text: &str, max: usize) -> String {
    let limit = max * 4;
    if text.len() <= limit {
        return text.to_string();
    }
    let mut cut = text[..text.floor_char_boundary(limit)].rfind('\n').unwrap_or(limit.min(text.len()));
    cut = text.floor_char_boundary(cut);
    let rest = text[cut..].lines().filter(|l| !l.trim().is_empty()).count();
    format!("{}\n… {rest} more lines cut to fit ~{max} tokens. Narrow the request (one file, fewer symbols).\n", &text[..cut])
}

// ------------------------------------------------------------------ doctor

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, serde::Serialize)]
pub struct Check {
    pub status: Health,
    pub what: String,
    /// command or action that fixes it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

fn check(status: Health, what: impl Into<String>, fix: Option<&str>) -> Check {
    Check { status, what: what.into(), fix: fix.map(str::to_string) }
}

fn json_file(p: &Path) -> Option<Value> {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// Is everything in place for agents to use gitnext here? Each check says how to fix it.
pub fn doctor(ws: &Workspace, version: &str) -> Vec<Check> {
    let root = &ws.root;
    let mut v = Vec::new();
    // the binary agents will start
    let on_path = std::process::Command::new("gitnext").arg("--version").output();
    match on_path {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if out.ends_with(version) {
                v.push(check(Health::Ok, format!("`gitnext` on PATH is this version ({version})"), None));
            } else {
                v.push(check(Health::Warn, format!("`gitnext` on PATH is {out}, this one is {version}: agents will start the other one"), Some("cargo install --path crates/gitnext-cli   (in the gitnext source folder)")));
            }
        }
        _ => v.push(check(Health::Fail, "`gitnext` is not on PATH: agents cannot start the MCP server", Some("cargo install --path crates/gitnext-cli, and make sure ~/.cargo/bin is on PATH"))),
    }
    // repository and index
    if ws.repo.is_some() {
        v.push(check(Health::Ok, "git repository", None));
    } else {
        v.push(check(Health::Warn, "not a git repository: history, diff and commit tools won't work", Some("gitnext init")));
    }
    let set_up = root.join(GITNEXT_DIR).is_dir();
    if !set_up {
        v.push(check(Health::Fail, "gitnext is not set up in this folder", Some("gitnext init   (then: gitnext setup)")));
    }
    match if set_up { ws.index().map(Some) } else { Ok(None) } {
        Ok(None) => {}
        Ok(Some((_, st))) if st.files_total > 0 => {
            let c = ws.graph().map(|g| g.counts()).unwrap_or_default();
            if c.code_files == 0 {
                v.push(check(Health::Warn, "no Rust, Python, C or C++ files found: nothing to index", None));
            } else {
                v.push(check(Health::Ok, format!("index: {}", c.line()), None));
            }
        }
        Ok(Some(_)) => v.push(check(Health::Warn, "the folder is empty", None)),
        Err(e) => v.push(check(Health::Fail, format!("index cannot be opened: {e:#}"), Some("delete .gitnext/index.db and run `gitnext index`"))),
    }
    if root.join(GITNEXT_DIR).join("map").is_dir() && root.join("AGENTS.md").exists() {
        v.push(check(Health::Ok, "code map in .gitnext/map and AGENTS.md", None));
    } else if set_up {
        v.push(check(Health::Warn, "no code map yet: agents start without an overview", Some("gitnext map")));
    }
    // agents
    let mut any = false;
    let mcp = json_file(&root.join(".mcp.json"));
    if mcp.as_ref().is_some_and(|c| c["mcpServers"]["gitnext"].is_object()) {
        any = true;
        let claude_md = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap_or_default();
        v.push(check(Health::Ok, "Claude Code: MCP server in .mcp.json", None));
        if !claude_md.contains("gitnext:agent:start") {
            v.push(check(Health::Warn, "Claude Code: no gitnext block in CLAUDE.md (the agent may not know when to use the tools)", Some("gitnext setup claude")));
        }
        let hooks = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap_or_default().contains("gitnext hook");
        if !hooks {
            v.push(check(Health::Warn, "Claude Code: hooks not installed (optional: outline instead of whole-file reads, brief at session start)", Some("gitnext setup claude --hooks")));
        }
    }
    if json_file(&root.join(".cursor/mcp.json")).is_some_and(|c| c["mcpServers"]["gitnext"].is_object()) {
        any = true;
        v.push(check(Health::Ok, "Cursor: MCP server in .cursor/mcp.json", None));
    }
    let loca = json_file(&root.join(".loca/config.json"));
    let loca_map = gitnext_docs::config_value(root, "agents_md").as_deref() == Some("compact");
    if loca_map && !loca.as_ref().is_some_and(|c| c["mcp_servers"]["gitnext"].is_object()) {
        any = true;
        v.push(check(Health::Ok, "loca: compact code map in AGENTS.md (no MCP tools; `gitnext setup loca --tools` adds them)", None));
    }
    if let Some(cfg) = loca.as_ref().filter(|c| c["mcp_servers"]["gitnext"].is_object()) {
        any = true;
        let args = cfg["mcp_servers"]["gitnext"]["args"].to_string();
        if args.contains("lite") {
            v.push(check(Health::Ok, "loca: gitnext MCP (lite profile) in .loca/config.json", None));
        } else {
            v.push(check(Health::Warn, "loca: gitnext MCP without the lite profile (too many tools for a small model)", Some("gitnext setup loca")));
        }
        let allowed: Vec<String> = cfg["mcp_servers"]["gitnext"]["tools"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if !allowed.is_empty() && !LITE_TOOLS.iter().all(|t| allowed.iter().any(|a| a == t)) {
            v.push(check(Health::Fail, format!("loca: tool allowlist {allowed:?} is from an older gitnext (now {LITE_TOOLS:?})"), Some("gitnext setup loca")));
        }
        if !std::fs::read_to_string(root.join("LOCA.md")).unwrap_or_default().contains("mcp__gitnext__look") {
            v.push(check(Health::Warn, "loca: LOCA.md has no (current) gitnext instructions", Some("gitnext setup loca")));
        }
        // the model server loca talks to
        let home_cfg = std::env::var_os("HOME").map(|h| Path::new(&h).join(".loca/config.json")).and_then(|p| json_file(&p));
        let pick = |k: &str| cfg[k].as_str().map(str::to_string).or_else(|| home_cfg.as_ref().and_then(|h| h[k].as_str().map(str::to_string)));
        let backend = pick("backend").unwrap_or_else(|| "ollama".into());
        let url = if backend == "llamacpp" {
            format!("{}/health", pick("llamacpp_host").or_else(|| std::env::var("LLAMA_HOST").ok()).unwrap_or_else(|| "http://127.0.0.1:8080".into()).trim_end_matches('/'))
        } else {
            format!("{}/api/tags", pick("host").or_else(|| std::env::var("OLLAMA_HOST").ok()).unwrap_or_else(|| "http://localhost:11434".into()).trim_end_matches('/'))
        };
        let up = std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "2", &url]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "200").unwrap_or(false);
        if up {
            v.push(check(Health::Ok, format!("loca: model server answers ({backend}, {url})"), None));
        } else {
            let fix = if backend == "llamacpp" { "start it: llama-server -m <model>.gguf --jinja -c 16384 -fa on --port 8080  (or loca's bin/llama.sh)" } else { "start it: ollama serve" };
            v.push(check(Health::Warn, format!("loca: model server not reachable at {url} ({backend})"), Some(fix)));
        }
    }
    if !any {
        v.push(check(Health::Warn, "no agent is connected to gitnext in this repository", Some("gitnext setup   (or: gitnext setup claude | cursor | loca)")));
    }
    // optional: summaries for undocumented code
    match crate::explain::Provider::detect() {
        Some(_) => v.push(check(Health::Ok, "explain: an LLM is configured for summaries", None)),
        None => v.push(check(Health::Warn, "explain: no LLM configured (optional; agents can still annotate)", Some("export GITNEXT_OPENAI_BASE=http://127.0.0.1:8080/v1   (or ANTHROPIC_API_KEY / GITNEXT_LLM_CMD)"))),
    }
    v
}

pub fn render_doctor(v: &[Check]) -> String {
    let mut s = String::new();
    for c in v {
        let mark = match c.status {
            Health::Ok => "✓",
            Health::Warn => "!",
            Health::Fail => "✗",
        };
        let _ = writeln!(s, "{mark} {}", c.what);
        if let Some(f) = &c.fix {
            let _ = writeln!(s, "    → {f}");
        }
    }
    let fails = v.iter().filter(|c| c.status == Health::Fail).count();
    let warns = v.iter().filter(|c| c.status == Health::Warn).count();
    let _ = writeln!(s, "\n{}", match (fails, warns) {
        (0, 0) => "all set".to_string(),
        (0, w) => format!("ready, {w} optional improvement{}", if w == 1 { "" } else { "s" }),
        (f, _) => format!("{f} problem{} to fix before agents can use gitnext", if f == 1 { "" } else { "s" }),
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_is_idempotent_and_keeps_user_text() {
        let dir = std::env::temp_dir().join(format!("gitnext-splice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("CLAUDE.md");
        std::fs::write(&p, "# My rules\n\nbe nice\n").unwrap();
        assert!(splice(&p, "one").unwrap());
        assert!(!splice(&p, "one").unwrap());
        assert!(splice(&p, "two").unwrap());
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.starts_with("# My rules\n\nbe nice\n"));
        assert!(t.contains("two") && !t.contains("one"));
        assert_eq!(t.matches("gitnext:agent:start").count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hooks_merge_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("gitnext-hooks-{}", std::process::id()));
        let p = dir.join(".claude/settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lint"}]}]},"model":"x"}"#).unwrap();
        install_claude_hooks(&p, "gitnext").unwrap();
        assert!(!install_claude_hooks(&p, "gitnext").unwrap());
        let v = load_json(&p);
        assert_eq!(v["model"], "x");
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cap_keeps_whole_lines() {
        let t: String = (0..200).map(|i| format!("line {i}\n")).collect();
        let c = cap_tokens(&t, 50);
        assert!(c.len() < 300 && c.contains("more lines cut"));
        assert!(c.lines().next().unwrap() == "line 0");
    }
}
