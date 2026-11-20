# Fixing a seeded bug: show, prove, try, combined — the full runs

Two notebooks on two T4 cards, 451 runs. What the model gets when `check <test>` reports a failing test:

- **V1 show** (`--modes lite`): the failure, the code the test ran with the most likely culprit first, and what changed
  since the tests last passed (`learn-tests`), as it was and as it is now.
- **V2 prove** (`prove`): before the model sees it, the harness undoes each change made since then on its own and runs
  the failing test; the one whose undoing makes it pass is named, with the function as it should be.
- **V3 try** (`try`): V1, plus a `try` tool that puts up to three versions of a fix in place and keeps the first that passes.
- **V4 combined** (`combo`): V2's proof, the changes tried in gitnext's rank order and shown without V1's view of the
  same change; no proof → V1's view; `try` with two versions.

Two kinds of task on the same seeded bug: **fix** (the bug is the only change since the tests last passed) and
**regress** (the bug and two meant changes — a comparison written the other way round, in functions the failing test
runs — in one commit; solved = the tests pass and the meant changes stay). The six projects, seed 11, context 16384,
KV cache q8_0, temperature 0.2, 25 steps, 20 minutes, `--learn`; gitnext `main` 9f5a623 with `eval/agent.py` of this
commit, llama.cpp 5676dfb. Each task is run in every variant (pairs); a pair counts where the two outcomes differ.

- `gemma-4-31b/` — notebook `gitnext-kaggle-fix-31b-v2.ipynb`, one server over both cards, 2.6 hours. 28 of 145 runs
  fell on llama-server crashes (the host ran out of memory with the server's prompt cache, `--cache-ram`, at its 8 GB
  default) and are left out below; the notebook's own report counted them as failures.
- `gemma-4-12b/` — notebook `gitnext-kaggle-fix-combo.ipynb`, one server per card, 4.4 hours, no crashes;
  `report.md` is the notebook's own summary.

`summarize.py <folder>` recomputes everything below from the `agent-*.json` files.

## Gemma 4 31B

145 runs; 28 fell on a llama-server crash and are left out.

| task | V1 show | V2 prove | V3 try |
|---|---|---|---|
| fix | 20 of 20 (100%) | 22 of 22 (100%) | 20 of 21 (95%) |
| regress | 17 of 17 (100%) | 19 of 19 (100%) | 18 of 18 (100%) |
| **both** | 37 of 37 (100%) | 41 of 41 (100%) | 38 of 39 (97%) |

| task | pair | tasks | solved | gained / lost | p (sign) | difference, points (95% interval) | input tokens, median ratio (p, Wilcoxon) |
|---|---|---|---|---|---|---|---|
| fix | V1 show → V2 prove | 20 | 20 → 20 | 0 / 0 | 1.000 | +0.0 (+0.0 to +0.0) | ×0.84 (0.044) |
| fix | V1 show → V3 try | 20 | 20 → 19 | 0 / 1 | 1.000 | -5.0 (-15.0 to +0.0) | ×1.12 (0.881) |
| fix | V2 prove → V3 try | 21 | 21 → 20 | 0 / 1 | 1.000 | -4.8 (-14.3 to +0.0) | ×1.30 (0.012) |
| regress | V1 show → V2 prove | 17 | 17 → 17 | 0 / 0 | 1.000 | +0.0 (+0.0 to +0.0) | ×1.08 (1.000) |
| regress | V1 show → V3 try | 17 | 17 → 17 | 0 / 0 | 1.000 | +0.0 (+0.0 to +0.0) | ×0.81 (0.225) |
| regress | V2 prove → V3 try | 18 | 18 → 18 | 0 / 0 | 1.000 | +0.0 (+0.0 to +0.0) | ×0.94 (0.983) |
| **both** | V1 show → V2 prove | 37 | 37 → 37 | 0 / 0 | 1.000 | +0.0 (+0.0 to +0.0) | ×0.94 (0.261) |
| **both** | V1 show → V3 try | 37 | 37 → 36 | 0 / 1 | 1.000 | -2.7 (-8.1 to +0.0) | ×0.82 (0.420) |
| **both** | V2 prove → V3 try | 39 | 39 → 38 | 0 / 1 | 1.000 | -2.6 (-7.7 to +0.0) | ×1.09 (0.094) |

