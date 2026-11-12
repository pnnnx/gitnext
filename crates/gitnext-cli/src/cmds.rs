//! Extra commands (edits, history, insight, explain, LSP, bench…), kept out of main.rs.

use crate::{lookup, one, Out};
use anyhow::{bail, Result};
use clap::Subcommand;
use gitnext_ops::edit::{self, EditKind};
use gitnext_ops::explain;
use gitnext_ops::insight;
use gitnext_ops::Workspace;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum EditOp {
    /// Replace a symbol with new code (from --file or stdin). Include the doc comment to replace it too.
    Replace {
        symbol: String,
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Write even if the file would not parse
        #[arg(long)]
        force: bool,
    },
    /// Insert code (from --file or stdin) right after a symbol
    InsertAfter {
        symbol: String,
        #[arg(short, long)]
        file: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Delete a symbol (with its doc comment)
    Delete {
        symbol: String,
        #[arg(long)]
        force: bool,
    },
}

fn code_from(file: Option<PathBuf>) -> Result<String> {
    match file {
        Some(p) if p.as_os_str() != "-" => Ok(std::fs::read_to_string(p)?),
        _ => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
    }
}

pub fn edit(ws: &Workspace, op: EditOp, json: bool) -> Result<()> {
    let g = ws.graph()?;
    let (sym, kind, code, force) = match op {
        EditOp::Replace { symbol, file, force } => (symbol, EditKind::Replace, code_from(file)?, force),
        EditOp::InsertAfter { symbol, file, force } => (symbol, EditKind::InsertAfter, code_from(file)?, force),
        EditOp::Delete { symbol, force } => (symbol, EditKind::Delete, String::new(), force),
    };
    let i = one(&g, &sym)?;
    let r = edit::edit_symbol(&ws.root, &g, i, kind, &code, force)?;
    if json {
        println!("{}", serde_json::to_string(&r)?);
    } else {
        print!("{}", edit::render_edit(&r));
    }
    Ok(())
}

/// Commands flattened into the top-level CLI.
#[derive(Subcommand)]
pub enum More {
    /// Symbol-level diff between a revision (HEAD~3, branch, commit) and the working tree
    Since { rev: String },
    /// Write one-line summaries for undocumented symbols/directories with an LLM (cached in .gitnext/notes.tsv)
    Explain {
        /// File or directory (default: whole repo)
        path: Option<String>,
        /// cmd | anthropic | ollama | openai (llama.cpp) (default: first configured)
        #[arg(long)]
        provider: Option<String>,
        /// Max symbols per run
        #[arg(short = 'n', long, default_value_t = 200)]
        limit: usize,
        /// Only show what would be sent
        #[arg(long)]
        dry_run: bool,
    },
    /// Store your own one-line summary for a symbol (or `dir:<path>`)
    Annotate { target: String, summary: Vec<String> },
    /// Map an error / panic / traceback (argument or stdin) to the symbols it points at
    Locate {
        text: Vec<String>,
        /// Don't print the code of the top symbol
        #[arg(long)]
        no_code: bool,
    },
    /// Resolve ambiguous calls precisely with language servers (rust-analyzer, pyright, clangd); cached
    Lsp {
        /// rust | python | c (default: every language in the repo)
        #[arg(long)]
        lang: Option<String>,
        #[arg(short = 'n', long, default_value_t = 3000)]
        limit: usize,
        /// Query every call site, not only ambiguous ones
        #[arg(long)]
        all: bool,
    },
    /// When a symbol appeared and how it changed, commit by commit
    History {
        symbol: String,
        /// Max commits to scan
        #[arg(short = 'n', default_value_t = 300)]
        limit: usize,
    },
    /// Semantic changelog of a range (`main..feature`, `v1.2..`), with breaking changes
    Changelog {
        #[arg(default_value = "HEAD~10..HEAD")]
        range: String,
    },
    /// One screen: what is in this repo and what is going on (areas, entry points, deps, activity, health)
    Overview,
    /// Commit activity: areas, authors, most changed files and symbols, branches
    Activity {
        #[arg(short = 'n', default_value_t = 200)]
        limit: usize,
    },
    /// Risky files: frequently changed × big × many dependants
    Hotspots {
        #[arg(short = 'n', default_value_t = 15)]
        top: usize,
    },
    /// Symbols nothing in the repo references
    Dead,
    /// Dependency cycles between directories
    Cycles,
    /// main functions, HTTP routes, CLI commands, scripts
    Entrypoints,
    /// What the agent did through MCP: calls, symbols read, edits, tokens saved
    Journal {
        /// How many recent sessions
        #[arg(short = 'n', default_value_t = 1)]
        sessions: usize,
    },
    /// Measure token cost of typical agent tasks here: naive (grep + read files) vs gitnext
    Bench {
        #[arg(short = 'n', long, default_value_t = 30)]
        symbols: usize,
        #[arg(long, default_value_t = 10)]
        dirs: usize,
    },
    /// Connect agents: MCP config + a short instruction block (Claude Code, Cursor, Codex, loca/llama.cpp)
    Setup {
        /// claude, cursor, codex, loca (default: the ones detected in the repo, else claude)
        agents: Vec<String>,
        /// Every supported agent
        #[arg(long)]
        all: bool,
        /// Claude Code: also install hooks (outline instead of whole reads of big code files; brief at session start)
        #[arg(long)]
        hooks: bool,
        /// loca: also add gitnext's MCP tools (by default only the compact map)
        #[arg(long)]
        tools: bool,
    },
    /// Where were we: branch, uncommitted symbol changes, last agent session, recent commits
    Brief,
    /// External libraries: which the code calls (by package, with declared versions) and which are only declared
    Libs,
    /// One call for any target: symbol → code + callers + callees + tests + impact; file/folder → outline;
    /// error text → where; words → where they appear. Several targets at once.
    Look {
        #[arg(required = true)]
        targets: Vec<String>,
    },
    /// Check that everything is in place for agents (binary on PATH, index, map, agent configs, model server)
    Doctor,
    /// Take back the last commit; its changes stay staged (refuses if already pushed).
    /// `--op N` undoes one gitnext edit instead (see `gitnext ops`)
    Undo {
        /// operation number from `gitnext ops`; other changes stay as they are
        #[arg(long)]
        op: Option<u64>,
    },
    /// Edits made through gitnext (CLI and MCP), newest first: who, what, which files; `undo --op N` takes one back
    Ops {
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
    },
    /// Branches: ahead / behind and what each one changes
    Branches,
    /// Every earlier point of HEAD (commits, resets, checkouts); `--to N` goes back to one
    Timeline {
        /// go back to entry N (soft: files stay as they are)
        #[arg(long)]
        to: Option<usize>,
        #[arg(short = 'n', default_value_t = 20)]
        limit: usize,
    },
    /// Suggest a commit message from the staged (or all) changes
    Msg,
    /// Hooks for agent harnesses (reads the hook JSON on stdin): `read`, `session-start`
    Hook { event: String },
    /// Run the tests once with coverage and remember which test runs which function (Python / pytest-cov);
    /// `tests-for`, `look` and `check` then name the tests that really execute a function
    LearnTests,
    /// A test fails: run it and get the failure in a few lines and the code it reaches, the most likely
    /// culprit first (exit 1 while it fails). `greater_than` finds `test_greater_than`
    Why {
        /// Test names
        names: Vec<String>,
        /// Seconds the test command may take
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Build or test output (stdin, or the arguments) in a few lines: what failed, where, the message —
    /// `cargo test 2>&1 | gitnext digest`
    Digest {
        text: Vec<String>,
        /// Failures printed in full; the rest are counted and named
        #[arg(long, default_value_t = 5)]
        max: usize,
    },
    /// After a change: build the project and run the tests of what changed; prints a verdict (exit 1 if it fails).
    /// With test names: why they fail (the same as `gitnext why <test>`)
    Check {
        /// Tests to explain instead of checking the change
        names: Vec<String>,
        /// Seconds each command may take
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Tests that exercise a symbol: the 10 most likely to fail first, with the command to run them
    TestsFor {
        symbol: String,
        #[arg(short, long, default_value_t = 4)]
        depth: usize,
        /// Name every test, not only the 10 most likely
        #[arg(short, long)]
        all: bool,
    },
}

pub fn more(cwd: &Path, m: More, out: &Out) -> Result<()> {
    let ws = Workspace::open(cwd)?;
    // `check <test>` is `why <test>`
    let m = match m {
        More::Check { names, timeout } if !names.is_empty() => More::Why { names, timeout },
        other => other,
    };
    match m {
        More::Explain { path, provider, limit, dry_run } => {
            let g = ws.graph()?;
            let scope = crate::norm_path(&ws, path.as_deref().unwrap_or(""));
            let p = match provider.as_deref() {
                Some(x) => Some(explain::Provider::parse(x).ok_or_else(|| anyhow::anyhow!("unknown provider `{x}`"))?),
                None => None,
            };
            let r = explain::explain(&ws.root, &g, &scope, p, limit, dry_run)?;
            if out.json {
                return out.json(&r);
            }
            if dry_run {
                println!(
                    "would explain {} symbols (+{} dirs) in {} batches, ≈ {} prompt tokens",
                    r.symbols_pending.min(limit), r.dirs_pending, r.batches, r.prompt_tokens
                );
            } else {
                println!("✓ {} notes written to .gitnext/notes.tsv ({} batches, ≈ {} prompt tokens)", r.written, r.batches, r.prompt_tokens);
            }
            for w in &r.warnings {
                eprintln!("warning: {w}");
            }
        }
        More::Annotate { target, summary } => {
            let g = ws.graph()?;
            let text = summary.join(" ");
            if text.trim().is_empty() {
                bail!("give the summary text");
            }
            if let Some(d) = target.strip_prefix("dir:") {
                explain::annotate_dir(&ws.root, &g, d.trim_end_matches('/'), &text, "user")?;
            } else {
                let i = one(&g, &target)?;
                explain::annotate(&ws.root, &g, i, &text, "user")?;
            }
            println!("✓ noted");
        }
        More::Locate { text, no_code } => {
            let g = ws.graph()?;
            let t = if text.is_empty() {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                text.join(" ")
            };
            out.text(&gitnext_ops::locate::render_locate(&g, &ws.root, &t, !no_code));
        }
        More::LearnTests => {
            let g = ws.graph()?;
            out.text(&gitnext_ops::learn::render(&gitnext_ops::learn::learn(&ws.root, &g)?));
        }
        More::Why { names, timeout } => {
            let g = ws.graph()?;
            if names.is_empty() {
                bail!("which test? `gitnext why <test name>`");
            }
            let mut ids: Vec<usize> = Vec::new();
            for t in &names {
                let found = gitnext_ops::why::find_tests(&g, t);
                if found.is_empty() {
                    let near: Vec<&str> = gitnext_ops::why::similar_tests(&g, t).into_iter().map(|i| g.syms[i].sym.name.as_str()).collect();
                    bail!("no test named `{t}`{}", if near.is_empty() { String::new() } else { format!("; close: {}", near.join(", ")) });
                }
                ids.extend(found.into_iter().filter(|i| !ids.contains(i)).collect::<Vec<_>>());
            }
            let w = gitnext_ops::why::why(&ws.root, &g, &ids, std::time::Duration::from_secs(timeout));
            out.text(&gitnext_ops::why::render(&g, &ws.root, &w));
            if w.failed() {
                std::process::exit(1);
            }
        }
        More::Digest { text, max } => {
            let t = if text.is_empty() {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                text.join(" ")
            };
            out.text(&gitnext_ops::digest::render(&gitnext_ops::digest::digest(&t), max));
        }
        More::Check { timeout, .. } => {
            let g = ws.graph()?;
            let c = gitnext_ops::check::check(&ws.root, &g, std::time::Duration::from_secs(timeout));
            out.text(&gitnext_ops::check::render(&g, &c));
            if !c.ok() {
                std::process::exit(1);
            }
        }
        More::TestsFor { symbol, depth, all } => {
            let g = ws.graph()?;
            for i in lookup(&g, &symbol)? {
                out.text(&gitnext_ops::locate::render_tests_for(&g, &ws.root, i, depth, all));
            }
        }
        More::Lsp { lang, limit, all } => {
            let g = ws.graph()?;
            let fams = match lang.as_deref() {
                Some(l) => vec![gitnext_ops::lsp::Family::parse(l).ok_or_else(|| anyhow::anyhow!("unknown language `{l}`"))?],
                None => gitnext_ops::lsp::families_in(&g),
            };
            for f in fams {
                match gitnext_ops::lsp::refine(&ws.root, &g, f, limit, all) {
                    Ok(r) if out.json => out.json(&r)?,
                    Ok(r) if r.queried == 0 => println!("{:?}: nothing ambiguous", f),
                    Ok(r) => println!(
                        "✓ {}: {} call sites → {} resolved in repo, {} external, {} unknown ({:.1}s)",
                        r.server, r.queried, r.resolved, r.external, r.unknown, r.seconds
                    ),
                    Err(e) => println!("✗ {:?}: {e:#}", f),
                }
            }
        }
        More::History { symbol, limit } => {
            let g = ws.graph()?;
            let i = one(&g, &symbol)?;
            let ev = gitnext_ops::history::history(ws.repo()?, &g, i, limit)?;
            if out.json {
                return out.json(&ev);
            }
            out.text(&gitnext_ops::history::render_history(&g, i, &ev));
        }
        More::Changelog { range } => {
            let g = ws.graph()?;
            let c = gitnext_ops::history::changelog(ws.repo()?, &g, &range)?;
            if out.json {
                return out.json(&c);
            }
            out.text(&gitnext_ops::history::render_changelog(&c));
        }
        More::Overview => {
            let g = ws.graph()?;
            out.text(&insight::overview(ws.repo.as_ref(), &g, &ws.root));
        }
        More::Activity { limit } => {
            let a = insight::activity(ws.repo()?, limit)?;
            if out.json {
                return out.json(&a);
            }
            out.text(&insight::render_activity(&a));
        }
        More::Hotspots { top } => {
            let g = ws.graph()?;
            let h = insight::hotspots(ws.repo.as_ref(), &g, 500, top)?;
            if out.json {
                return out.json(&h);
            }
            out.text(&insight::render_hotspots(&h));
        }
        More::Dead => {
            let g = ws.graph()?;
            let d = insight::dead(&g, &ws.root);
            if out.json {
                return out.json(&d);
            }
            out.text(&insight::render_dead(&d));
        }
        More::Cycles => {
            let g = ws.graph()?;
            let c = insight::cycles(&g);
            if out.json {
                return out.json(&c);
            }
            out.text(&insight::render_cycles(&g, &c));
        }
        More::Entrypoints => {
            let g = ws.graph()?;
            let e = insight::entrypoints(&g, &ws.root);
            if out.json {
                return out.json(&e);
            }
            out.text(&insight::render_entrypoints(&e));
        }
        More::Journal { sessions } => {
            let j = gitnext_ops::journal::load(&ws.root);
            if out.json {
                return out.json(&j);
            }
            out.text(&gitnext_ops::journal::render(&j, sessions));
        }
        More::Bench { symbols, dirs } => {
            let g = ws.graph()?;
            let b = gitnext_ops::bench::run(&ws.root, &g, symbols, dirs);
            if out.json {
                return out.json(&b);
            }
            out.text(&gitnext_ops::bench::render(&b));
        }
        More::Setup { agents, all, hooks, tools } => {
            use gitnext_ops::agents::{self as ag, Agent};
            let mut list: Vec<Agent> = if all {
                Agent::ALL.to_vec()
            } else {
                agents.iter().map(|a| Agent::parse(a).ok_or_else(|| anyhow::anyhow!("unknown agent `{a}` (claude, cursor, codex, loca)"))).collect::<Result<_>>()?
            };
            if list.is_empty() {
                list = Agent::ALL.into_iter().filter(|a| a.detected(&ws.root)).collect();
                if list.is_empty() {
                    list.push(Agent::Claude);
                }
            }
            list.sort();
            list.dedup();
            if !ws.root.join(gitnext_ops::gitnext_index::GITNEXT_DIR).is_dir() {
                let g = ws.graph()?;
                ws.write_map(&g)?;
            }
            let r = ag::setup(&ws.root, &ag::SetupOpts { agents: list, hooks, command: "gitnext".into(), tools })?;
            // settings may change what AGENTS.md looks like
            let g = ws.graph()?;
            ws.write_map(&g)?;
            if out.json {
                return out.json(&r);
            }
            out.text(&ag::render_setup(&r));
            if !hooks && r.configured.iter().any(|(a, _)| a == "Claude Code") {
                println!("  tip: `gitnext setup claude --hooks` also answers whole reads of big code files with an outline");
            }
        }
        More::Undo { op: Some(id) } => {
            let r = gitnext_ops::oplog::undo(&ws.root, id)?;
            if out.json {
                return out.json(&r);
            }
            out.text(&gitnext_ops::oplog::render_undo(&r));
        }
        More::Ops { n } => {
            let ops = gitnext_ops::oplog::load(&ws.root);
            if out.json {
                return out.json(&ops.iter().rev().take(n).collect::<Vec<_>>());
            }
            out.text(&gitnext_ops::oplog::render(&ops, n));
        }
        More::Undo { op: None } => {
            let c = ws.repo()?.undo_commit().map_err(|e| anyhow::anyhow!(gitnext_ops::changes::humanize(&e)))?;
            println!("✓ took back {} “{}” — its changes are staged; edit and `gitnext commit` again", c.short, c.summary);
            println!("  (to redo it as it was: `gitnext timeline --to 1`)");
        }
        More::Branches => {
            let repo = ws.repo()?;
            let b = repo.branches()?;
            if out.json {
                return out.json(&b);
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let g = ws.graph()?;
            for b in b {
                let pl = gitnext_ops::changes::plural;
                let rel = match (b.ahead, b.behind) {
                    (0, 0) if b.upstream.is_some() => format!("in sync with {}", b.compared_to),
                    (0, 0) => String::new(),
                    (a, 0) => format!("{} ahead of {}", pl(a, "commit"), b.compared_to),
                    (0, h) => format!("{} behind {}", pl(h, "commit"), b.compared_to),
                    (a, h) => format!("{} ahead and {h} behind {} (diverged)", pl(a, "commit"), b.compared_to),
                };
                println!("{} {}{}", if b.current { "●" } else { " " }, b.name, if rel.is_empty() { String::new() } else { format!(" — {rel}") });
                println!("    last: {} · {} · {} ago", b.last_summary, b.last_author, gitnext_ops::agents::human_ago(now - b.last_time));
                if !b.current && b.ahead > 0 && b.upstream.is_none() {
                    if let Ok(cl) = gitnext_ops::history::changelog(repo, &g, &format!("{}..{}", b.compared_to, b.name)) {
                        for l in gitnext_ops::history::render_changelog(&cl).lines().skip(1).take_while(|l| !l.starts_with("commits")).filter(|l| !l.trim().is_empty()).take(6) {
                            println!("    {}", l.trim());
                        }
                    }
                }
            }
        }
        More::Timeline { to, limit } => {
            let repo = ws.repo()?;
            let log = repo.reflog(limit.max(to.map(|t| t + 1).unwrap_or(0)))?;
            if let Some(n) = to {
                let e = log.get(n).ok_or_else(|| anyhow::anyhow!("there is no entry {n}"))?;
                repo.reset_soft(&e.id)?;
                println!("✓ branch moved to {} ({}) — files unchanged, differences are staged", e.short, gitnext_ops::changes::human_reflog(&e.message));
                return Ok(());
            }
            if out.json {
                return out.json(&log);
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            for (i, e) in log.iter().enumerate() {
                println!("{i:>3}  {:>8} ago  {}  {}{}", gitnext_ops::agents::human_ago(now - e.time), e.short, gitnext_ops::changes::human_reflog(&e.message), if i == 0 { "   ← now" } else { "" });
            }
            println!("go back: gitnext timeline --to N (keeps your files; nothing is lost)");
        }
        More::Msg => {
            let cv = gitnext_ops::changes::changes(ws.repo()?)?;
            println!("{}", cv.draft);
        }
        More::Look { targets } => {
            let g = ws.graph()?;
            out.text(&gitnext_ops::look::look_many(&ws, &g, &targets, false));
        }
        More::Doctor => {
            let v = gitnext_ops::agents::doctor(&ws, env!("CARGO_PKG_VERSION"));
            if out.json {
                return out.json(&v);
            }
            print!("{}", gitnext_ops::agents::render_doctor(&v));
            if v.iter().any(|c| c.status == gitnext_ops::agents::Health::Fail) {
                std::process::exit(1);
            }
        }
        More::Libs => {
            let g = ws.graph()?;
            out.text(&gitnext_ops::libs::render_libraries(&ws.root, &g, 40));
        }
        More::Brief => {
            let g = ws.graph()?;
            out.text(&gitnext_ops::agents::brief(&ws, &g));
        }
        More::Hook { event } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
            match event.as_str() {
                "read" | "pre-read" => {
                    let min = std::env::var("GITNEXT_HOOK_MIN_TOKENS").ok().and_then(|x| x.parse().ok()).unwrap_or(2000);
                    if let gitnext_ops::agents::HookDecision::Block(msg) = gitnext_ops::agents::hook_read(&v, min) {
                        // Claude Code: exit code 2 blocks the tool call and shows stderr to the model
                        eprint!("{msg}");
                        std::process::exit(2);
                    }
                }
                "session-start" => {
                    // stdout of a SessionStart hook is added to the agent's context
                    let cwd = v["cwd"].as_str().map(PathBuf::from).unwrap_or_else(|| ws.root.clone());
                    let ws = Workspace::open(&cwd)?;
                    if ws.root.join(gitnext_ops::gitnext_index::GITNEXT_DIR).is_dir() {
                        let g = ws.graph()?;
                        print!("{}", gitnext_ops::agents::brief(&ws, &g));
                    }
                }
                other => bail!("unknown hook `{other}` (read, session-start)"),
            }
        }
        More::Since { rev } => {
            let d = gitnext_ops::since(ws.repo()?, &rev)?;
            if out.json {
                return out.json(&d);
            }
            out.text(&gitnext_ops::render_diff(&d, out.opts.full_doc));
        }
    }
    Ok(())
}


