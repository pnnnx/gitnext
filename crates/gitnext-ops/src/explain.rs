//! `gitnext explain`: one-line summaries for code without docs, written once by
//! an LLM (or by the agent itself via MCP `annotate`) and cached in
//! `.gitnext/notes.tsv` against the code's hash.
//!
//! Providers (first available wins unless `--provider` is given):
//! - `cmd`:       `GITNEXT_LLM_CMD` — any command reading the prompt on stdin (`claude -p`, `llm -m …`, `ollama run …`)
//! - `anthropic`: `ANTHROPIC_API_KEY` (+ `GITNEXT_MODEL`, default `claude-haiku-5-5`), via `curl`
//! - `ollama`:    `GITNEXT_OLLAMA_MODEL` against http://localhost:11434, via `curl`
//! - `openai`:    any OpenAI-compatible server — `GITNEXT_OPENAI_BASE` (e.g. llama.cpp
//!   `llama-server` at http://127.0.0.1:8080/v1, LM Studio, vLLM), optional
//!   `GITNEXT_OPENAI_MODEL` and `OPENAI_API_KEY`

use anyhow::{bail, Context, Result};
use gitnext_index::notes::{self, Note};
use gitnext_index::{dir_of, Graph};
use gitnext_parse::Kind;
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const BATCH: usize = 25;
const BODY_CHARS: usize = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Cmd,
    Anthropic,
    Ollama,
    OpenAi,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "cmd" => Some(Provider::Cmd),
            "anthropic" | "claude" => Some(Provider::Anthropic),
            "ollama" => Some(Provider::Ollama),
            "openai" | "llamacpp" | "llama.cpp" | "local" => Some(Provider::OpenAi),
            _ => None,
        }
    }

    pub fn detect() -> Option<Provider> {
        if std::env::var_os("GITNEXT_LLM_CMD").is_some() {
            Some(Provider::Cmd)
        } else if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
            Some(Provider::Anthropic)
        } else if std::env::var_os("GITNEXT_OLLAMA_MODEL").is_some() {
            Some(Provider::Ollama)
        } else if std::env::var_os("GITNEXT_OPENAI_BASE").is_some() {
            Some(Provider::OpenAi)
        } else {
            None
        }
    }

    fn label(self) -> String {
        match self {
            Provider::Cmd => format!("llm:{}", std::env::var("GITNEXT_LLM_CMD").unwrap_or_default().split_whitespace().next().unwrap_or("cmd")),
            Provider::Anthropic => format!("llm:{}", model()),
            Provider::Ollama => format!("llm:{}", std::env::var("GITNEXT_OLLAMA_MODEL").unwrap_or_default()),
            Provider::OpenAi => format!("llm:{}", std::env::var("GITNEXT_OPENAI_MODEL").unwrap_or_else(|_| "local".into())),
        }
    }
}

fn model() -> String {
    std::env::var("GITNEXT_MODEL").unwrap_or_else(|_| "claude-haiku-5-5".into())
}

/// Symbols (and directories) in `scope` that have no doc and no fresh note.
pub fn pending(g: &Graph, scope: &str) -> (Vec<usize>, Vec<String>) {
    let inside = |f: &str| scope.is_empty() || f == scope || f.starts_with(&format!("{scope}/"));
    let mut syms: Vec<usize> = (0..g.syms.len())
        .filter(|&i| {
            let r = &g.syms[i];
            let s = &r.sym;
            inside(&r.file)
                && s.summary.is_none()
                && !s.test
                && !s.decl
                && !(s.kind == Kind::Module && s.start_line == 1)
                && s.trait_impl.is_none() // `impl Display::fmt` explains itself
                && !matches!(s.kind, Kind::Const | Kind::Static)
        })
        .collect();
    // public API first, then bigger bodies
    syms.sort_by_key(|&i| (!g.syms[i].sym.is_public(), std::cmp::Reverse(g.syms[i].sym.end_line - g.syms[i].sym.start_line)));
    let dirs: Vec<String> = g
        .dirs()
        .into_iter()
        .filter(|d| inside(d) || (scope.is_empty()))
        .filter(|d| !g.dir_notes.contains_key(d) && g.files.iter().any(|f| f.parsed && dir_of(&f.path) == d))
        .collect();
    (syms, dirs)
}

fn excerpt(root: &Path, g: &Graph, i: usize) -> String {
    let r = &g.syms[i];
    let text = std::fs::read_to_string(root.join(&r.file)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let (a, b) = (r.sym.start_line as usize, (r.sym.end_line as usize).min(lines.len()));
    let mut s = String::new();
    if a >= 1 && a <= b {
        for l in &lines[a - 1..b] {
            if s.len() + l.len() > BODY_CHARS {
                s.push_str("  …\n");
                break;
            }
            s.push_str(l);
            s.push('\n');
        }
    }
    s
}

pub fn build_prompt(items: &[(String, String)]) -> String {
    let mut p = String::from(
        "You write one-line summaries for a code map read by developers and AI agents.\n\
         For EACH item reply with exactly one line: `<id> | <summary>`.\n\
         Summary: what it does and why it exists, at most 15 words, plain English, no code, \
         do not restate the name or the parameter list. Reply with the lines only.\n\n",
    );
    for (id, body) in items {
        p.push_str(&format!("[{id}]\n{body}\n"));
    }
    p
}

/// Parse `<id> | <summary>` lines.
pub fn parse_reply(reply: &str) -> Vec<(String, String)> {
    reply
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_start_matches(['-', '*', ' ']);
            let (id, sum) = l.split_once('|')?;
            let id = id.trim().trim_matches(['[', ']', '`', ' ']).to_string();
            let sum = sum.trim().trim_matches('`').trim().to_string();
            (!id.is_empty() && !sum.is_empty()).then_some((id, sum))
        })
        .collect()
}

