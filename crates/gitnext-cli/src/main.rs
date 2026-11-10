//! `gitnext` — a git built for AI agents: semantic code map, symbol graph,
//! semantic diffs and a minimal TUI, on top of a 100% git-compatible repo.

mod cmds;
mod mcp;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use gitnext_ops::gitnext_index::Graph;
use gitnext_ops::gitnext_parse::Kind;
use gitnext_ops::render::{self, Opts};
use gitnext_ops::{CommitOpts, DiffMode, Workspace};
use serde::Serialize;
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gitnext", version, about = "git for AI agents: code map, symbol graph, semantic diffs, TUI")]
#[command(after_help = "Run without a command to open the TUI. Unknown commands are passed to `git` (e.g. `gitnext push`).\n\
Output is compact plain text when piped (for agents) and colored in a terminal.")]
struct Cli {
    /// Machine-readable JSON output (compact; add --pretty to indent)
    #[arg(long, global = true)]
    json: bool,
    /// Indent JSON output
    #[arg(long, global = true)]
    pretty: bool,
    /// Include test code (normally collapsed into counts)
    #[arg(long, global = true)]
    tests: bool,
    /// Full doc comments instead of the first paragraph
    #[arg(long, global = true)]
    full: bool,
    /// Run as if started in this directory
    #[arg(short = 'C', global = true, value_name = "DIR", id = "chdir")]
    chdir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a repository (if needed), index the code and write the map
    Init { path: Option<PathBuf> },
    /// Working tree status
    Status {
        /// git-style two-letter codes instead of words
        #[arg(short, long)]
        short: bool,
    },
    /// Stage paths or single functions/types (default: everything)
    Add { paths: Vec<String> },
    /// Unstage paths or single functions/types
    Reset { paths: Vec<String> },
    /// Commit staged changes; refreshes the map and appends a semantic change list
    Commit {
        /// Message (default: the one suggested from your changes; asks first in a terminal)
        #[arg(short, long)]
        message: Option<String>,
        /// Stage all changes first
        #[arg(short, long)]
        all: bool,
        /// Don't regenerate/stage the .gitnext map
        #[arg(long)]
        no_map: bool,
        /// Don't append the Semantic-Changes trailer
        #[arg(long)]
        no_semantic: bool,
    },
    /// Commit history
    Log {
        #[arg(short = 'n', default_value_t = 20)]
        limit: usize,
    },
    /// Symbol-level diff (HEAD → working tree, or → index with --staged)
    Diff {
        #[arg(long)]
        staged: bool,
    },
    /// Update the code index (incremental)
    Index,
    /// Regenerate .gitnext/map/**/DIR.md and AGENTS.md; with DIR, print that directory's map
    Map {
        dir: Option<String>,
        /// Print to stdout instead of writing files
        #[arg(long)]
        print: bool,
    },
    /// Skeleton of a file or directory: signatures + one-line docs, no bodies
    Outline {
        path: Option<String>,
        /// For directories: include private items too
        #[arg(short, long)]
        all: bool,
    },
    /// Exact source of one or more symbols (with doc comments and line numbers)
    Read {
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Everything about a symbol: signature, docs, callers, callees, types, hierarchy
    Show { symbol: String },
    /// Find symbols by name or doc text
    Search {
        query: Vec<String>,
        #[arg(short = 'n', default_value_t = 20)]
        limit: usize,
    },
    /// List symbols in a file or directory (default: whole repo)
    Symbols {
        path: Option<String>,
        /// Filter by kind: fn, method, struct, class, enum, trait, type, const, mod…
        #[arg(short, long)]
        kind: Option<String>,
        /// Only public items
        #[arg(long = "pub")]
        public: bool,
    },
    /// Who calls this function/method (or instantiates this class)
    Callers { symbol: String },
    /// What this function/method calls
    Callees { symbol: String },
    /// What breaks if this symbol changes: reverse dependencies, files, tests
    Impact {
        symbol: String,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
    },
    /// Base classes / implementors / subclasses / methods of a type
    Hierarchy { symbol: String },
    /// What a directory depends on and what depends on it
    Deps { dir: Option<String> },
    /// Pack the code relevant to a task into a token budget (for agents)
    Context {
        query: Vec<String>,
        #[arg(short, long, default_value_t = 6000)]
        tokens: usize,
    },
    /// Edit one symbol without reading its file: replace, insert after, delete
    Edit {
        #[command(subcommand)]
        op: cmds::EditOp,
    },
    /// Rename a symbol at its definition and all unambiguous references
    Rename {
        symbol: String,
        new_name: String,
        /// Only report what would change
        #[arg(long)]
        dry_run: bool,
    },
    #[command(flatten)]
    More(cmds::More),
    /// Run as an MCP server on stdio (for Claude Code, Cursor, …)
    Mcp {
        /// Write/merge .mcp.json in the repository root instead of serving
        #[arg(long)]
        install: bool,
        /// full (every tool) or lite (7 tools, terse, small outputs — for local 3–14B models)
        #[arg(long, default_value = "full")]
        profile: String,
    },
    /// Open the interactive UI
    Tui,
    #[command(external_subcommand)]
    Git(Vec<String>),
}

fn main() {
    reset_sigpipe();
    if let Err(e) = run() {
        eprintln!("{} {e:#}", paint("error:", "31;1"));
        std::process::exit(1);
    }
}

pub(crate) struct Out {
    pub json: bool,
    pub pretty: bool,
    pub opts: Opts,
}

impl Out {
    pub fn json<T: Serialize>(&self, v: &T) -> Result<()> {
        if self.pretty {
            println!("{}", serde_json::to_string_pretty(v)?);
        } else {
            println!("{}", serde_json::to_string(v)?);
        }
        Ok(())
    }
    pub fn text(&self, s: &str) {
        if color_on() {
            print!("{}", colorize(s));
        } else {
            print!("{s}");
        }
    }
}

fn run() -> Result<()> {
    // git habits: `gitnext log -5` = `log -n 5`
    let mut argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("log") {
        if let Some(i) = argv.iter().position(|a| a.len() > 1 && a.starts_with('-') && a[1..].bytes().all(|b| b.is_ascii_digit())) {
            let n = argv[i][1..].to_string();
            argv.splice(i..=i, ["-n".to_string(), n]);
        }
    }
    let cli = Cli::parse_from(argv);
    let cwd = match &cli.chdir {
        Some(d) => d.clone(),
        None => std::env::current_dir()?,
    };
    let out = Out {
        json: cli.json,
        pretty: cli.pretty,
        opts: Opts { verbose: color_on(), tests: cli.tests, all: false, full_doc: cli.full },
    };
    let Some(cmd) = cli.cmd else {
        return gitnext_tui::run(&Workspace::open(&cwd)?);
    };
    match cmd {
        Cmd::Init { path } => {
            let p = path.map(|p| if p.is_absolute() { p } else { cwd.join(p) }).unwrap_or(cwd);
            let r = gitnext_ops::init(&p)?;
            if out.json {
                return out.json(&r);
            }
            println!(
                "{} {} {}",
                paint("✓", "32"),
                if r.created_repo { "initialized repository in" } else { "gitnext enabled in" },
                r.root.display()
            );
            let g = Workspace::open(&r.root)?.graph()?;
            println!("  indexed {} · map: .gitnext/map + AGENTS.md", g.counts().line());
            let skip = gitnext_ops::gitnext_index::skipped_dirs(&r.root);
            let mut skipped: Vec<String> = std::fs::read_dir(&r.root)
                .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
                .unwrap_or_default();
            skipped.retain(|n| skip.iter().any(|p| gitnext_ops::gitnext_index::dir_matches(p, n)));
            skipped.sort();
            if !skipped.is_empty() {
                println!("  skipped build output / caches: {} (keep one: `index_keep = name` in .gitnext/config)", skipped.join(", "));
            }
            println!("  next: `gitnext setup` connects your agents · `gitnext doctor` checks everything · `gitnext` opens the UI");
        }
        Cmd::Status { short } => {
            let ws = Workspace::open(&cwd)?;
            let repo = ws.repo()?;
            if short || out.json {
                let st = repo.status()?;
                if out.json {
                    return out.json(&serde_json::json!({ "branch": repo.branch(), "entries": st, "changes": gitnext_ops::changes::changes(repo)? }));
                }
                println!("on {}", paint(&repo.branch(), "1"));
                for e in st {
                    let s = e.staged.map(|c| paint(&c.to_string(), "32")).unwrap_or_else(|| " ".into());
                    let u = e.unstaged.map(|c| paint(&c.to_string(), "31")).unwrap_or_else(|| " ".into());
                    println!(" {s}{u} {}", e.path);
                }
                return Ok(());
            }
            out.text(&gitnext_ops::changes::render_changes(&gitnext_ops::changes::changes(repo)?));
        }
        Cmd::Add { paths } => stage_args(&Workspace::open(&cwd)?, &paths, false)?,
        Cmd::Reset { paths } => {
            if paths.is_empty() {
                bail!("give files or symbols to unstage");
            }
            stage_args(&Workspace::open(&cwd)?, &paths, true)?
        }
        Cmd::Commit { message, all, no_map, no_semantic } => {
            let ws = Workspace::open(&cwd)?;
            let message = match message {
                Some(m) => m,
                None => {
                    let cv = gitnext_ops::changes::changes(ws.repo()?)?;
                    if cv.draft.is_empty() {
                        bail!("nothing to describe — no changes. Give a message with -m");
                    }
                    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                        eprint!("commit with “{}”? [Y]es / [e]dit / [n]o: ", cv.draft);
                        let mut a = String::new();
                        std::io::stdin().read_line(&mut a)?;
                        match a.trim().to_lowercase().as_str() {
                            "" | "y" | "yes" | "д" | "да" => cv.draft,
                            "e" | "edit" => {
                                eprint!("message: ");
                                let mut m = String::new();
                                std::io::stdin().read_line(&mut m)?;
                                if m.trim().is_empty() {
                                    bail!("empty message, nothing committed");
                                }
                                m.trim().to_string()
                            }
                            _ => bail!("nothing committed"),
                        }
                    } else {
                        cv.draft
                    }
                }
            };
            let r = gitnext_ops::commit(&ws, &message, &CommitOpts { all, map: !no_map, semantic: !no_semantic })
                .map_err(|e| anyhow::anyhow!(gitnext_ops::changes::humanize(&e)))?;
            if out.json {
                return out.json(&r);
            }
            println!("{} {} {}", paint("✓", "32"), paint(&r.id[..7], "33"), message.lines().next().unwrap_or(""));
            let n: usize = r.changes.iter().map(|c| c.changes.len()).sum();
            if n > 0 {
                println!("  {n} symbol changes recorded");
            }
            if r.map_updated > 0 {
                println!("  map updated ({} files)", r.map_updated);
            }
            println!("  {}", dim("changed your mind? `gitnext undo` takes the commit back and keeps the changes"));
        }
        Cmd::Log { limit } => {
            let ws = Workspace::open(&cwd)?;
            let log = ws.repo()?.log(limit)?;
            if out.json {
                return out.json(&log);
            }
            for c in log {
                println!("{} {} {}", paint(&c.short, "33"), c.summary, dim(&format!("· {}", c.author)));
                if let Some(t) = c.message.split("Semantic-Changes (gitnext):\n").nth(1) {
                    for l in t.lines().take(8) {
                        println!("  {}", dim(l.trim_start()));
                    }
                }
            }
        }
        Cmd::Diff { staged } => {
            let ws = Workspace::open(&cwd)?;
            let mode = if staged { DiffMode::Staged } else { DiffMode::Worktree };
            let d = gitnext_ops::semantic_diff(ws.repo()?, mode)?;
            if out.json {
                return out.json(&d);
            }
            out.text(&gitnext_ops::render_diff(&d, out.opts.full_doc));
        }
        Cmd::Index => {
            let ws = Workspace::open(&cwd)?;
            let t = std::time::Instant::now();
            let (_, s) = ws.index()?;
            if out.json {
                return out.json(&s);
            }
            println!(
                "{} {} files ({} reparsed, {} removed) · {} symbols · {} edges · {:.0?}",
                paint("✓", "32"), s.files_total, s.files_reparsed, s.files_removed, s.symbols_total, s.edges_total, t.elapsed()
            );
        }
        Cmd::Map { dir, print } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            if print || dir.is_some() {
                let d = norm_path(&ws, dir.as_deref().unwrap_or(""));
                let d = render::dir_for(&g, &d);
                let ctx = gitnext_ops::gitnext_docs::Ctx::new(&ws.root, &g);
                let md = gitnext_ops::gitnext_docs::render_dir(&ctx, &d);
                if out.json {
                    return out.json(&serde_json::json!({ "dir": d, "markdown": md }));
                }
                print!("{md}");
                return Ok(());
            }
            let o = ws.write_map(&g)?;
            if out.json {
                return out.json(&serde_json::json!({
                    "written": o.written, "unchanged": o.unchanged, "removed": o.removed,
                    "source_tokens": o.source_tokens, "map_tokens": o.map_tokens }));
            }
            let k = gitnext_ops::gitnext_docs::fmt_k;
            println!(
                "{} map: {} written, {} unchanged, {} removed → .gitnext/map/, AGENTS.md",
                paint("✓", "32"), o.written.len(), o.unchanged, o.removed.len()
            );
            if o.source_tokens > 0 {
                println!(
                    "  whole map ≈ {} tokens vs ≈ {} tokens of source ({:.0}× smaller)",
                    k(o.map_tokens), k(o.source_tokens), o.source_tokens as f64 / o.map_tokens.max(1) as f64
                );
            }
        }
        Cmd::Outline { path, all } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let p = norm_path(&ws, path.as_deref().unwrap_or(""));
            let mut o = out.opts;
            o.all = all;
            out.text(&render::outline(&g, &ws.root, &p, o));
        }
        Cmd::Read { symbols } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let mut ids = Vec::new();
            for q in &symbols {
                ids.extend(lookup_same(&g, q)?);
            }
            if out.json {
                let v: Vec<_> = ids.iter().map(|&i| serde_json::json!({
                    "symbol": g.syms[i].sym.qualified, "file": g.syms[i].file,
                    "source": render::read(&g, &ws.root, &[i]) })).collect();
                return out.json(&v);
            }
            print!("{}", render::read(&g, &ws.root, &ids));
        }
        Cmd::Show { symbol } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let ids = lookup(&g, &symbol)?;
            if out.json {
                let v: Vec<_> = ids.iter().map(|&i| show_json(&g, i)).collect();
                return out.json(&v);
            }
            for (n, &i) in ids.iter().enumerate() {
                if n > 0 {
                    println!();
                }
                out.text(&render::show(&g, i, out.opts));
            }
        }
        Cmd::Search { query, limit } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let q = query.join(" ");
            if out.json {
                let v: Vec<_> = g.search(&q, limit).into_iter().map(|i| g.sym_ref(i)).collect();
                return out.json(&v);
            }
            out.text(&render::search(&g, &q, limit, out.opts));
        }
        Cmd::Symbols { path, kind, public } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let p = norm_path(&ws, path.as_deref().unwrap_or(""));
            let kind = match kind.as_deref() {
                Some(k) => Some(Kind::parse(k).ok_or_else(|| anyhow::anyhow!("unknown kind `{k}`"))?),
                None => None,
            };
            if out.json {
                let v: Vec<_> = g
                    .syms
                    .iter()
                    .filter(|s| (p.is_empty() || s.file == p || s.file.starts_with(&format!("{p}/"))) && kind.is_none_or(|k| s.sym.kind == k) && (!public || s.sym.is_public()) && (out.opts.tests || !s.sym.test))
                    .collect();
                return out.json(&v);
            }
            out.text(&render::symbols(&g, &p, kind, public, out.opts));
        }
        Cmd::Callers { symbol } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let ids = lookup(&g, &symbol)?;
            if out.json {
                let v: Vec<_> = ids
                    .iter()
                    .map(|&i| serde_json::json!({ "symbol": g.sym_ref(i),
                        "callers": g.callers(i).into_iter().filter(|(c, _)| out.opts.tests || !g.syms[*c].sym.test).map(|(c, l)| (g.sym_ref(c), l)).collect::<Vec<_>>() }))
                    .collect();
                return out.json(&v);
            }
            for &i in &ids {
                out.text(&render::callers(&g, i, out.opts));
            }
        }
        Cmd::Callees { symbol } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let ids = lookup(&g, &symbol)?;
            if out.json {
                let v: Vec<_> = ids.iter().map(|&i| callees_json(&g, i)).collect();
                return out.json(&v);
            }
            for &i in &ids {
                out.text(&render::callees(&g, i, out.opts));
            }
        }
        Cmd::Impact { symbol, depth } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let ids = lookup(&g, &symbol)?;
            if out.json {
                let v: Vec<_> = ids.iter().map(|&i| serde_json::json!({ "symbol": g.sym_ref(i), "impact": g.impact(i, depth) })).collect();
                return out.json(&v);
            }
            for &i in &ids {
                out.text(&render::impact(&g, i, depth, out.opts));
            }
        }
        Cmd::Hierarchy { symbol } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let ids: Vec<usize> = lookup(&g, &symbol)?.into_iter().filter(|&i| g.syms[i].sym.kind.is_type()).collect();
            if ids.is_empty() {
                bail!("`{symbol}` is not a type, class or trait");
            }
            if out.json {
                let v: Vec<_> = ids.iter().map(|&i| g.hierarchy(i)).collect();
                return out.json(&v);
            }
            for &i in &ids {
                out.text(&render::hierarchy(&g, i, out.opts));
            }
        }
        Cmd::Deps { dir } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let d = render::dir_for(&g, &norm_path(&ws, dir.as_deref().unwrap_or("")));
            if out.json {
                return out.json(&g.dir_deps(&d));
            }
            out.text(&render::deps(&g, &d));
        }
        Cmd::Context { query, tokens } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let q = query.join(" ");
            if q.trim().is_empty() {
                bail!("describe the task, e.g. `gitnext context \"fix lexer error handling\"`");
            }
            let pack = gitnext_ops::context(&ws, &g, &q, tokens);
            if out.json {
                return out.json(&pack);
            }
            print!("{}", gitnext_ops::render_context_md(&pack));
        }
        Cmd::Mcp { install, profile } => {
            let ws = Workspace::open(&cwd)?;
            if install {
                let p = mcp::install(&ws.root)?;
                println!("{} wrote {}", paint("✓", "32"), p.display());
                println!("  Claude Code picks it up when started in this folder (approve the server once).");
                println!("  Cursor: copy the same block into .cursor/mcp.json · any client: command `gitnext mcp`, cwd = repo root");
                return Ok(());
            }
            let p = gitnext_ops::agents::Profile::parse(&profile).ok_or_else(|| anyhow::anyhow!("unknown profile `{profile}` (full, lite)"))?;
            mcp::serve(ws, p)?;
        }
        Cmd::Edit { op } => {
            let ws = Workspace::open(&cwd)?;
            cmds::edit(&ws, op, out.json)?;
        }
        Cmd::Rename { symbol, new_name, dry_run } => {
            let ws = Workspace::open(&cwd)?;
            let g = ws.graph()?;
            let i = one(&g, &symbol)?;
            let r = gitnext_ops::edit::rename(&ws.root, &g, i, &new_name, dry_run)?;
            if out.json {
                return out.json(&r);
            }
            out.text(&gitnext_ops::edit::render_rename(&r));
        }
        Cmd::More(m) => cmds::more(&cwd, m, &out)?,
        Cmd::Tui => gitnext_tui::run(&Workspace::open(&cwd)?)?,
        Cmd::Git(args) => {
            let word = args.first().cloned().unwrap_or_default();
            if !is_git_command(&cwd, &word) {
                let ours: Vec<String> = <Cli as clap::CommandFactory>::command().get_subcommands().map(|c| c.get_name().to_string()).collect();
                let mut near: Vec<(usize, &String)> = ours.iter().map(|c| (edit_distance(&word, c), c)).filter(|(d, c)| *d <= 2 || c.starts_with(&word)).collect();
                near.sort();
                let hint = match near.first() {
                    Some((_, c)) => format!(" Did you mean `gitnext {c}`?"),
                    None => " `gitnext --help` lists the commands; anything git knows is passed to git.".to_string(),
                };
                bail!("`{word}` is neither a gitnext nor a git command.{hint}");
            }
            let status = std::process::Command::new("git").current_dir(&cwd).args(&args).status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

/// Git commands we pass through: the built-in porcelain plus the user's aliases
/// and `git-<name>` helpers on PATH.
fn is_git_command(cwd: &std::path::Path, w: &str) -> bool {
    const GIT: &[&str] = &[
        "add", "am", "annotate", "apply", "archive", "bisect", "blame", "branch", "bundle", "cat-file", "check-ignore", "checkout", "cherry", "cherry-pick",
        "citool", "clean", "clone", "commit", "config", "count-objects", "describe", "diff", "difftool", "fetch", "format-patch", "fsck", "gc", "grep", "gui",
        "help", "init", "instaweb", "log", "ls-files", "ls-remote", "ls-tree", "maintenance", "merge", "merge-base", "mergetool", "mv", "notes", "pull", "push",
        "range-diff", "rebase", "reflog", "remote", "repack", "replace", "request-pull", "reset", "restore", "rev-list", "rev-parse", "revert", "rm", "shortlog",
        "show", "show-branch", "sparse-checkout", "stash", "status", "submodule", "switch", "tag", "worktree", "whatchanged", "lfs", "filter-branch", "var",
        "version", "--version", "--help", "-h", "-c", "-C",
    ];
    if GIT.contains(&w) || w.starts_with('-') {
        return true;
    }
    let alias = std::process::Command::new("git").current_dir(cwd).args(["config", "--get", &format!("alias.{w}")]).output();
    if alias.is_ok_and(|o| o.status.success()) {
        return true;
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(format!("git-{w}")).is_file()))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Resolve a symbol query; several *different* symbols → error listing candidates.
/// Overloads (same qualified name) are all returned.
/// Exactly one symbol (overloads must be pinned with `Name@LINE`).
/// `add` / `reset` arguments: paths as usual; a name that is not a path but a
/// changed function / type stages (or unstages) just that symbol's lines.
fn stage_args(ws: &Workspace, args: &[String], unstage: bool) -> Result<()> {
    use gitnext_ops::changes::{self as ch, GroupKind};
    let repo = ws.repo()?;
    let (paths, syms): (Vec<String>, Vec<String>) = args.iter().cloned().partition(|a| {
        a == "." || a.contains('*') || ws.root.join(a).exists() || std::path::Path::new(a).exists() || a.contains('/')
    });
    if !paths.is_empty() || args.is_empty() {
        if unstage { repo.unstage(&paths)? } else { repo.add(&paths)? }
    }
    for q in syms {
        let cv = ch::changes(repo)?;
        let kind = if unstage { GroupKind::Staged } else { GroupKind::Changed };
        let hits = ch::find_change(&cv, kind, &q);
        match hits.len() {
            0 => bail!(
                "`{q}` is not a file, and no {} function or type is called that (see `gitnext status`)",
                if unstage { "staged" } else { "changed" }
            ),
            1 => {
                let (path, c) = &hits[0];
                ch::stage_symbol(repo, path, c, unstage).map_err(|e| anyhow::anyhow!(ch::humanize(&e)))?;
                println!("{} {} {} ({path})", paint("✓", "32"), if unstage { "unstaged" } else { "staged" }, ch::short(c));
            }
            _ => bail!("`{q}` matches several changed symbols: {}; use the full name", hits.iter().map(|(_, c)| c.qualified.as_str()).collect::<Vec<_>>().join(", ")),
        }
    }
    Ok(())
}

pub(crate) fn one(g: &Graph, q: &str) -> Result<usize> {
    let ids = lookup_many(g, q)?;
    if ids.len() > 1 {
        let mut msg = render::candidates(g, q, &ids);
        msg.push_str("pin one with Name@LINE, e.g. ");
        msg.push_str(&format!("{}@{}", g.short_name(ids[0]), g.syms[ids[0]].sym.start_line));
        bail!("{msg}");
    }
    Ok(ids[0])
}

pub(crate) fn lookup(g: &Graph, q: &str) -> Result<Vec<usize>> {
    let ids = lookup_many(g, q)?;
    let first = &g.syms[ids[0]].sym.qualified;
    if ids.iter().any(|&i| &g.syms[i].sym.qualified != first) && ids.len() > 3 {
        bail!("{}", render::candidates(g, q, &ids).trim_end());
    }
    Ok(ids)
}

/// Like [`lookup_many`], but several *different* symbols (three `main`s) is an
/// error listing them; overloads / declaration+definition of one symbol are fine.
pub(crate) fn lookup_same(g: &Graph, q: &str) -> Result<Vec<usize>> {
    let ids = lookup_many(g, q)?;
    let first = &g.syms[ids[0]].sym.qualified;
    if ids.iter().any(|&i| &g.syms[i].sym.qualified != first) {
        let mut msg = render::candidates(g, q, &ids);
        msg.push_str("name one of them, e.g. ");
        msg.push_str(g.short_name(ids[0]));
        bail!("{msg}");
    }
    Ok(ids)
}

pub(crate) fn lookup_many(g: &Graph, q: &str) -> Result<Vec<usize>> {
    let ids = g.find(q);
    if ids.is_empty() {
        let hints: Vec<String> = g.search(q, 5).into_iter().map(|i| g.syms[i].sym.qualified.clone()).collect();
        if hints.is_empty() {
            bail!("symbol `{q}` not found");
        }
        bail!("symbol `{q}` not found. Did you mean:\n  {}", hints.join("\n  "));
    }
    Ok(ids)
}

/// Accept paths relative to cwd or to the repo root; return repo-relative slash path.
pub(crate) fn norm_path(ws: &Workspace, p: &str) -> String {
    let p = p.trim_end_matches('/');
    if p.is_empty() || p == "." {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = cwd.canonicalize().unwrap_or(cwd).strip_prefix(&ws.root) {
                return gitnext_ops::gitnext_index::to_slash(rel);
            }
        }
        return String::new();
    }
    if let Ok(abs) = std::path::Path::new(p).canonicalize() {
        if let Ok(rel) = abs.strip_prefix(&ws.root) {
            return gitnext_ops::gitnext_index::to_slash(rel);
        }
    }
    p.trim_start_matches("./").to_string()
}

