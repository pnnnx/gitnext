# Eval history

Same repos and commits (eval/repos.json), same sample (seed 7, 8 functions, 4 mutants per repo).
Ground truth comes from each project's compiler and test suite, see `eval/run.py`.
From the last run on, a rename only counts as ok if the project's tests still pass too.

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

- tests precision is low where the graph reaches a function from almost every test (cobra `Execute` paths): "reaches" is a superset of "fails when it breaks".
- click (Python): tests go through `CliRunner.invoke` and decorators; the graph sees few of those calls, the test-file convention fills part of it.
- Rust operator traits (`==` → `PartialEq::eq` → `ptr_eq`) are invisible to the graph.
- jsoup callers precision: calls on objects of unknown type (`x.release()`) still match every method of that name.
