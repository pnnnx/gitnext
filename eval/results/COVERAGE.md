# What checks each gitnext function

Three levels, strongest first. A command on the last level answers on real code but nobody has checked
that the answer is right.

## 1. Measured against the truth (10 projects, 5 languages; the truth is the project's own compiler and tests)

`eval/run.py` — numbers and their noise thresholds in `latest.md`, history in `HISTORY.md`.

| function | what is measured | result |
|---|---|---|
| `callers` (and the callers part of `look`, `impact`) | found / correct, against compile errors after the definition is renamed | 100% ±1 / 99% ±1 (232 functions) |
| … methods that share a name | found, and found counting "maybe" | 83% ±6 / 98% ±2 (108) |
| `rename` | the project still compiles and its tests pass | 91% ±3 (338) |
| `tests-for` (and the tests part of `look`, `check`) | failing tests found; at least one of the first 10 fails | 49% ±6; 79% ±6 (167). TypeScript: 19% |
| `learn-tests` (Python, Go, Rust) | the same with the learned map (`learn_tests.py`, `learn_tests_go.py`, `learn_tests_rust.py`) | 100% of the failing tests in all three (the graph: 19% on click, Go 78%, Rust 59%) |
| `why`, `check <test>` | place of the function at fault for a seeded bug (`why_rank.py`, `why-rank.md`) | first in 10% from the call graph (37 bugs); with the learned map 90% (20 bugs, Python) and 70% (20 bugs, Rust) |
| the tools as a small model uses them | `agent.py`, Qwen 3.5 4B: find / callers / rename / fix | see the agent runs in `HISTORY.md` |

## 2. Unit tests (`cargo test --workspace`, 115 tests)

| function | tests |
|---|---|
| parsers: Rust, Python, Go, Java, TS, JS, C, C++ | 35 fixture files (`gitnext-parse/tests/fixtures`) and unit tests in the parsers |
| call graph: receiver types, results of functions, library results | `gitnext-index/tests` (10) |
| `rename`, `impact` of a rename | `rename_impact.rs`, `rename_more.rs` (11) |
| `ops`, `undo --op` (operation journal) | `oplog.rs` (11) |
| `digest` | `digest.rs` (10, on saved output of real tools) |
| `why` | `why.rs` (11) |
| `tests-for`, the learned map | `tests_for.rs`, `testmap.rs` (8) |
| `learn-tests`: Go, Rust | unit tests of the parsers of coverage output, and one end-to-end test each (skipped without `go` / `llvm-tools`) |
| `check`, `context`, `locate`, `outline` | 3, 4, 2, 1 |
| semantic diff (`diff`, `since`, `changelog`), `libs`, `setup`, MCP server, TUI | 4, 3, 3, 2, 3 |

## 3. Answers on every project, correctness not checked (`eval/smoke.py`, `smoke.md`)

Every command and every MCP tool is called once on each of the 10 projects with a real function, class,
file and folder: 86 commands and tools × 10 = 860 calls, none failed, crashed, came back empty or took 20 s.

Only this level: `callees`, `hierarchy`, `deps`, `search`, `read`, `show`, `symbols`, `map`, `overview`,
`activity`, `hotspots`, `dead`, `cycles`, `entrypoints`, `history`, `brief`, `journal`, `bench`, `branches`,
`timeline`, `msg`, `status`, `add`, `reset`, `commit`, `undo`, `edit replace / insert / delete`, and `look`
as a whole (its parts are on level 1; that the parts are put together right is not checked).

## Not checked at all

`tui` beyond three unit tests, `lsp` (needs language servers), `explain` (needs a model), `hook`, `init` on an
empty folder, what `doctor` and `setup` write for each agent.

## What would move level 3 up

- `callees`, `impact`, `look`: must agree with `callers` — if B is among the callers of A, A is among the callees
  of B and B is in the impact of A. One script over the eval's own truth.
- `dead`: delete what it names and see that the project still compiles and its tests pass.
- `hierarchy`: against compile errors after a method is removed from the base type.
- `search`, `read`, `outline`: exactness against the source (the lines printed are the lines of the symbol).
