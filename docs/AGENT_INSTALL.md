# Installing gitnext — instructions for an AI agent

You were asked to set up gitnext for a repository. Follow these steps in order;
each command is safe to re-run.

1. **Is it installed?** `gitnext --version`. If that fails:
   - from a clone of gitnext: `sh scripts/install.sh` (or `cargo install --locked --path crates/gitnext-cli`);
   - otherwise: `curl -fsSL https://raw.githubusercontent.com/pnnnx/gitnext/main/scripts/install.sh | sh`.
   Building from source needs Rust (https://rustup.rs). Make sure the install folder
   (`~/.cargo/bin` or `~/.local/bin`) is on `PATH`.
2. **Index the project** (from its root): `gitnext init`. It creates `.git` if missing,
   indexes Rust / Python / C / C++ code and writes `.gitnext/map/` and a block in `AGENTS.md`.
3. **Connect the agent:** `gitnext setup <agent>` where agent is `claude`, `cursor`,
   `codex` or `loca` (local models via llama.cpp / Ollama). Without arguments it
   configures the agents it detects. For Claude Code add `--hooks` if the user wants
   whole-file reads of big code files answered with an outline first.
4. **Check:** `gitnext doctor`. Fix every line marked `✗` with the command shown
   under it; lines marked `!` are optional. Exit code 0 means ready.
5. **Restart the agent session** so it starts the MCP server (`gitnext mcp`).
   Claude Code asks once to approve the `gitnext` server from `.mcp.json`.

What gets written (all inside the project, all idempotent):

| Agent | Files |
|---|---|
| any | `.gitnext/` (map, notes, config; `index.db` is git-ignored), block in `AGENTS.md` |
| claude | `.mcp.json`, block in `CLAUDE.md`, with `--hooks`: `.claude/settings.json` |
| cursor | `.cursor/mcp.json`, `.cursor/rules/gitnext.mdc` |
| codex | nothing; prints the `~/.codex/config.toml` block to add |
| loca | `agents_md = compact` in `.gitnext/config` (short map in `AGENTS.md`); with `--tools` also `.loca/config.json` (MCP, lite profile) and a block in `LOCA.md` |

Using it afterwards (MCP tools):
- big models: `look` first — one call gives a symbol's code, callers, callees, tests (with the command) and impact; pass several targets at once. Then edit with `replace_symbol`, `insert_after`, `rename_symbol`; `overview` once in a new repo, `brief` / `changes` when resuming. Every extra step re-sends the whole conversation, so batch;
- small local models (lite profile): `look(what)` for any file, name, error or words; `replace(symbol, code)`; `add_after(symbol, code)`.

Removing it: delete `.gitnext/`, the `gitnext` entries in the MCP config files and
the blocks between `gitnext:agent:start` / `gitnext:agent:end` and
`gitnext:auto:start` / `gitnext:auto:end` markers.
