| repo | lang | symbols | index s | callers recall | callers precision | callers recall, picked blind | shared names: recall (+maybe) | shared names: precision | rename ok | tests recall | tests precision | top-10 fail (best possible) | caught by top-10 | look / file |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| pflag | go | 756 | 0.1 | 98% ±10 (10) | 100% ±10 | 98% ±13 (8) | 89% ±7 (89% ±7) (34) | 97% ±5 | 82% ±19 (11) | 100% ±15 (7) | 64% ±23 | 64% ±22 (80% ±20) | 100% ±15 | 10% ±6 |
| mux | go | 128 | 0.0 | 100% ±20 (5) | 100% ±20 | 100% ±34 (3) | 100% ±10 (100% ±10) (10) | 100% ±10 | 100% ±20 (5) | 90% ±25 (4) | 93% ±25 | 95% ±25 (100% ±25) | 100% ±25 | 5% ±1 |
| byteorder | rust | 198 | 0.0 | — | — | 0% ±25 (4) | 73% ±20 (73% ±20) (5) | 100% ±20 | — | — | — | — | — | — |
| either | rust | 150 | 0.0 | 100% ±50 (2) | 100% ±50 | 100% ±100 (1) | — | — | 100% ±50 (2) | — | — | — | — | 2% ±1 |
| strsim | rust | 48 | 0.0 | 100% ±20 (5) | 100% ±20 | 100% ±20 (5) | — | — | 100% ±8 (13) | 100% ±10 (11) | 75% ±17 | 90% ±14 (90% ±14) | 100% ±10 | 5% ±1 |
| urfave-cli | go | 703 | 0.1 | 96% ±6 (18) | 89% ±10 | 65% ±20 (13) | 100% ±20 (100% ±20) (5) | 100% ±20 | 84% ±14 (19) | 42% ±19 (13) | 42% ±17 | 53% ±17 (58% ±18) | 100% ±8 | 16% ±7 |
| logrus | go | 311 | 0.0 | 100% ±17 (6) | 92% ±17 | 100% ±17 (6) | 94% ±6 (96% ±5) (41) | 100% ±3 | 33% ±34 (6) | 67% ±20 (5) | 64% ±24 | 72% ±28 (72% ±28) | 100% ±20 | 9% ±2 |
| itertools | rust | 1013 | 0.1 | 100% ±3 (39) | 93% ±6 | 96% ±6 (26) | 96% ±8 (96% ±8) (13) | 100% ±8 | 79% ±10 (47) | 64% ±13 (23) | 76% ±11 | 76% ±11 (80% ±10) | 96% ±7 | 21% ±8 |

`77% ±6 (20)`: the mean over 20 functions; a change smaller than ±6 points is noise. The ± is half the 90% interval
the number stays in when the functions are drawn again (bootstrap), and never less than what one function weighs.

| per language | all | go | rust |
|---|---:|---:|---:|
| callers recall | 99% ±2 (85) | 98% ±3 (39) | 100% ±3 (46) |
| callers precision | 94% ±4 (85) | 94% ±6 (39) | 94% ±5 (46) |
| callers recall, picked blind | 85% ±7 (66) | 84% ±11 (30) | 86% ±10 (36) |
| shared names: recall | 93% ±4 (108) | 93% ±4 (90) | 90% ±8 (18) |
| shared names: recall with maybe | 94% ±4 (108) | 94% ±4 (90) | 90% ±8 (18) |
| shared names: precision | 99% ±2 (106) | 99% ±2 (88) | 100% ±6 (18) |
| rename ok (tests pass) | 82% ±6 (103) | 78% ±11 (41) | 84% ±9 (62) |
| tests recall | 72% ±8 (63) | 67% ±12 (29) | 76% ±10 (34) |
| tests precision | 68% ±8 (63) | 58% ±12 (29) | 76% ±9 (34) |
| top-10 that fail | 73% ±7 (63) | 65% ±12 (29) | 81% ±9 (34) |
| … best possible order | 78% ±7 (63) | 72% ±11 (29) | 83% ±8 (34) |
| caught by the top 10 | 98% ±3 (63) | 100% ±4 (29) | 97% ±5 (34) |
| look / file | 15% ±4 (103) | 12% ±4 (41) | 17% ±6 (62) |

| all repos | mean | functions | 90% interval | on each seed | 2 × SD of the seeds | threshold |
|---|---:|---:|---:|---:|---:|---:|
| callers recall | 98.9% | 85 | 98–100% | 98 / 98 / 98 | 0.0 | ±2 |
| callers precision | 94.1% | 85 | 90–97% | 94 / 94 / 95 | 1.4 | ±4 |
| callers recall, picked blind | 85.3% | 66 | 78–92% | 88 / 80 / 76 | 11.8 | ±7 |
| shared names: recall | 92.7% | 108 | 89–96% | 94 / 94 / 91 | 3.4 | ±4 |
| shared names: recall with maybe | 93.6% | 108 | 90–96% | 94 / 94 / 93 | 1.4 | ±4 |
| shared names: precision | 98.9% | 106 | 97–100% | 98 / 98 / 100 | 1.6 | ±2 |
| rename ok (tests pass) | 81.6% | 103 | 76–87% | 80 / 82 / 83 | 2.6 | ±6 |
| tests recall | 71.7% | 63 | 64–79% | 76 / 77 / 77 | 1.6 | ±8 |
| tests precision | 67.6% | 63 | 60–75% | 68 / 65 / 68 | 3.4 | ±8 |
| top-10 that fail | 73.3% | 63 | 66–80% | 75 / 70 / 74 | 5.0 | ±7 |
| … best possible order | 77.6% | 63 | 71–84% | 80 / 75 / 80 | 5.4 | ±7 |
| caught by the top 10 | 98.4% | 63 | 95–100% | 100 / 98 / 100 | 2.8 | ±3 |
| look / file | 14.9% | 103 | 11–19% | 12 / 15 / 14 | 3.4 | ±4 |

Sample `n20-m8-seeds7,8,9`: 20 functions and 8 mutants per repo and seed, seeds 7, 8, 9. Run time: 1 min 29 s; of the compilers' and test suites' answers 731 came from the cache, 1 were asked now.