Solved fix · regress by project:

| project | V1 show | V2 prove | V3 try |
|---|---|---|---|
| click | 3/3 · 3/3 | 4/4 · 3/3 | 4/4 · 3/3 |
| semver | 4/4 · 3/3 | 4/4 · 3/3 | 4/4 · 3/3 |
| chi | 2/2 · 2/2 | 2/2 · 2/2 | 2/2 · 2/2 |
| sqlparse | 4/4 · 3/3 | 5/5 · 4/4 | 3/4 · 3/3 |
| indexmap | 4/4 · 4/4 | 4/4 · 4/4 | 4/4 · 4/4 |
| cobra | 3/3 · 2/2 | 3/3 · 3/3 | 3/3 · 3/3 |

## Gemma 4 12B

306 runs; 0 fell on a llama-server crash and are left out.

| task | V1 show | V2 prove | V4 combined |
|---|---|---|---|
| fix | 28 of 53 (53%) | 46 of 53 (87%) | 49 of 53 (92%) |
| regress | 21 of 49 (43%) | 45 of 49 (92%) | 38 of 49 (78%) |
| **both** | 49 of 102 (48%) | 91 of 102 (89%) | 87 of 102 (85%) |

| task | pair | tasks | solved | gained / lost | p (sign) | difference, points (95% interval) | input tokens, median ratio (p, Wilcoxon) |
|---|---|---|---|---|---|---|---|
| fix | V1 show → V2 prove | 53 | 28 → 46 | 21 / 3 | <0.001 | +34.0 (+18.9 to +49.1) | ×1.04 (0.700) |
| fix | V1 show → V4 combined | 53 | 28 → 49 | 23 / 2 | <0.001 | +39.6 (+24.5 to +54.7) | ×0.97 (0.237) |
| fix | V2 prove → V4 combined | 53 | 46 → 49 | 5 / 2 | 0.453 | +5.7 (-3.8 to +15.1) | ×1.00 (0.355) |
| regress | V1 show → V2 prove | 49 | 21 → 45 | 24 / 0 | <0.001 | +49.0 (+34.7 to +63.3) | ×0.83 (0.227) |
| regress | V1 show → V4 combined | 49 | 21 → 38 | 21 / 4 | <0.001 | +34.7 (+18.4 to +51.0) | ×1.14 (0.396) |
| regress | V2 prove → V4 combined | 49 | 45 → 38 | 2 / 9 | 0.065 | -14.3 (-26.5 to -2.0) | ×1.13 (0.013) |
| **both** | V1 show → V2 prove | 102 | 49 → 91 | 45 / 3 | <0.001 | +41.2 (+30.4 to +52.0) | ×0.93 (0.288) |
| **both** | V1 show → V4 combined | 102 | 49 → 87 | 44 / 6 | <0.001 | +37.3 (+25.5 to +49.0) | ×0.98 (0.817) |
| **both** | V2 prove → V4 combined | 102 | 91 → 87 | 7 / 11 | 0.481 | -3.9 (-11.8 to +3.9) | ×1.06 (0.332) |

Solved fix · regress by project:

| project | V1 show | V2 prove | V4 combined |
|---|---|---|---|
| click | 5/8 · 2/7 | 7/8 · 5/7 | 8/8 · 7/7 |
| semver | 2/8 · 1/7 | 8/8 · 7/7 | 8/8 · 7/7 |
| chi | 6/8 · 5/8 | 7/8 · 7/8 | 6/8 · 3/8 |
| sqlparse | 2/10 · 2/9 | 8/10 · 8/9 | 9/10 · 7/9 |
| indexmap | 5/10 · 6/9 | 7/10 · 9/9 | 9/10 · 6/9 |
| cobra | 8/9 · 5/9 | 9/9 · 9/9 | 9/9 · 8/9 |

`try` was called 0 times in 102 V4 runs.
