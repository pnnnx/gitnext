# Eval history

Ground truth comes from each project's compiler and test suite, see `eval/run.py`.
Runs 1–5: five repos (one per language), one sample (seed 7, 8 functions, 4 mutants per repo).
From run 6 on: ten repos, 20 functions and 8 mutants per repo, three seeds, and a threshold next to every number.
Since the run after run 4, a rename only counts as ok if the project's tests still pass too.

## The agent runs · a 4B local model with and without gitnext

`eval/agent.py` (`sh eval/run_agent_mac.sh`): Qwen 3.5 4B on llama.cpp, the projects click (Python) and semver (Rust).
Four kinds of task — find where a function is, who calls it, rename it everywhere, fix a seeded bug — judged by the
project's own compiler and tests. `plain`: read, grep, edit, run. `gitnext`: the same plus look, rename, replace, check.
`verify`: gitnext, and an answer given before any tool call or a change that does not pass `gitnext check` goes back
to the model. `lite`: only the five tools of `gitnext mcp --profile lite`.

| run | tasks per mode | plain | gitnext | verify | lite |
|---|---:|---:|---:|---:|---:|
| 1 (8 tasks × 3) | 24 | 3 | 11 | | |
| 2 | 24 | 4 | 14 | 14 | |
| 3 (`why` added) | 24 | 4 | 15 | 17 | |
| 4 (find, callers, rename only) | 18 | | 14 | | 17 |
| 5 (fix only; stopped at 22 of 24) | 11 | | 2 | | 1 |

Run 3 by kind, 6 tasks each (plain → gitnext → verify): find 4 → 6 → 6, who calls 0 → 3 → 6, rename 0 → 6 → 5, fix 0 → 0 → 0.
Run 4: tokens sent to the model, all 18 tasks: 34.0 k with eleven tools, 28.8 k with five.

What the conversations showed, and what was changed after each run:
- without gitnext the model guesses file paths (`src/processing.py`) and looks for `*.py` in a Rust project;
- told "the test `test_greater_than` fails" it asked for `greater_than`, found nothing and invented a function —
  hence `gitnext why <test>` / `check <test>`: run the test, give the failure and the code to look at;
- it writes the corrected function as text instead of calling `replace` — the harness now puts such a function in
  place and checks it (as it already did with tool calls written as text);
- all 5 lost runs of run 4 were answers given before any tool call — every gitnext mode now sends such an answer back;
- run 5, the first `fix` tasks solved (click 3 of 12, with the coverage map; semver 0 of 10, without one): where
  the model called `check <test>` and the map was there, the culprit was first 5 times of 5 and 3 were solved; in
  the other two it did not see the wrong line — hence the changed lines in the answer of `why`.

### A trial on a rented GPU: the same `fix` tasks with a 27B model

The harness on Kaggle (two T4 cards, llama.cpp with CUDA, the settings of the Mac runs), gitnext 0.3.0, the `fix`
tasks of click and semver with the coverage map, three bugs per project. One run, 6 tasks per cell — the counts of
solved tasks are too few to compare; the steps and tokens differ in every single task.

| model | mode | solved | median steps | tokens sent, all 6 | seconds, all 6 |
|---|---|---:|---:|---:|---:|
| Qwen 3.5 27B | plain | 5 of 6 | 16 | 498 k | 1184 |
| | gitnext | 6 of 6 | 5 | 58 k | 378 |
| | lite | 6 of 6 | 7 | 105 k | 676 |
| Qwen 3.5 4B | gitnext | 2 of 6 | 16 | 499 k | 385 |
| | lite | 2 of 6 | 17 | 332 k | 322 |

- The 27B model with gitnext does the same thing every time: `check <test>`, one edit, `check`, done — 4 or 5 steps.
- The 4B model: 37 of its and the 27B model's 89 `look` calls named a place the way a traceback does
  (`src/eval.rs:62`, `tests/test_options.py:460-500`, `eval.rs::matches_greater`) and got "nothing found"; with the
  five tools of `lite` there is no other way to read lines, and three of its four lost `lite` runs ended at the step
  limit on such calls. `look` takes these now. In the three lost `gitnext` runs out of four the model named the bug
  and stopped without an edit. One run ended on a `replace` cut by the token limit — the harness asks again now.

