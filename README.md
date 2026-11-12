<h1 align="center">gitnext</h1>

<p align="center">
  <b>git that understands your code — for AI agents and the people who review them</b><br>
  <a href="README.ru.md">Русский</a> · <a href="docs/GUIDE.ru.md">Full guide (RU)</a> · <a href="docs/AGENT_INSTALL.md">Install guide for agents</a>
</p>

<p align="center">
  <a href="https://github.com/pnnnx/gitnext/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/pnnnx/gitnext/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <a href="https://zenodo.org/badge/latestdoi/1453025544"><img alt="DOI" src="https://zenodo.org/badge/1453025544.svg"></a>
  <img alt="Rust" src="https://img.shields.io/badge/written%20in-Rust-orange.svg">
</p>

<p align="center"><img src="docs/images/tui-symbol.svg" width="900" alt="gitnext TUI: files, symbols and a function with its inputs, output, callers and code"></p>

gitnext is a drop-in companion to git (`gitnext` = `git` + a code index). It parses Rust, Python, C, C++, TypeScript, JavaScript, Go and Java into functions, types and the calls between them, knows which libraries each function calls (with the versions from `package.json`, `go.mod`, `pom.xml`, `Cargo.toml`, `pyproject.toml`), and puts that graph to work:

- **agents** read one function instead of a whole file, see who calls it and which tests cover it in **one call**, and rename or edit by symbol;
- **people** browse code as a map, see changes as "function X changed" instead of line noise, stage a single function and undo a commit without fear.

The repository stays plain git — GitHub, CI and your other tools keep working.

## Results

Everything here is measured on other people's code — ten open-source projects in five languages — and judged by their own compilers and test suites. How each number is obtained and how much of it is noise: [eval/results/HISTORY.md](eval/results/HISTORY.md).

### A small local model solves more

<p align="center"><img src="docs/images/agent.svg" width="900" alt="Tasks solved by a 4B local model: with plain tools 4 of 24, with gitnext tools 17 of 24"></p>

The same 4B model, the same tasks, the same judge. With plain tools it guesses file paths and answers from memory; with gitnext one call gives it the code, the callers and the tests. For small models there is a short list of five tools (`gitnext mcp --profile lite`): in a separate run of 18 tasks the model solved 17 with the five and 14 with eleven, and spent 15% fewer tokens.

### A failing test leads to the function at fault

<p align="center"><img src="docs/images/why.svg" width="900" alt="Share of seeded bugs where gitnext why names the function at fault first: 18% from the call graph, 81% with the learned coverage map"></p>

`gitnext why <test>` runs the test and answers in a dozen lines: the failure itself, the code the test ran with the most likely culprit first, and the lines that changed since the tests last passed. `gitnext learn-tests` is one test run with coverage (Python, Go, Rust); a long build or test log goes through `gitnext digest` (a vitest run with 35 failures: 843 lines → 12).

### How accurate the code graph is

| | Go | Python | TypeScript | Java | Rust | all |
|---|---:|---:|---:|---:|---:|---:|
| who calls a function — found | 100% | — | 100% | 100% | 99% | **100%** ±1 |
| who calls a function — correct | 98% | — | 100% | 100% | 100% | **99%** ±1 |
| rename — the project still builds and its tests pass | 93% | 90% | 85% | 96% | 87% | **91%** ±3 |
| tests for a function — one of the first 10 named does fail | 84% | 81% | 41% | 95% | 73% | **79%** ±6 |
| … with a learned coverage map — failing tests found | 100% | 100% | — | — | 100% | |

232 functions for callers, 338 renames, 167 functions for tests; ± is the 90% interval, a difference inside it is noise. Python has no compiler to say who calls what, so callers are not scored there. `python3 eval/run.py` repeats the run; the full table is [eval/results/latest.md](eval/results/latest.md).

### Fewer steps, less to read

<p align="center"><img src="docs/images/savings.svg" width="900" alt="Bytes of code an agent read for six tasks: 49 KB without gitnext, 29 KB recounted, 22 KB with gitnext look"></p>