fn show_json(g: &Graph, i: usize) -> serde_json::Value {
    let mut v = serde_json::json!({
        "file": g.syms[i].file,
        "symbol": g.syms[i].sym,
        "callers": g.callers(i).into_iter().map(|(c, l)| (g.syms[c].sym.qualified.clone(), l)).collect::<Vec<_>>(),
        "uses": g.uses(i).into_iter().map(|c| g.syms[c].sym.qualified.clone()).collect::<Vec<_>>(),
    });
    if g.syms[i].sym.kind.is_callable() {
        v["calls"] = callees_json(g, i);
    }
    if g.syms[i].sym.kind.is_type() {
        v["hierarchy"] = serde_json::to_value(g.hierarchy(i)).unwrap_or_default();
    }
    v
}

fn callees_json(g: &Graph, i: usize) -> serde_json::Value {
    let c = g.callees(i);
    serde_json::json!({
        "resolved": c.resolved.into_iter().map(|(x, l)| (g.syms[x].sym.qualified.clone(), l)).collect::<Vec<_>>(),
        "ambiguous": c.ambiguous.into_iter().map(|(n, v)| (n, v.into_iter().map(|x| g.syms[x].sym.qualified.clone()).collect::<Vec<_>>())).collect::<Vec<_>>(),
        "external": c.external,
    })
}