### The full run on a rented GPU: three models, 216 tasks each way

Kaggle, two T4 cards, llama.cpp with CUDA, the settings of the Mac runs; gitnext 0.3.0 plus the `look` change above.
Qwen 3.5 4B, 9B and 27B (Q4_K_M); six projects (click, sqlparse, semver, indexmap, cobra, chi); find, who calls, fix,
rename; three sets of tasks per project (seeds 11, 12, 13) — 72 tasks, the same for every model and mode, 648 runs.
Every row: `eval/results/agent-kaggle-20261113.json`; the tables below: `python3 eval/agent_stats.py` on it.

Solved, of 72 (median steps · tokens sent per task · seconds per task):

| model | plain | gitnext (11 tools) | lite (5 tools) |
|---|---|---|---|
| Qwen 3.5 4B | 46 (10 · 54.7 k · 37 s) | 47 (5 · 44.7 k · 30 s) | 52 (3 · 41.1 k · 28 s) |
| Qwen 3.5 9B | 44 (9 · 53.9 k · 51 s) | 50 (5 · 38.0 k · 44 s) | 51 (4 · 36.6 k · 50 s) |
| Qwen 3.5 27B | 54 (10 · 49.6 k · 123 s) | 59 (4 · 18.8 k · 55 s) | 60 (3 · 20.9 k · 65 s) |
| all three, of 216 | 144 | 156 | 163 |

By kind, of 18 (plain → gitnext → lite):

| model | find | who calls | fix | rename |
|---|---|---|---|---|
| 4B | 17 → 18 → 18 | 12 → 15 → 16 | 3 → 2 → 6 | 14 → 12 → 12 |
| 9B | 18 → 18 → 18 | 10 → 14 → 15 | 4 → 5 → 6 | 12 → 13 → 12 |
| 27B | 18 → 18 → 18 | 15 → 14 → 16 | 7 → 14 → 14 | 14 → 13 → 12 |

What is and is not shown (the same task in two modes is a pair; sign test on the pairs that differ; the interval is a
bootstrap over the 72 tasks):
- **One model, all kinds: inside the noise.** Plain → lite: 4B +12 −6 (p = 0.24), 9B +10 −3 (p = 0.09), 27B +10 −4
  (p = 0.18). Plain → gitnext: +7 −6, +10 −4, +8 −3.
- **Three models together:** plain → lite 144 → 163 of 216, +32 −13, +8.8 points, 95% interval +0.9 to +16.7.
  Plain → gitnext 144 → 156, +25 −13, +5.6 points, interval −1.4 to +13.0.
- **Fixing a bug with the 27B model:** 7 → 14 of 18 in both gitnext modes, +7 −0 (p = 0.016). `check <test>` was its
  first call in all 36 runs. The small models fix 2–6 of 18 whatever the tools.