fn run_curl(args: &[&str], body: &str) -> Result<String> {
    let mut child = Command::new("curl")
        .args(["-sS", "--max-time", "120"])
        .args(args)
        .args(["-d", "@-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("`curl` not found")?;
    child.stdin.take().unwrap().write_all(body.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn ask(provider: Provider, prompt: &str) -> Result<String> {
    match provider {
        Provider::Cmd => {
            let cmd = std::env::var("GITNEXT_LLM_CMD").context("GITNEXT_LLM_CMD is not set")?;
            let mut child = Command::new("sh")
                .args(["-c", &cmd])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .with_context(|| format!("cannot run `{cmd}`"))?;
            child.stdin.take().unwrap().write_all(prompt.as_bytes())?;
            let out = child.wait_with_output()?;
            if !out.status.success() {
                bail!("`{cmd}` exited with {}", out.status);
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        }
        Provider::Anthropic => {
            let key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY is not set")?;
            let body = serde_json::json!({
                "model": model(),
                "max_tokens": 2048,
                "messages": [{ "role": "user", "content": prompt }],
            })
            .to_string();
            let hdr_key = format!("x-api-key: {key}");
            let raw = run_curl(
                &["https://api.anthropic.com/v1/messages", "-H", &hdr_key, "-H", "anthropic-version: 2023-06-01", "-H", "content-type: application/json"],
                &body,
            )?;
            let v: serde_json::Value = serde_json::from_str(&raw).context("bad API response")?;
            if let Some(e) = v.get("error") {
                bail!("API error: {e}");
            }
            Ok(v["content"].as_array().map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default())
        }
        Provider::Ollama => {
            let m = std::env::var("GITNEXT_OLLAMA_MODEL").context("GITNEXT_OLLAMA_MODEL is not set")?;
            let body = serde_json::json!({ "model": m, "prompt": prompt, "stream": false }).to_string();
            let raw = run_curl(&["http://localhost:11434/api/generate"], &body)?;
            let v: serde_json::Value = serde_json::from_str(&raw).context("bad Ollama response")?;
            Ok(strip_think(v["response"].as_str().unwrap_or("")))
        }
        Provider::OpenAi => {
            let base = std::env::var("GITNEXT_OPENAI_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into());
            let url = format!("{}/chat/completions", base.trim_end_matches('/'));
            let model = std::env::var("GITNEXT_OPENAI_MODEL").unwrap_or_else(|_| "local".into());
            let body = serde_json::json!({
                "model": model,
                "temperature": 0.2,
                "max_tokens": 2048,
                "messages": [{ "role": "user", "content": prompt }],
            })
            .to_string();
            let auth = std::env::var("OPENAI_API_KEY").map(|k| format!("Authorization: Bearer {k}")).unwrap_or_default();
            let mut args = vec![url.as_str(), "-H", "content-type: application/json"];
            if !auth.is_empty() {
                args.extend(["-H", auth.as_str()]);
            }
            let raw = run_curl(&args, &body)?;
            let v: serde_json::Value = serde_json::from_str(&raw).with_context(|| format!("bad response from {url}"))?;
            if let Some(e) = v.get("error") {
                bail!("server error: {e}");
            }
            Ok(strip_think(v["choices"][0]["message"]["content"].as_str().unwrap_or("")))
        }
    }
}

/// Drop `<think>…</think>` blocks that reasoning models (Qwen3, DeepSeek-R1) put in the answer.
pub fn strip_think(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(a) = out.find("<think>") {
        match out[a..].find("</think>") {
            Some(b) => out.replace_range(a..a + b + "</think>".len(), ""),
            None => out.truncate(a),
        }
    }
    out.trim().to_string()
}

#[derive(Debug, Default, Serialize)]
pub struct ExplainReport {
    pub symbols_pending: usize,
    pub dirs_pending: usize,
    pub written: usize,
    pub batches: usize,
    pub prompt_tokens: usize,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Explain up to `limit` symbols (and the directories in scope) and store the notes.
pub fn explain(root: &Path, g: &Graph, scope: &str, provider: Option<Provider>, limit: usize, dry_run: bool) -> Result<ExplainReport> {
    let (syms, dirs) = pending(g, scope);
    let mut rep = ExplainReport { symbols_pending: syms.len(), dirs_pending: dirs.len(), dry_run, ..Default::default() };
    let syms: Vec<usize> = syms.into_iter().take(limit).collect();
    let provider = match provider.or_else(Provider::detect) {
        Some(p) => p,
        None if dry_run => Provider::Cmd,
        None => bail!(
            "no LLM configured. Set one of:\n  GITNEXT_LLM_CMD='claude -p'   (any command: prompt on stdin → answer on stdout)\n  ANTHROPIC_API_KEY=…            (optional GITNEXT_MODEL, default claude-haiku-5-5)\n  GITNEXT_OLLAMA_MODEL=llama3.2  (local Ollama)\n  GITNEXT_OPENAI_BASE=http://127.0.0.1:8080/v1  (llama.cpp llama-server, LM Studio, vLLM…)\nor let your agent write them with the MCP tool `annotate`."
        ),
    };
    let src = provider.label();
    for chunk in syms.chunks(BATCH) {
        let items: Vec<(String, String)> = chunk
            .iter()
            .enumerate()
            .map(|(n, &i)| {
                let r = &g.syms[i];
                (n.to_string(), format!("{} {} in {}\n{}", r.file.rsplit('.').next().unwrap_or(""), r.sym.qualified, r.file, excerpt(root, g, i)))
            })
            .collect();
        let prompt = build_prompt(&items);
        rep.prompt_tokens += prompt.len().div_ceil(4);
        rep.batches += 1;
        if dry_run {
            continue;
        }
        let reply = ask(provider, &prompt)?;
        let parsed = parse_reply(&reply);
        if std::env::var_os("GITNEXT_LLM_DEBUG").is_some() || (parsed.is_empty() && !reply.trim().is_empty()) {
            let head: String = reply.chars().take(300).collect();
            rep.warnings.push(format!("model reply had no `<id> | <summary>` lines: {head}"));
        }
        let notes = parsed.into_iter().filter_map(|(id, sum)| {
            let i = *chunk.get(id.parse::<usize>().ok()?)?;
            Some((g.syms[i].sym.qualified.clone(), Note { hash: g.syms[i].sym.body_hash.clone(), source: src.clone(), summary: sum }))
        });
        rep.written += notes::upsert(root, notes)?;
    }
    // directories: purpose from file list + API with (possibly fresh) summaries
    if !dirs.is_empty() {
        let fresh = {
            let mut idx = gitnext_index::Index::open(root)?;
            idx.refresh()?;
            idx.graph()?
        };
        let items: Vec<(String, String)> = dirs
            .iter()
            .enumerate()
            .map(|(n, d)| (n.to_string(), dir_brief(&fresh, d)))
            .collect();
        for (k, chunk) in items.chunks(BATCH).enumerate() {
            let prompt = build_prompt(chunk).replace("what it does and why it exists", "the purpose of this directory/module");
            rep.prompt_tokens += prompt.len().div_ceil(4);
            rep.batches += 1;
            if dry_run {
                continue;
            }
            let reply = ask(provider, &prompt)?;
            let notes = parse_reply(&reply).into_iter().filter_map(|(id, sum)| {
                let d = dirs.get(id.parse::<usize>().ok()? + k * BATCH)?;
                Some((format!("dir:{d}"), Note { hash: fresh.dir_hash(d), source: src.clone(), summary: sum }))
            });
            rep.written += notes::upsert(root, notes)?;
        }
    }
    Ok(rep)
}

/// Compact description of a directory for the LLM: files + public API lines.
pub fn dir_brief(g: &Graph, d: &str) -> String {
    let mut s = format!("directory {}/\n", if d.is_empty() { "." } else { d });
    for f in g.files.iter().filter(|f| dir_of(&f.path) == d) {
        s.push_str(&format!("file {}\n", f.path.rsplit('/').next().unwrap_or(&f.path)));
    }
    for r in g.syms.iter().filter(|r| dir_of(&r.file) == d && r.sym.is_public() && !r.sym.test && r.sym.kind != Kind::Module).take(30) {
        s.push_str(&format!("  {}{}\n", r.sym.signature, r.sym.summary.as_deref().map(|x| format!(" — {x}")).unwrap_or_default()));
    }
    s
}

/// Manual / agent-written note for one symbol.
pub fn annotate(root: &Path, g: &Graph, i: usize, summary: &str, source: &str) -> Result<()> {
    let s = &g.syms[i].sym;
    notes::upsert(root, [(s.qualified.clone(), Note { hash: s.body_hash.clone(), source: source.into(), summary: summary.into() })])?;
    Ok(())
}

pub fn annotate_dir(root: &Path, g: &Graph, dir: &str, summary: &str, source: &str) -> Result<()> {
    notes::upsert(root, [(format!("dir:{dir}"), Note { hash: g.dir_hash(dir), source: source.into(), summary: summary.into() })])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reply_parsing() {
        let r = parse_reply("0 | Parses one line.\n[1] | Loads all tasks\n- 2 | `Sums things`\ngarbage");
        assert_eq!(r, vec![("0".into(), "Parses one line.".into()), ("1".into(), "Loads all tasks".into()), ("2".into(), "Sums things".into())]);
    }
}
