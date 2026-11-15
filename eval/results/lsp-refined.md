| repo | lang | symbols | index s | callers recall | callers precision | callers recall, picked blind | shared names: recall (+maybe) | shared names: precision | rename ok | tests recall | tests precision | top-10 fail (best possible) | caught by top-10 | look / file |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| cobra | go | 288 | 0.0 | 100% ±3 (45) | 99% ±3 | 100% ±4 (31) | 100% ±100 (100% ±100) (1) | 100% ±100 | 91% ±7 (46) | 89% ±10 (21) | 36% ±13 | 44% ±14 (78% ±12) | 81% ±15 | 7% ±3 |
| chi | go | 351 | 0.0 | 100% ±4 (26) | 97% ±5 | 100% ±12 (9) | 100% ±10 (100% ±10) (10) | 100% ±10 | 94% ±6 (35) | 64% ±16 (16) | 60% ±18 | 66% ±17 (67% ±17) | 88% ±13 | 22% ±7 |
| semver | rust | 113 | 0.0 | 100% ±20 (5) | 100% ±20 | 100% ±20 (5) | 100% ±50 (100% ±50) (2) | 100% ±50 | 100% ±20 (5) | 61% ±34 (3) | 64% ±34 | 67% ±34 (67% ±34) | 67% ±34 | 15% ±9 |
| indexmap | rust | 1026 | 0.1 | 99% ±4 (32) | 100% ±4 | 100% ±5 (21) | 95% ±7 (100% ±4) (29) | 100% ±4 | 88% ±10 (33) | 80% ±12 (19) | 58% ±13 | 58% ±13 (58% ±13) | 95% ±8 | 15% ±7 |

`77% ±6 (20)`: the mean over 20 functions; a change smaller than ±6 points is noise. The ± is half the 90% interval
the number stays in when the functions are drawn again (bootstrap), and never less than what one function weighs.

| per language | all | go | rust |
|---|---:|---:|---:|
| callers recall | 100% ±1 (108) | 100% ±2 (71) | 99% ±3 (37) |
| callers precision | 99% ±2 (108) | 98% ±2 (71) | 100% ±3 (37) |
| callers recall, picked blind | 100% ±2 (66) | 100% ±3 (40) | 100% ±4 (26) |
| shared names: recall | 96% ±5 (42) | 100% ±10 (11) | 95% ±6 (31) |
| shared names: recall with maybe | 100% ±3 (42) | 100% ±10 (11) | 100% ±4 (31) |
| shared names: precision | 100% ±3 (41) | 100% ±10 (11) | 100% ±4 (30) |
| rename ok (tests pass) | 92% ±5 (119) | 93% ±5 (81) | 90% ±8 (38) |
| tests recall | 78% ±8 (59) | 78% ±10 (37) | 77% ±12 (22) |
| tests precision | 51% ±9 (59) | 47% ±11 (37) | 59% ±12 (22) |
| top-10 that fail | 56% ±9 (59) | 54% ±11 (37) | 60% ±13 (22) |
| … best possible order | 68% ±8 (59) | 73% ±10 (37) | 60% ±13 (22) |
| caught by the top 10 | 86% ±8 (59) | 84% ±10 (37) | 91% ±10 (22) |
| look / file | 14% ±4 (119) | 14% ±4 (81) | 15% ±6 (38) |

| all repos | mean | functions | 90% interval | on each seed | 2 × SD of the seeds | threshold |
|---|---:|---:|---:|---:|---:|---:|
| callers recall | 99.7% | 108 | 99–100% | 100 / 100 / 100 | 0.2 | ±1 |
| callers precision | 98.8% | 108 | 98–100% | 98 / 100 / 99 | 2.2 | ±2 |
| callers recall, picked blind | 99.9% | 66 | 100–100% | 100 / 100 / 100 | 0.4 | ±2 |
| shared names: recall | 96.4% | 42 | 92–100% | 98 / 96 / 96 | 3.0 | ±5 |
| shared names: recall with maybe | 100.0% | 42 | 100–100% | 100 / 100 / 100 | 0.0 | ±3 |
| shared names: precision | 100.0% | 41 | 100–100% | 100 / 100 / 100 | 0.0 | ±3 |
| rename ok (tests pass) | 91.6% | 119 | 87–96% | 95 / 94 / 86 | 9.8 | ±5 |
| tests recall | 77.9% | 59 | 71–85% | 68 / 79 / 79 | 13.0 | ±8 |
| tests precision | 51.1% | 59 | 43–59% | 53 / 51 / 53 | 2.0 | ±9 |
| top-10 that fail | 55.9% | 59 | 48–64% | 55 / 57 / 58 | 2.8 | ±9 |
| … best possible order | 68.3% | 59 | 61–76% | 66 / 67 / 66 | 1.4 | ±8 |
| caught by the top 10 | 86.4% | 59 | 78–93% | 78 / 82 / 96 | 19.6 | ±8 |
| look / file | 14.0% | 119 | 11–17% | 16 / 16 / 13 | 3.4 | ±4 |

Sample `n20-m8-seeds7,8,9`: 20 functions and 8 mutants per repo and seed, seeds 7, 8, 9. Run time: 8 min 49 s; of the compilers' and test suites' answers 512 came from the cache, 37 were asked now.