- **Steps:** a median of 9–10 plain, 4–5 with gitnext, 3–4 with the five tools — in every model.
- **The earlier Mac result did not repeat.** Runs 1–3 above gave the 4B model 3–4 of 24 plain and 11–15 with gitnext
  on click and semver. Here, on the same two projects, plain is 19 of 24 and gitnext 18. Since run 3 the harness
  itself changed (the guard's hints act in every mode: 29 of 72 plain runs got one) and llama.cpp is another build;
  which of the two moved the plain numbers was not measured. The numbers of runs 1–3 should not be quoted any more.

What the conversations showed:
- With eleven tools the small models do not start a fix from `check <test>`: the 9B model never in 18 tasks, the 4B
  in 5 (they call `look`). With five tools: 16 of 18 each. The tool description now starts with the failing test.
- `rename` with the tool lost to a rename by hand on the judge (old name nowhere outside `*.md`): the same five tasks
  in every model. Two are a name in the `.rst` documentation (sqlparse) — `rename --text` covers documentation now;
  the others are names that are also common words (`Name`, `remaining`) or a standard-library function (`FileServer`),
  where the judge's `git grep` finds other things.
- `look` after the change, the 553 calls of the lite fix runs: 18 named something that is not there (a path outside
  the repo, a shell command, a library function), 5 a name that is not in the file given, 62 repeated an earlier call.
- Rust `why` with the map after macros became symbols: the function at fault first in 17 of 20 (it was 14).

## Run 8 · in TypeScript the test is the file; coverage maps for Go and Rust

**tests-for, TypeScript.** `it('…', () => …)` is not a named function; its calls are booked to the module of the
test file, and `tests-for` dropped everything that is not a function. A test file's module now counts as a test.
Paired comparison with run 7 on the same functions (`--compare`): only yaml and zustand moved, every metric up.

| yaml (21 functions) | run 7 | run 8 | difference, 90% interval |
|---|---:|---:|---|
| tests recall | 1.9% | 18.0% | +16.1 (+7.8 … +25.1) |
| tests precision | 15.9% | 30.8% | +14.9 (+5.5 … +25.9) |
| caught by the top 10 | 23.8% | 38.1% | +14.3 (+4.8 … +28.6) |

All languages: tests recall 47% → 49% ±6, caught by the top 10: 77% → 79% ±6; TypeScript 3% → 19% ±10 and 27% → 41% ±19.
Why it stops there (`learn-tests-yaml.causes.md`): paths deeper than 4 calls, "maybe" edges, exports under another
name. A coverage map per test file finds 100% on both TypeScript projects (`learn_tests_js.py`) for 2.3–6.2 plain test runs.

**learn-tests, Go and Rust.** The prototypes are in the product (`learn/go.rs`, `learn/rust.rs`): the map of cobra is
236 functions / 13 106 pairs and of semver 70 / 782 — the prototypes' numbers; chi 166 / 3 992 (164 / 3 826 plus two
of its four `init` functions, which now have entries of their own), indexmap 358 / 3 837 (43 of its 192 tests are
written inside a macro and are not symbols of the graph).

**why with the map, Go** (`why-rank-go.md`, 17 seeded bugs in cobra and chi): the culprit is first in 35% from the
call graph and in 82% with the map, and among the 8 printed in 100% (52% from the graph).

**why with the map, Rust** (`why-rank-rust.md`, 20 seeded bugs): the culprit is first in 15% from the call graph and
in 70% with the map; semver 10 of 10 (2 of 10 from the graph), indexmap 4 of 10 — its other 6 fail only in tests made
by a macro. Two things found on the way: the first line of a function was not counted for Rust (one-line functions
lost), and `impl Display` / `impl Debug` of one type share a qualified name — the map compared the code of one with
the hash of the other and marked an untouched function "changed".

## `why` · where the function at fault is in the list (37 seeded bugs)

`eval/why_rank.py`: one operator is changed inside a function (as `eval/agent.py` does for its `fix` tasks), a test
that passed starts failing, and `gitnext why <that test> --json` is asked. Known answer: the changed function.
Table: `why-rank.md`. `GITNEXT=… python3 eval/why_rank.py --repos click,sqlparse,semver,cobra --bugs 10 --learn --variants static,nochange,default`.

| what `why` ranks | projects | bugs | first | in the first 3 | in the first 8 (what is printed) | in the list |
|---|---|---:|---:|---:|---:|---:|
| the call graph | all four | 37 | 4 (10%) | 6 (16%) | 11 (29%) | 22 |
| the call graph | click, sqlparse | 20 | 1 (5%) | 2 (10%) | 4 (20%) | 9 |
| learned coverage, rarest first | click, sqlparse | 20 | 4 (20%) | 10 (50%) | 16 (80%) | 20 |
| learned coverage + "changed since `learn-tests`" | click, sqlparse | 20 | 18 (90%) | 19 (95%) | 20 (100%) | 20 |

- The call graph alone is not enough to name the culprit: almost every test of a project reaches the same code, so
  counting the tests that reach a function ("rare first") moved nothing (3 → 3 → 2 first of 40 in the three orders
  tried), and in 15 of 37 the culprit is not reachable through the graph at all (a CLI runner, a filter stack, `format!`).
