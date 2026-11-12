# What checks each gitnext function

Four levels, strongest first. A command on the last level answers on real code but nobody has checked
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
| `why`, `check <test>` | place of the function at fault for a seeded bug (`why_rank.py`, `why-rank.md`) | with the learned map the culprit is first in 81% of 57 seeded bugs (Python 90%, Go 82%, Rust 70%); from the call graph alone in 18% |
| the tools as a small model uses them | `agent.py`, Qwen 3.5 4B: find / callers / rename / fix | see the agent runs in `HISTORY.md` |

## 2. Unit tests (`cargo test --workspace`, 122 tests)

| function | tests |
|---|---|
| parsers: Rust, Python, Go, Java, TS, JS, C, C++ | fixtures with the expected output in eight languages (`gitnext-parse/tests/fixtures`) and unit tests in the parsers |
| call graph: receiver types, results of functions, library results | `gitnext-index/tests` (10) |
| `rename`, `impact` of a rename | `rename_impact.rs`, `rename_more.rs` (11) |
| `ops`, `undo --op` (operation journal) | `oplog.rs` (11) |
| `digest` | `digest.rs` (10, on saved output of real tools) |
| `why` | `why.rs` (11) |
| `tests-for`, the learned map | `tests_for.rs`, `testmap.rs` (8) |
| `learn-tests`: Go, Rust | unit tests of the parsers of coverage output, and one end-to-end test each (skipped without `go` / `llvm-tools`) |
| `check`, `context`, `locate`, `outline` | 3, 4, 2, 1 |
| `look` (a place in a file), `edit`, `dead` | `look.rs` (3), `edit.rs` (3), `dead.rs` (1) |
| semantic diff (`diff`, `since`, `changelog`), `libs`, `setup`, MCP server, TUI | 4, 3, 3, 2, 3 |

## 3. Agreement with each other and with the source (`eval/consistency.py`, `consistency.md`)

No compiler is asked here; what can be checked without one is, on the 10 projects, 30 functions each:

| function | what is checked | result |
|---|---|---|
| `callers` ↔ `callees` ↔ `impact` | B is a caller of A ⇒ A is a callee of B and B is in the impact of A, and the other way round; a call that is a guess for one is a guess for the other | 399 + 369 pairs, no disagreement (before: 5 calls sure in `callees`, "maybe" in `callers`) |
| `read` (and the code part of `look`, `show`) | the code printed is exactly the lines of the file it names | 300 symbols, all exact |
| `symbols`, `outline` | the lines exist, the name is in them, a method lies inside its type | 6642 symbols, all right |
| `search` | a symbol is found by its own name | 296, all found |
| `edit replace`, `edit delete` | a symbol replaced with its own code leaves the file as it was; a delete removes the symbol with its doc comment and nothing else | 200 edits, all right (before: 11 wrong in the two Java projects — a body line indented less than the first, tabs under spaces, a Javadoc without `*`) |
| `hierarchy` | the bases in a class's own signature (Python, Java, TypeScript) are in `extends` / `implements`, and the class is among their subtypes | 195 bases, both ways, all there |
| `dead` | the name is written nowhere else in the code; with everything it calls likely dead deleted, the project compiles and no test starts failing (Python, Go, Rust, TypeScript) | see below |

`dead` before this check named 225 symbols "likely dead" on the 10 projects; 160 of them were named elsewhere in
the code — used as a value, a type, a base, an import, a callback, or a Go `init` — and deleting them broke the
build of at least 6 of the 8 projects where that is tried. Now the name must be written nowhere outside the symbol
itself: 64 are left. indexmap 57 — deleted, it compiles and its tests pass (not tried with every feature and the
benches built). sqlparse 5 — deleted, eight tests fail: the methods are reached by names built at run time
(`getattr(self, "_process_" + …)`). The two Java projects, one each — not tried (the delete run would need Maven
twice per project). The `public but unused` list is not checked.

## 4. Answers on every project, correctness not checked (`eval/smoke.py`, `smoke.md`)

Every command and every MCP tool is called once on each of the 10 projects with a real function, class,
file and folder: 86 commands and tools × 10 = 860 calls, none failed, crashed, came back empty or took 20 s.

Only this level: `deps`, `show` beyond its code, `map`, `overview`, `activity`, `hotspots`, `cycles`,
`entrypoints`, `history`, `brief`, `journal`, `bench`, `branches`, `timeline`, `msg`, `status`, `add`, `reset`,
`commit`, `undo`, `edit insert-after`, and `look` as a whole (its parts are on levels 1 and 3; that the parts are
put together right is not checked).

## Not checked at all

`tui` beyond three unit tests, `lsp` (needs language servers), `explain` (needs a model), `hook`, `init` on an
empty folder, what `doctor` and `setup` write for each agent.

## What would move things up

- `dead`: names built at run time; the delete run for Java and for Rust with every feature; the public list.
- `hierarchy`: against compile errors after a method is removed from the base type (now only against signatures).
- `look` as a whole: its callers, callees and tests must be the ones `callers`, `callees` and `tests-for` give.