Most of an agent session is the system prompt re-sent on every step, so the win comes from **fewer steps**, not just shorter output: `gitnext look A B C` answers *what is it · who calls it · what it calls · which tests reach it and how to run them · what a change affects* for several targets at once. How to reproduce: [integrations/ab](integrations/ab).

### What is checked, and what is not good yet

- 122 unit tests; parser output is pinned by fixtures in eight languages; every command and every MCP tool — 86 in all — is run on each of the ten projects (860 calls, none fails). What checks each function: [eval/results/COVERAGE.md](eval/results/COVERAGE.md).
- TypeScript: `tests-for` finds 19% of the failing test files (it was 3%); the coverage map exists for Python, Go and Rust only.
- A rename breaks the build or the tests in 9% of cases — mostly dynamic access and names shared by unrelated types.
- A 4B model rarely fixes a seeded bug yet: 3 of 12 on click.
- Rust tests written inside a macro are not seen (22% of the tests of indexmap).

## Install

```sh
git clone https://github.com/pnnnx/gitnext && cd gitnext
sh scripts/install.sh          # builds from source (needs Rust: https://rustup.rs)
# or, once a release is published:
curl -fsSL https://raw.githubusercontent.com/pnnnx/gitnext/main/scripts/install.sh | sh
```

## Quick start

```sh
cd your-project
gitnext init        # index the code, write the map (.gitnext/map, AGENTS.md)
gitnext setup       # connect Claude Code / Cursor / Codex / loca
gitnext doctor      # check that everything is in place
gitnext             # open the TUI
```

## For agents

| Need | Command (CLI) / MCP tool |
|---|---|
| everything about a symbol, several at once | `gitnext look Agent.execute brief config.py` / `look` |
| skeleton of a file or folder | `gitnext outline <path>` / `outline` |
| edit one function without reading the file | `gitnext edit replace <Sym> -f new.py` / `replace_symbol` |
| rename everywhere (imports, tests, `__all__`, `mod.func`) | `gitnext rename <old> <new>` / `rename_symbol` |
| what breaks, which tests to run | `gitnext impact <Sym>` · `gitnext tests-for <Sym>` (the 10 most likely to fail + one command for the rest) |
| which tests really run a function | `gitnext learn-tests` — the tests run once with coverage (Python: pytest-cov; Go; Rust: `llvm-tools-preview`); `tests-for`, `look`, `check` and `why` then use the real list |
| did my change break anything | `gitnext check` — builds and runs the tests of what changed, prints a verdict (`gitnext rename A B --check` does it right after the rename) |
| a test fails — why | `gitnext why <test>` / `check` with `test` — runs it: the failure in a few lines, the code the test ran with the most likely culprit first, the lines changed since the tests passed |
| a long build or test log | `cargo test 2>&1 \| gitnext digest` — what failed, where, the message (a vitest run with 35 failures: 843 lines → 12) |
| error or traceback → code | `gitnext locate` (stdin) / `locate` |
| where were we | `gitnext brief` · `gitnext journal` |
| which libraries the code calls, with versions | `gitnext libs` (and `look` shows them per function) |

`gitnext setup` writes the MCP config and a short instruction block for each agent; `--hooks` makes Claude Code see an outline before reading a big file whole.

**Small local models.** `gitnext mcp --profile lite` shows five tools instead of 29 — `look`, `replace`, `add_after`, `rename`, `check` — with short descriptions and short answers: a 4B model chooses better from a short list. For a model that calls no tools at all, `gitnext context "<task>"` prints the code the task is about, to put into the prompt.

## Git in plain words

<p align="center"><img src="docs/images/tui-changes.svg" width="900" alt="Changes screen: one function staged, a commit message suggested"></p>

```sh
gitnext status            # what changed, by function, with a suggested commit message
gitnext add Task::reopen  # stage just this function
gitnext commit            # offers the message; the symbol list goes into the commit
gitnext undo              # take the last commit back, changes stay staged
gitnext timeline          # every earlier point of the branch; --to N goes back
gitnext branches          # how far ahead/behind each branch is and what it changes
```