- Coverage of the failing test contains the culprit every time (20 of 20) and the list is short enough for 8 lines in 80%.
- "Changed since the map was learned" is what puts it first. It is fair for the common case — the tests passed when
  the map was learned and something was changed since — and says nothing about a bug that was always there:
  that case is the "rarest first" row.
- Go and Rust have no map yet (`learn-tests` knows Python): cobra 1 of 7 first, semver 2 of 10.
- 3 of the 10 cobra bugs had only a test that fails on the untouched code as well (it expects a permission error and
  the eval runs as root): `seed_bug` now leaves such tests out.

## Run 7 · the renames that broke the code

Run 6 showed that 58 of 337 renames left a project that does not build or pass its tests. This run fixes the
causes that are gitnext's own; same functions as run 6 (click picked one more), `a → b` is run 6 → now.

| rename ok (tests pass) | all | go | python | typescript | java | rust |
|---|---:|---:|---:|---:|---:|---:|
| run 6 | 83% ±4 | 90% ±6 | 73% ±8 | 70% ±13 | 96% ±3 | 74% ±12 |
| now | **91% ±3** (338) | 90% ±6 | **90% ±5** | 85% ±10 | 96% ±3 | 87% ±10 |

25 renames fixed, none broken (click 66% → 86%, sqlparse 81% → 90%, yaml 69% → 85%, indexmap 70% → 85%).
The difference is real for all repos and for Python (larger than the threshold); TypeScript and Rust moved by
about their threshold. Python tests-for moved with it: tests recall 30% → 36% ±10, caught by the top 10
74% → 81% (click: recall 21% → 31%, top-10 that fail 40% → 53%) — inside the threshold for the language.

What was wrong:
- **Python, calls inside nested functions were not seen at all** (decorators, closures: `def decorator(f): def
  new_func(): ctx.find_object(…)`). They now belong to the function they are written in.
- **Python, a call through the package** — `click.style(…)` in tests and examples, where `click/__init__.py`
  re-exports `style` from `click.termui` — was not resolved. Also `click._compat.f(…)` (a dotted module path),
  `@ctx.call_on_close` (a decorator through a receiver), a bare name used as a value
  (`f = hidden_prompt_func if hide else visible_prompt_func`), and a loop variable that has the name of an
  imported function (`for name, command in …: command.get_short_help_str()`) read as that function.
- **Rust, a name inside a `use` group** (`use crate::util::{third, slice_eq};`) kept the old name.
- **TypeScript, a `#private` member** was refused with the `#` and became public without it.

What is left (31 of 338), by cause: a method whose name is unique in the repo takes the calls of a library
method through a receiver of unknown type and is renamed there (`t.Execute`, `ctx.Value`, `resp.Body.Close`,
`re.Match.start`, `number[].unshift`, `Stream.findFirst`) — 8; a method that implements a library interface
cannot be renamed at all (`error.Error`, `Map.Entry.getValue`, `Iterator.hasNext`) — 3; a use through a receiver
whose type gitnext does not know, inside a macro or as a value in an object literal is left behind
(`bar.update(1)`, `map.contains_key(k)` in `assert!`, `resolve: resolvePairs`) — 7; `--text` renames a string
that is data (`'warn'` in a union type) — 2; TypeScript `YAML.parseDocument(…)` through a namespace import — 2;
tests fail and the cause is not looked at yet (cobra 3, click 2, sqlparse 4) — 9.

**After that, measured in pairs** (`--compare`: both versions on the same functions, the difference taken
per function). Two guards against wrongly "sure" callers were tried and thrown out — "several types of that name →
maybe" (shared names recall −1.5, worse) and "unknown receiver → maybe even when the name is unique" (callers recall
−2.9, picked blind −8.9, worse). What stays: `f().m()` where `f` returns a library type is "maybe"
(`c.PersistentFlags().HasFlags()`, `r.Context().Value(…)`): callers precision 98.6% → 99.4% (+0.8, interval
+0.2 … +1.6), Go 96.3% → 98.2% (+2.0, +0.2 … +4.2), nothing else moves. `rename` leaves such a call alone where it
is known — `f` is a repo function with a library result, or a Go package function (`template.New(…).Execute`):
cobra `Execute` and `HasFlags` fixed, rename ok 90.8% → 91.4% (Go 90% → 93%). A wider rule also skipped
`assertInstanceOf(TextNode.class, n).getWholeText()` and broke one jsoup rename — a library call can hand a repo
object on. 29 of 338 renames still fail.

