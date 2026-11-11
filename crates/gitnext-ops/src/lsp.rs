//! `gitnext lsp`: ask a real language server (rust-analyzer, pyright, clangd)
//! where *ambiguous* calls actually go, and cache the answers in the index.
//! Name-based resolution stays the default; this only sharpens the uncertain part.

use anyhow::{anyhow, bail, Context, Result};
use gitnext_index::{Graph, Index};
use gitnext_parse::{lang_for, Lang};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Rust,
    Python,
    C,
    Js,
    Go,
    Java,
}

impl Family {
    pub fn of(path: &str) -> Option<Family> {
        match lang_for(Path::new(path))? {
            Lang::Rust => Some(Family::Rust),
            Lang::Python => Some(Family::Python),
            Lang::C | Lang::Cpp => Some(Family::C),
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => Some(Family::Js),
            Lang::Go => Some(Family::Go),
            Lang::Java => Some(Family::Java),
        }
    }
    pub fn parse(s: &str) -> Option<Family> {
        match s {
            "rust" | "rs" => Some(Family::Rust),
            "python" | "py" => Some(Family::Python),
            "c" | "cpp" | "c++" => Some(Family::C),
            "ts" | "typescript" | "js" | "javascript" => Some(Family::Js),
            "go" => Some(Family::Go),
            "java" => Some(Family::Java),
            _ => None,
        }
    }
    fn command(self) -> Vec<String> {
        let (env, default) = match self {
            Family::Rust => ("GITNEXT_LSP_RUST", "rust-analyzer"),
            Family::Python => ("GITNEXT_LSP_PYTHON", "pyright-langserver --stdio"),
            Family::C => ("GITNEXT_LSP_C", "clangd"),
            Family::Js => ("GITNEXT_LSP_TS", "typescript-language-server --stdio"),
            Family::Go => ("GITNEXT_LSP_GO", "gopls"),
            Family::Java => ("GITNEXT_LSP_JAVA", "jdtls"),
        };
        std::env::var(env).unwrap_or_else(|_| default.into()).split_whitespace().map(str::to_string).collect()
    }
    fn language_id(self, path: &str) -> &'static str {
        match self {
            Family::Rust => "rust",
            Family::Python => "python",
            Family::Js => {
                if path.ends_with(".tsx") {
                    "typescriptreact"
                } else if path.ends_with(".jsx") {
                    "javascriptreact"
                } else if path.ends_with(".ts") || path.ends_with(".mts") || path.ends_with(".cts") {
                    "typescript"
                } else {
                    "javascript"
                }
            }
            Family::Go => "go",
            Family::Java => "java",
            Family::C => {
                if path.ends_with(".c") || path.ends_with(".h") {
                    "c"
                } else {
                    "cpp"
                }
            }
        }
    }
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
    active_progress: HashSet<String>,
    saw_progress: bool,
}

fn uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

fn path_of(u: &str) -> Option<PathBuf> {
    let p = u.strip_prefix("file://")?;
    // minimal percent-decoding (spaces etc.)
    let mut out = Vec::new();
    let b = p.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&p[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).to_string()))
}