/// Light terminal coloring of the compact text format.
fn colorize(s: &str) -> String {
    const LABELS: &[&str] = &[
        "callers", "calls", "external", "ambiguous", "uses", "extends", "implements", "implemented by",
        "extended by", "methods", "doc", "in", "out", "files", "tests", "depends on", "used by", "impl",
    ];
    let mut out = String::with_capacity(s.len() + 64);
    for (n, line) in s.lines().enumerate() {
        let mut l = line.to_string();
        if let Some(h) = l.strip_prefix("# ") {
            l = paint(&format!("# {h}"), "1;36");
        } else if n == 0 && !l.starts_with('L') {
            l = paint(&l, "1");
        } else {
            // dim `L123` line markers
            if let Some(rest) = l.trim_start().strip_prefix('L') {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let pad = &l[..l.len() - l.trim_start().len()];
                    l = format!("{pad}{} {}", dim(&format!("L{digits}")), rest[digits.len()..].trim_start());
                }
            }
            for lab in LABELS {
                if let Some(rest) = l.strip_prefix(lab) {
                    if rest.starts_with(':') || rest.starts_with(" (") {
                        l = format!("{}{}", dim(lab), rest);
                        break;
                    }
                }
            }
            match l.chars().next() {
                Some('+') if l.starts_with("+ ") => l = paint(&l, "32"),
                Some('-') if l.starts_with("- ") => l = paint(&l, "31"),
                Some('~') => l = paint(&l, "33"),
                _ => {}
            }
        }
        if let Some(i) = l.find(" — ") {
            l = format!("{}{}", &l[..i], dim(&l[i..]));
        }
        out.push_str(&l);
        out.push('\n');
    }
    out
}

/// Die quietly on a closed pipe (`gitnext symbols | head`) instead of panicking.
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}
#[cfg(not(unix))]
fn reset_sigpipe() {}

fn color_on() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(s: &str, code: &str) -> String {
    if color_on() { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

fn dim(s: &str) -> String {
    paint(s, "2")
}