## Run 6 · a sample large enough to tell a change from noise

The measurement changed, so these numbers are a new starting point. `a → b` below: the code of run 5
(`run6-before.json`) → gitnext with the Rust receiver types, `callers` for several names and the tests-for
changes that followed it (`run6-seeds.json`), both measured the new way on the same functions.

- **A second repo per language**: chi (Go), sqlparse (Python), yaml (TypeScript), json-java (Java), indexmap (Rust).
- **20 functions and 8 mutants per repo, seeds 7, 8, 9.** A function that several seeds picked is measured once.
  Checked now (run 5 in brackets): rename 337 functions (30), callers 232 (22), shared names 108 (10), mutants 166 (16).
- **A threshold for every number**, written as `77% ±6 (20)`. The functions are drawn again 2000 times (bootstrap);
  the ± is half the 90% interval of the mean, and never less than what one function weighs (100 / functions).
  A change smaller than the ± is noise. `--check` uses the same thresholds per repo instead of one 5-point tolerance.

| all repos | mean | functions | 90% interval | on seed 7 / 8 / 9 | 2 × SD of the seeds | threshold |
|---|---:|---:|---:|---:|---:|---:|
| callers recall | 99.8% | 232 | 100–100% | 100 / 100 / 100 | 0.2 | ±1 |
| callers precision | 98.6% | 232 | 98–100% | 98 / 100 / 98 | 2.0 | ±1 |
| callers recall, picked blind | 99.2% | 163 | 98–100% | 100 / 98 / 100 | 1.6 | ±1 |
| shared names: recall | 83.2% | 108 | 78–89% | 85 / 83 / 80 | 5.0 | ±6 |
| shared names: recall with maybe | 97.6% | 108 | 96–99% | 99 / 97 / 97 | 1.8 | ±2 |
| shared names: precision | 99.5% | 93 | 99–100% | 100 / 100 / 99 | 0.6 | ±2 |
| rename ok (tests pass) | 82.8% | 337 | 80–86% | 85 / 85 / 80 | 6.2 | ±4 |
| tests recall | 45.4% | 166 | 40–51% | 42 / 51 / 51 | 10.4 | ±6 |
| tests precision | 47.1% | 166 | 42–52% | 44 / 49 / 53 | 9.0 | ±6 |
| top-10 that fail | 50.9% | 166 | 46–56% | 47 / 55 / 56 | 9.8 | ±6 |
| … best possible order | 60.5% | 166 | 55–66% | 55 / 64 / 65 | 11.2 | ±6 |
| caught by the top 10 | 75.3% | 166 | 70–81% | 71 / 76 / 82 | 11.8 | ±6 |
| look / file | 19.3% | 337 | 17–22% | 21 / 19 / 18 | 2.6 | ±3 |

A seed is one sample of 20 per repo, the mean is over all three, so two seeds differ by more than the threshold of
the mean — the two columns agree (2 × SD ≈ threshold × √3). On one seed (all repos, about 55 mutants) the
tests-for numbers move by 9–12 points; in one repo (16–23 mutants) their threshold is 10–18 points.