impl Client {
    fn start(cmd: &[String], root: &Path) -> Result<Client> {
        let (bin, args) = cmd.split_first().ok_or_else(|| anyhow!("empty LSP command"))?;
        let mut child = Command::new(bin)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("cannot start `{}` (install it or set GITNEXT_LSP_*)", cmd.join(" ")))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let l = line.trim();
                    if l.is_empty() {
                        break;
                    }
                    if let Some(v) = l.strip_prefix("Content-Length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut buf = vec![0; len];
                if r.read_exact(&mut buf).is_err() {
                    return;
                }
                if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                    if tx.send(v).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Client { child, stdin, rx, next_id: 1, active_progress: HashSet::new(), saw_progress: false })
    }

    fn send(&mut self, v: &Value) -> Result<()> {
        let body = serde_json::to_string(v)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Handle a server→client message that isn't the response we wait for.
    fn on_other(&mut self, m: &Value) -> Result<()> {
        if let (Some(id), Some(method)) = (m.get("id"), m.get("method").and_then(Value::as_str)) {
            // server request: answer so it doesn't stall
            let result = match method {
                "workspace/configuration" => json!(m["params"]["items"].as_array().map(|a| a.iter().map(|_| Value::Null).collect::<Vec<_>>()).unwrap_or_default()),
                _ => Value::Null,
            };
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))?;
        } else if m.get("method").and_then(Value::as_str) == Some("$/progress") {
            let token = m["params"]["token"].to_string();
            match m["params"]["value"]["kind"].as_str() {
                Some("begin") => {
                    self.saw_progress = true;
                    self.active_progress.insert(token);
                }
                Some("end") => {
                    self.active_progress.remove(&token);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(m) => {
                    if m.get("id").and_then(Value::as_i64) == Some(id) && m.get("method").is_none() {
                        if let Some(e) = m.get("error") {
                            bail!("{method}: {e}");
                        }
                        return Ok(m.get("result").cloned().unwrap_or(Value::Null));
                    }
                    self.on_other(&m)?;
                }
                Err(RecvTimeoutError::Timeout) => bail!("{method}: timeout"),
                Err(RecvTimeoutError::Disconnected) => bail!("language server exited"),
            }
        }
    }

    /// Wait until background indexing reported via `$/progress` is over.
    fn wait_idle(&mut self, max: Duration) {
        let start = Instant::now();
        let mut quiet_since = Instant::now();
        while start.elapsed() < max {
            match self.rx.recv_timeout(Duration::from_millis(300)) {
                Ok(m) => {
                    let _ = self.on_other(&m);
                    quiet_since = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let idle = self.active_progress.is_empty() && quiet_since.elapsed() > Duration::from_millis(1500);
            if idle && (self.saw_progress || start.elapsed() > Duration::from_secs(4)) {
                return;
            }
        }
    }

    fn shutdown(mut self) {
        let _ = self.request("shutdown", Value::Null, Duration::from_secs(5));
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.wait();
    }
}

#[derive(Debug, Default, Serialize)]
pub struct LspReport {
    pub server: String,
    pub queried: usize,
    pub resolved: usize,
    pub external: usize,
    pub unknown: usize,
    pub seconds: f64,
}

fn utf16_col(line: &str, byte_col: usize) -> usize {
    line.get(..byte_col.min(line.len())).map(|s| s.encode_utf16().count()).unwrap_or(byte_col)
}

/// Refine ambiguous call edges of one language family with its language server.
pub fn refine(root: &Path, g: &Graph, fam: Family, limit: usize, all: bool) -> Result<LspReport> {
    let t0 = Instant::now();
    // ambiguous (or, with `all`, every) call site of this language
    let mut sites: Vec<(String, u32, u32)> = Vec::new();
    let mut seen = HashSet::new();
    for (file, e) in &g.edges {
        if e.kind != gitnext_parse::EdgeKind::Calls || Family::of(file) != Some(fam) || e.col == 0 && e.line == 0 {
            continue;
        }
        let n = g.resolve(file, e).len();
        // `.get()` & co are skipped by name-based resolution; ask the server when the repo has such methods
        let common = n == 0 && e.to.starts_with('.') && g.has_method_named(gitnext_parse::last_segment(&e.to));
        if ((all && n >= 1) || n > 1 || common) && seen.insert((file.clone(), e.line, e.col)) {
            sites.push((file.clone(), e.line, e.col));
        }
        if sites.len() >= limit {
            break;
        }
    }
    let cmd = fam.command();
    let mut rep = LspReport { server: cmd.join(" "), queried: sites.len(), ..Default::default() };
    if sites.is_empty() {
        return Ok(rep);
    }
    let mut c = Client::start(&cmd, root)?;
    let root_uri = uri(root);
    c.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "repo" }],
            "capabilities": {
                "textDocument": { "definition": { "linkSupport": true }, "synchronization": {} },
                "window": { "workDoneProgress": true },
                "workspace": { "configuration": true, "workspaceFolders": true }
            },
            "initializationOptions": {}
        }),
        Duration::from_secs(60),
    )?;
    c.notify("initialized", json!({}))?;
    // open the files we query
    let mut texts: HashMap<String, Vec<String>> = HashMap::new();
    for (file, _, _) in &sites {
        if texts.contains_key(file) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
        c.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri(&root.join(file)), "languageId": fam.language_id(file), "version": 1, "text": text } }),
        )?;
        texts.insert(file.clone(), text.lines().map(str::to_string).collect());
    }
    c.wait_idle(Duration::from_secs(180));

    let mut links: Vec<(String, u32, u32, String, u32)> = Vec::new();
    let canon_root = root.canonicalize().unwrap_or(root.to_path_buf());
    for (file, line, col) in &sites {
        let text = texts.get(file).and_then(|v| v.get(*line as usize - 1)).cloned().unwrap_or_default();
        let ch = utf16_col(&text, *col as usize);
        let params = json!({ "textDocument": { "uri": uri(&root.join(file)) }, "position": { "line": line - 1, "character": ch } });
        let mut res = c.request("textDocument/definition", params.clone(), Duration::from_secs(30)).unwrap_or(Value::Null);
        if res.is_null() || res.as_array().is_some_and(|a| a.is_empty()) {
            // still warming up: one retry after a short wait
            std::thread::sleep(Duration::from_millis(300));
            res = c.request("textDocument/definition", params, Duration::from_secs(30)).unwrap_or(Value::Null);
        }
        let first = match &res {
            Value::Array(a) => a.first().cloned(),
            Value::Object(_) => Some(res.clone()),
            _ => None,
        };
        let Some(loc) = first else {
            rep.unknown += 1;
            continue;
        };
        let u = loc.get("targetUri").or_else(|| loc.get("uri")).and_then(Value::as_str).unwrap_or("");
        let range = loc.get("targetSelectionRange").or_else(|| loc.get("range")).cloned().unwrap_or(Value::Null);
        let tline = range["start"]["line"].as_u64().unwrap_or(0) as u32 + 1;
        let target = path_of(u).and_then(|p| {
            let p = p.canonicalize().unwrap_or(p);
            p.strip_prefix(&canon_root).ok().map(gitnext_index::to_slash)
        });
        match target {
            Some(t) if g.files.iter().any(|f| f.path == t) => {
                rep.resolved += 1;
                links.push((file.clone(), *line, *col, t, tline));
            }
            _ => {
                rep.external += 1;
                links.push((file.clone(), *line, *col, String::new(), 0));
            }
        }
    }
    c.shutdown();
    let mut idx = Index::open(root)?;
    idx.store_links(&links)?;
    rep.seconds = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    Ok(rep)
}

pub fn families_in(g: &Graph) -> Vec<Family> {
    let mut v: Vec<Family> = Vec::new();
    for f in &g.files {
        if let Some(x) = Family::of(&f.path) {
            if !v.contains(&x) {
                v.push(x);
            }
        }
    }
    v
}
