# Changelog

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