| per language | all | go | python | typescript | java | rust |
|---|---:|---:|---:|---:|---:|---:|
| callers recall | 100% ±1 (232) | 100% ±2 (71) | — | 100% ±3 (39) | 100% ±2 (85) | 99% ±3 (37) |
| callers precision | 99% ±1 (232) | 96% ±3 (71) | — | 100% ±3 (39) | 99% ±2 (85) | 100% ±3 (37) |
| callers recall, picked blind | 99% ±1 (163) | 100% ±3 (40) | — | 100% ±3 (34) | 98% ±3 (63) | 100% ±4 (26) |
| shared names: recall | 83% ±6 (108) | 73% ±23 (11) | — | 65% ±25 (10) | 90% ±6 (56) | 80% ±11 (31) |
| shared names: recall with maybe | 98% ±2 (108) | 100% ±10 (11) | — | 90% ±10 (10) | 97% ±3 (56) | 100% ±4 (31) |
| shared names: precision | 100% ±2 (93) | 100% ±13 (8) | — | 100% ±15 (7) | 100% ±2 (52) | 99% ±4 (26) |
| rename ok (tests pass) | 83% ±4 (337) | 90% ±6 (81) | 73% ±8 (92) | 70% ±13 (40) | 96% ±3 (86) | 74% ±12 (38) |
| tests recall | 45% ±6 (166) | 79% ±9 (37) | 30% ±10 (42) | 3% ±5 (22) | 49% ±12 (43) | 53% ±14 (22) |
| tests precision | 47% ±6 (166) | 47% ±11 (37) | 49% ±11 (42) | 20% ±12 (22) | 61% ±10 (43) | 43% ±14 (22) |
| top-10 that fail | 51% ±6 (166) | 54% ±11 (37) | 52% ±11 (42) | 20% ±12 (22) | 67% ±9 (43) | 44% ±14 (22) |
| … best possible order | 60% ±6 (166) | 75% ±10 (37) | 58% ±11 (42) | 20% ±12 (22) | 80% ±8 (43) | 44% ±14 (22) |
| caught by the top 10 | 75% ±6 (166) | 84% ±10 (37) | 74% ±12 (42) | 27% ±16 (22) | 95% ±6 (43) | 73% ±14 (22) |
| look / file | 19% ±3 (337) | 14% ±4 (81) | 17% ±4 (92) | 54% ±12 (40) | 14% ±4 (86) | 14% ±6 (38) |

Per repo: `latest.md`; every number with its interval and its value on each seed: `run6-seeds.json`.

**The first use of the thresholds** — what the changes in gitnext since run 5 did, on the same functions:

| | a → b | threshold | |
|---|---:|---:|---|
| shared names: recall, all repos | 71% → 83% | ±7 | real |
| shared names: recall, Rust | 34% → 80% (28 → 31 methods) | ±13 | real |
| shared names: recall with maybe, Rust | 89% → 100% | ±8 | real |
| shared names: recall with maybe, all repos | 95% → 98% | ±3 | at the threshold |
| tests recall, Rust | 49% → 53% | ±14 | noise |
| caught by the top 10, Rust | 68% → 73% | ±16 | noise |
| every other number, all repos | moves by 0–0.6 points | ±1 … ±6 | noise |

What 8 functions per repo did not show:

- **rename ok was 100% everywhere; on 337 functions it is 83% ±4** (cobra 87, chi 94, click 66, sqlparse 81, yaml 69,
  jsoup 96, json-java 97, indexmap 70). Of the 58 renames that fail: 22 do not compile, 28 break tests, 6 gitnext
  refuses (`#private` methods in TypeScript: "not a valid identifier"), 2 leave a test suite that does not start.
  Seen causes: `--text` renames a method of another type with the same name (`template.Execute`, `io.ReadCloser.Close`,
  `string[].unshift`) and string literals (`'warn'`, `'flow'`); a method that implements an interface of a library
  is renamed (`error.Error`, `Map.Entry.getValue`, `Iterator.hasNext`); uses are left behind — through the package
  (`click.format_filename(…)`, `sqlparse.parse(…)`) and in a Rust `use crate::util::{a, b, third}` list (7 of the
  10 failures in indexmap).
- **tests-for in yaml finds 2% ±5 of the failing test files**: the tests are anonymous `test('…', () => …)` callbacks
  in `tests/*.ts`, there is no named test function to reach through the call graph.
- **callers recall does not depend on how the functions are picked.** The samples hold only functions gitnext finds a
  caller for. Picked blind (the first 20 of the same order, whatever gitnext sees), 163 functions have uses the
  compiler names, and gitnext misses all callers of one of them (`Safelist.isSafeTag` in jsoup): 99.2% ±1.

Errors of the measurement, found and fixed on the way (the answers taken the old way are not used):

