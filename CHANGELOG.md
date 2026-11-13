# Changelog

## 0.3.1 — 2026-11-13

- **`gitnext look` takes a place in a file**, written the way tracebacks, test runners and models write it:
  `src/eval.rs:62` (the function around the line), `tests/test_x.py:120-160` (those lines),
  `src/eval.rs::matches_greater`, `tests/test_x.py::TestA::test_b[case]`, `eval.rs matches_greater`.
  Words of which only some are names list those names instead of "nothing found". In a trial with
  Qwen 3.5 4B and 27B, 37 of 89 `look` calls were written one of these ways and found nothing; all are answered now.
- **`gitnext rename --text` reaches the documentation** (`.md`, `.rst`, `.txt`, `.adoc`): the old name written as
  code there (`name()`, `` `name` ``, `Type.name`, or a name that cannot be a word) is renamed with the rest.
- `look` also takes a list of places (`a.go:497-506, a_test.go:123-128`) and a Go subtest (`TestX/case`); the MCP
  `check` tool says first that a failing test is what it is for.
- eval: the agent runs repeated on three models, 216 tasks each way (`eval/results/HISTORY.md`): +9 points of solved
  tasks with the five tools, a third of the steps; the earlier 24-task comparison did not repeat.
- **`gitnext dead` names only what nothing refers to.** It knew calls only: a function used as a value, a class
  used as a type, a base or a metaclass, an imported name, a Go `init` were "likely dead" — 160 of 225 on ten
  projects, and deleting them broke the build of six. Now the name must be written nowhere outside the symbol.
- **`gitnext edit replace`** keeps every line where it was when the code comes back as it was read (a line indented
  less than the first, tabs under spaces); **`edit delete`** and `read` take the whole doc comment (a Javadoc
  whose lines do not start with `*` was cut in half, and the delete refused).
- **`callees`** lists a method called on an object of unknown type as "maybe", as `callers` does; `look` counts
  such calls instead of naming them.
- **`gitnext learn-tests` for JavaScript and TypeScript** (vitest with v8 coverage, every test file alone):
  `tests-for`, `look` and `check` name the test files that really run the code — 100% of the failing ones on yaml
  and zustand (the call graph: 16% and 68%), for 2.6–6.6 ordinary test runs. Says which package to install when
  `@vitest/coverage-v8` is missing; jest is not supported yet.
- eval: a coverage map for Java measured as a prototype (`eval/learn_tests_java.py`, JaCoCo with a dump per test
  in one Maven run): 94% and 99% of the failing tests on jsoup and json-java (the call graph: 29% and 71%) for
  2.1–3.5 ordinary test runs. Not in `learn-tests` yet.
- **Rust: functions declared inside a macro call are symbols** (`quickcheck! { fn prop(..) -> bool { .. } }`,
  `proptest!`, a local `macro_rules!` that writes `#[test]`). In indexmap 43 of 192 tests were outside the graph and
  the coverage map, now 4; `why` put the function at fault first in 14 of 20 seeded Rust bugs, now in 16–18, and
  no longer answers with an empty list (4 of 10 on indexmap).
- `dead` again: a name built next to reflection (`getattr(self, "_process_" + kind)`) is used; a Rust function run
  by an attribute (`#[bench]`, `#[no_mangle]`, …) is not dead; `protected` is API. On the ten projects what it
  calls likely dead went 225 → 64 → 1, and that one deletes cleanly.
- `eval/consistency.py`: do the commands agree with each other and with the source (`eval/results/consistency.md`).
- eval: a tool call cut by the token limit (a long `replace`) is asked for again instead of ending the run.
- Release: the Intel Mac binary is built again (its runner was retired); `CITATION.cff`.

## 0.3.0 — 2026-11-12

Measured on ten open-source projects in five languages; the numbers and how they are obtained are in
[eval/results/HISTORY.md](eval/results/HISTORY.md).

### New

- **`gitnext why <test>`** (MCP: `why`, or `check` with `test`) — a test fails: run it and get the failure in a
  few lines, the code the test ran with the most likely culprit first, and the lines changed since the tests
  last passed. On 57 seeded bugs the function at fault is first in 81% with a learned coverage map
  (18% from the call graph alone).
- **`gitnext learn-tests`** — one test run with coverage remembers which test runs which function:
  Python (pytest-cov), Go (`go test -cover`, every test alone), Rust (`llvm-tools-preview`, every test alone).
  `tests-for`, `look`, `check` and `why` then name the tests that really run the code: 100% of the failing
  tests found (the call graph: 19% on click, Go 78%, Rust 59%).
- **`gitnext digest`** — a long build or test log in a few lines: what failed, where, the message
  (rustc, cargo test, pytest, go, tsc, vitest, jest, maven; a vitest run with 35 failures: 843 lines → 12).
- **`gitnext check <test>`** answers as `why`; `gitnext check` after a change prints a verdict.
- **`gitnext ops`, `gitnext undo --op N`** — every edit made through gitnext (CLI and MCP) is in a journal
  and can be taken back one by one.
- **Short tool list for small models** — `gitnext mcp --profile lite`: `look`, `replace`, `add_after`,
  `rename`, `check`. A 4B local model solved 17 of 18 tasks with the five tools and 14 with eleven.

### Better

- **rename**: the project still builds and its tests pass in 91% of 338 renames (83% before): `#private`
  names, names in Rust `use` groups, methods of a library's result are left alone.
- **tests-for, TypeScript / JavaScript**: the test is the file (`it('…', () => …)` is not a named function) —
  failing test files found 3% → 19%.
- **Python calls**: calls inside nested functions, `module.func` through `__init__` re-exports, functions
  passed as values and decorators.
- **callers** in Go directories the build skips (`_examples`, `testdata`) are no longer counted as callers.
- `gitnext outline <folder>` says what is in a folder with nothing public instead of printing nothing.
- `why` and `check` accept the id a test runner prints (`tests/test_x.py::TestA::test_b[case]`, `TestRun/sub`).

### Measured

- `eval/run.py`: 10 projects, 3 samples, a noise interval next to every number; `--compare A B` — two versions
  on the same functions.
- `eval/agent.py`: a 4B local model (Qwen 3.5 4B) on real tasks — 4 of 24 solved with plain tools, 17 with gitnext.
- `eval/why_rank.py`: where the function at fault is in the answer of `why`, on seeded bugs.
- `eval/smoke.py`: every command and every MCP tool on every project — 860 calls, none fails.
- What checks each function: [eval/results/COVERAGE.md](eval/results/COVERAGE.md).

### Known gaps

- TypeScript: no coverage map yet; `tests-for` finds 19% of the failing test files.
- A rename breaks the build or the tests in 9% of cases (dynamic access, names shared by unrelated types).
- Rust tests written inside a macro are not seen.
- A 4B model rarely fixes a seeded bug yet (3 of 12 on click).

## 0.2.0, 0.1.0

See the release pages on GitHub.