In the TUI: `s` changes, `space` stages a file or one function, `enter` shows before/after side by side, `c` commit, `U` undo, `H` time machine, `B` branches, `ctrl-p` every action.

<p align="center"><img src="docs/images/tui-overview.svg" width="900" alt="Project overview: languages, areas, entry points, dependency layers"></p>
<p align="center"><sub>Overview (<code>o</code>): languages and size, areas and their purpose, entry points (main, HTTP routes, CLI), dependency layers — foundation at the bottom, its users on top.</sub></p>

## How it works

<p align="center"><img src="docs/images/architecture.svg" width="900" alt="Repository → parsers → index → code graph → map, CLI, TUI, MCP"></p>

| Folder | What it does |
|---|---|
| `crates/gitnext-parse` | tree-sitter parsers → symbols (kind, signature, inputs, output, docs) and references. Rust, Python, C, C++ have their own extractors; TypeScript, JavaScript, Go and Java go through one table-driven engine (`generic.rs` + a table per language in `specs.rs`) |
| `crates/gitnext-index` | incremental SQLite index, code graph (callers, impact, inheritance, layers), semantic diff |
| `crates/gitnext-core` | git plumbing on libgit2: status, commit, stage by line range, undo, branches, reflog |
| `crates/gitnext-docs` | the map: `.gitnext/map/**/DIR.md` and the block in `AGENTS.md` |
| `crates/gitnext-ops` | features: `look`, edits and rename, changes in words, history, insight, agent setup, explain, LSP |
| `crates/gitnext-cli` | the `gitnext` command and the MCP server |
| `crates/gitnext-tui` | the terminal UI |
| `integrations/` | A/B test kit for real agents, loca (llama.cpp) benchmark |
| `eval/` | the measurements: `run.py` (accuracy on ten projects), `agent.py` (a local model on real tasks), `why_rank.py` (seeded bugs), `smoke.py` (every command on every project) |

## Status

Early, and used daily on its own code (this repository is indexed by gitnext — see `AGENTS.md`). Languages: Rust, Python, Go, Java, TypeScript, JavaScript, C, C++. Name-based resolution is approximate for dynamic code; `gitnext lsp` refines it with rust-analyzer / pyright / clangd. Every change to what the tools answer is measured before it is merged: [eval/results/HISTORY.md](eval/results/HISTORY.md).

## Similar projects

Each piece exists elsewhere; gitnext puts them in one tool and measures the effect.

| Project | What it does | How gitnext differs |
|---|---|---|
| [Aider repo map](https://aider.chat/docs/repomap.html) | tree-sitter map of the repo in the prompt | the map is committed to the repo (`AGENTS.md`, `.gitnext/map`), so any agent — even a small local model without tools — gets it |
| [Serena](https://oraios.github.io/serena/01-about/035_tools.html) | MCP tools to find, edit and rename by symbol, via language servers | one call `look` returns code + callers + callees + tests + impact; works without a language server (LSP is optional refinement) |
| [CodeGraph](https://colbymchenry.github.io/codegraph/), [CodeGraphContext](https://mcpservers.org/servers/shashankss1205/codegraphcontext) | code graph over MCP: callers, callees, impact | also injects the code a task needs before the request (`gitnext context`) for agents that do not call tools |
| [sem](https://docs.rs/sem-cli/0.7.0/sem/) | entity-level diff, blame and impact on top of git | adds plain-language git: stage or commit one function, undo, timeline, commit message from changed functions |
| [lazygit](https://github.com/jesseduffield/lazygit), [GitButler](https://gitbutler.com) | friendly git UIs | the TUI shows code by function and relation, not only files and hunks |

What we can show that others do not: accuracy against compilers and tests on ten projects, and the effect on a real agent — including a 4B local model ([eval](eval), [integrations/ab](integrations/ab), [integrations/loca](integrations/loca)).

## License

MIT — see [LICENSE](LICENSE).