- **Go: seven cobra tests failed under every mutant.** The tests were run one at a time from a binary named
  `.gnx.test`; cobra reads the command line as its own arguments unless the binary is called `cobra.test`. The binary
  keeps the name `go test` gives it, and tests that fail alone on the untouched commit are part of the baseline.
  cobra on the sample of run 5: tests recall 82% → 96%, precision 17% → 22%, top-10 that fail 30% → 28%,
  caught by the top 10 100% → 50% (the seven tests made every top 10 look like a hit).
- **Python: the list of failing tests was cut and polluted.** `| tail -400` kept the last 400 lines of the pytest
  summary: 4 of 24 click mutants fail more tests than that (372 test names became 122, 365 → 122, 353 → 102,
  288 → 102). The name of a parametrized case was taken from inside its id (`test_x[a::integer]` → `integer`):
  9 of 24 sqlparse mutants had such names in their truth. The sample of run 5 (4 click mutants) was not affected.
- **Python: a test module that no longer imports stopped the run** ("Interrupted: 1 error during collection") and
  read as "no test fails". The suite now continues past it and every test of such a module counts as failing. No
  mutant of this sample hit it, but four click renames that had passed this way now fail (click rename ok 74% → 66%).
- **Python: a test is `file::name` now**, a bare name merged tests of the same name in different files.
- **Rust: code behind a feature was not compiled** (`cargo check --tests`), so real callers in indexmap's `rayon`
  and `serde` modules counted as wrong: callers precision 90% → 100% with `--all-features`. Benches (they need
  nightly there) are left out of what gitnext found.

Cost. The verdict on a rename (build + tests) is kept per distinct edit, like the other answers of the compilers
and test suites, and `gitnext callers` is asked about 40 functions at once, so a second run only asks gitnext
again: all ten repos, three seeds — 3 min 17 s (jsoup 2 min 7 s; with one name per `callers` run it was 5 min 48 s
of 7 min 41 s). With an empty cache the compilers and test suites work for about 240 minutes (jsoup 103, yaml 45,
chi 34, json-java 29). CI runs one seed of 8 functions and 4 mutants (about 55 minutes of that with an empty
cache, under a minute after) and keeps `truth.json` between runs.

