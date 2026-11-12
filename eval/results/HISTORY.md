# Eval history

Ground truth comes from each project's compiler and test suite, see `eval/run.py`.
Runs 1–5: five repos (one per language), one sample (seed 7, 8 functions, 4 mutants per repo).
From run 6 on: ten repos, 20 functions and 8 mutants per repo, three seeds, and a threshold next to every number.
Since the run after run 4, a rename only counts as ok if the project's tests still pass too.

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
