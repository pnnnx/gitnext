# Eval history

Same repos and commits (eval/repos.json), same sample (seed 7, 8 functions, 4 mutants per repo).
Ground truth comes from each project's compiler and test suite, see `eval/run.py`.
From the last run on, a rename only counts as ok if the project's tests still pass too.

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