**Checked at acceptance.** The same sample run again gives the same numbers; sqlparse, semver and a small sample of
chi asked from scratch (121 answers of the compilers and test suites) give the same answers. One more error of the
measurement came up: `go build ./...` skips directories that start with `_` (chi's `_examples`), so the real callers
there counted as wrong. They are left out now, like Rust benches: callers precision chi 91% → 97%, Go 94% → 96% ±3,
all repos 97.9% → 98.6% ±1 (the tables above hold the new numbers). What is left in Go is gitnext's own error:
a method whose name is unique in the repo takes the calls of a library method of the same name
(`ctx.Value(…)` → `endpoints.Value`, `c.Flags().HasFlags()` → `Command.HasFlags`).

## Run 5 · the ground truth was wrong in three places

Three numbers in the tables below were errors of the measurement, not of gitnext:

- **jsoup callers precision 47%.** Maven recompiles only the files that changed, so after renaming a definition the
  "compiler" never looked at callers in other files. Every "wrong" caller gitnext reported was real. Now every class is
  recompiled, and the uses the compiler names are fixed and compiled again until the build passes (javac and rustc stop
  at the first failing class / crate).
- **jsoup rename ok "with tests", jsoup tests-for "—".** `mvn -o test` never started (the surefire plugin had not been
  downloaded by the setup), and a suite that does not run has no failures. The setup now runs one test, and every check
  fails loudly if the baseline suite did not run.
- **cobra tests precision 1%.** A Go `panic` kills the whole test binary: only the first test that reached the broken
  function was reported. Go tests of a mutant are now run one at a time.

Two measures are new:

- **shared names** — callers of methods whose name several types share (`release`, `isWhitespace`…): only the receiver's
  type tells them apart. Sure callers, and in brackets sure + "maybe".
- **top-10** — of the 10 tests `tests-for` names first, how many really fail (in brackets: the best any order of the
  same list could reach), and **caught**: whether at least one of the ten fails.

What the compilers and test suites said is cached in `eval/.work/truth.json`, so a re-run only re-asks gitnext.

Same sample as before (seed 7, 8 functions, 4 mutants per repo). `a → b`: the code of run 4 (`run5-before.json`) → now,
both against the fixed ground truth.

| | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| callers recall | 100% | — | 100% | 100% | 100% |
| callers precision | 97% | — | 100% | 100% | 100% |
| shared names: recall | 100% | — | — | 74% → 77% | 25% |
| shared names: recall with maybe | 100% | — | — | 96% | 50% |
| shared names: precision | 100% | — | — | 100% | 100% |
| rename ok (tests pass) | 100% | 100% | 100% | 100% | 100% |
| tests recall | 82% | 19% | 33% | 14% → 19% | 61% |
| tests precision | 16% → 17% | 41% → 41% | 100% | 66% → 68% | 51% → 64% |
| top-10 that fail | 22% → 30% | 45% → 48% | 100% | 60% → 62% | 57% → 67% |
| … best possible order | 78% | 62% | 100% | 75% | 67% |
| caught by the top 10 | 100% | 75% | 100% | 75% | 67% |

What changed in gitnext for this run:

- `tests-for` puts the tests most likely to fail first (direct calls, tests named after the function, then by
  distance), names 10 and gives one command for whole test files; helpers in test files are no longer listed as tests.
- A receiver known only as "what that function returns" (`x := getPool(); x.release()`, `getPool().release()`,
  `new Store().load()`, casts) gets the declared result type (Go, Java, TS/JS). In jsoup, of 36912 calls, those
  resolved to exactly one target went from 12173 to 13288 and the ambiguous ones from 12003 to 11390.

The tables below are the earlier runs, measured the old way.

## callers recall

| run | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| 1 · before | 100% | — | 33% | 100% | 85% |
| 2 · receiver types | 100% | — | 50% | 100% | 85% |
| 4 · fields, anon classes, Rust types | 100% | — | 50% | 100% | 85% |
| now · rename checked by tests | 100% | — | 50% | 100% | 93% |

## callers precision

| run | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| 1 · before | 97% | — | 100% | 48% | 100% |
| 2 · receiver types | 97% | — | 100% | 48% | 100% |
| 4 · fields, anon classes, Rust types | 97% | — | 100% | 47% | 100% |
| now · rename checked by tests | 97% | — | 100% | 47% | 100% |

## rename ok

| run | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| 1 · before | 88% | 100% | 100% | 88% | 80% |
| 2 · receiver types | 100% | 100% | 0% | 62% | 80% |
| 4 · fields, anon classes, Rust types | 100% | 100% | 100% | 100% | 80% |
| now · rename checked by tests | 100% | 100% | 100% | 100% | 100% |

## tests recall

| run | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| 1 · before | 17% | 3% | — | — | 1% |
| 2 · receiver types | 83% | 19% | — | — | 1% |
| 4 · fields, anon classes, Rust types | 100% | 19% | — | — | 61% |
| now · rename checked by tests | 100% | 19% | — | — | 61% |

## tests precision

| run | cobra | click | zustand | jsoup | semver |
|---|---:|---:|---:|---:|---:|
| 1 · before | 1% | 25% | — | — | 6% |
| 2 · receiver types | 1% | 41% | — | — | 6% |
| 4 · fields, anon classes, Rust types | 1% | 41% | — | — | 51% |
| now · rename checked by tests | 1% | 41% | — | — | 51% |

## Known gaps

- "Reaches" is a superset of "fails when it breaks": in cobra most tests reach a function through `Execute`, and the
  call graph cannot tell which of them take that branch — the top 10 hold 30% failing tests where the best order of the
  same list would hold 78%. Coverage from one real test run (`learn-tests`) is the way to close this.
- click (Python): tests go through `CliRunner.invoke` and decorators; the graph sees few of those calls (tests recall 19%).
- Rust: methods whose name several types share are found for 25% of callers (50% with "maybe") — field receivers
  (`self.identifier.is_empty()`) and operator traits (`==` → `PartialEq::eq` → `ptr_eq`) are invisible to the graph.
- jsoup: tests recall 19% — breaking a hub function fails 450–1750 tests; the walk (4 hops) reaches a fraction of them.
- The samples are small (zustand: one function; shared names in cobra: one, in semver: two).
