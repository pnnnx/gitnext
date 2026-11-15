| repo | lang | symbols | index s | callers recall | callers precision | callers recall, picked blind | shared names: recall (+maybe) | shared names: precision | rename ok | tests recall | tests precision | top-10 fail (best possible) | caught by top-10 | look / file |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| pflag | go | 756 | 0.1 | 98% ±10 (10) | 100% ±10 | 98% ±13 (8) | 89% ±7 (89% ±7) (34) | 97% ±5 | 82% ±19 (11) | 100% ±15 (7) | 64% ±23 | 64% ±22 (80% ±20) | 100% ±15 | 10% ±6 |
| mux | go | 128 | 0.1 | 100% ±20 (5) | 100% ±20 | 100% ±34 (3) | 100% ±10 (100% ±10) (10) | 100% ±10 | 100% ±20 (5) | 68% ±29 (4) | 100% ±25 | 100% ±25 (100% ±25) | 100% ±25 | 4% ±1 |
| itsdangerous | python | 79 | 0.1 | — | — | — | — | — | 100% ±13 (8) | 18% ±13 (8) | 70% ±17 | 73% ±15 (75% ±16) | 100% ±13 | 50% ±25 |
| more-itertools | python | 596 | 0.1 | — | — | — | — | — | 100% ±50 (2) | — | — | — | — | 1% ±1 |
| byteorder | rust | 198 | 0.4 | — | — | 0% ±25 (4) | 73% ±20 (73% ±20) (5) | 100% ±20 | — | — | — | — | — | — |
| either | rust | 150 | 0.3 | 100% ±50 (2) | 100% ±50 | 100% ±100 (1) | — | — | 100% ±50 (2) | — | — | — | — | 2% ±1 |
| strsim | rust | 48 | 0.4 | 100% ±20 (5) | 100% ±20 | 100% ±20 (5) | — | — | 100% ±8 (13) | 100% ±10 (11) | 75% ±17 | 90% ±14 (90% ±14) | 100% ±10 | 5% ±1 |
| ufo | typescript | 83 | 0.1 | 100% ±4 (28) | 100% ±4 | 85% ±12 (26) | — | — | 100% ±4 (28) | 77% ±8 (17) | 89% ±7 | 89% ±7 (89% ±7) | 100% ±6 | 20% ±7 |
| pathe | typescript | 57 | 0.1 | 100% ±10 (11) | 100% ±10 | 100% ±10 (11) | — | — | 18% ±19 (11) | 89% ±10 (11) | 100% ±10 | 100% ±10 (100% ±10) | 100% ±10 | 34% ±31 |
| urfave-cli | go | 703 | 0.9 | 96% ±6 (18) | 89% ±10 | 65% ±20 (13) | 60% ±40 (100% ±20) (5) | 100% ±34 | 84% ±14 (19) | 41% ±18 (13) | 42% ±17 | 53% ±17 (58% ±18) | 100% ±8 | 16% ±7 |
| logrus | go | 311 | 0.4 | 100% ±17 (6) | 92% ±17 | 100% ±17 (6) | 49% ±13 (96% ±5) (41) | 100% ±5 | 33% ±34 (6) | 59% ±20 (5) | 65% ±25 | 72% ±28 (72% ±28) | 100% ±20 | 9% ±2 |
| marshmallow | python | 352 | 0.2 | — | — | — | — | — | 69% ±16 (26) | 54% ±21 (13) | 38% ±17 | 39% ±18 (54% ±20) | 62% ±24 | 35% ±9 |
| boltons | python | 1171 | 0.2 | — | — | — | — | — | 69% ±11 (48) | 98% ±5 (23) | 40% ±13 | 42% ±11 (48% ±13) | 100% ±5 | 8% ±2 |
| itertools | rust | 1013 | 0.4 | 100% ±3 (39) | 93% ±6 | 96% ±6 (26) | 88% ±14 (96% ±8) (13) | 100% ±9 | 79% ±10 (47) | 61% ±15 (23) | 65% ±15 | 65% ±15 (68% ±15) | 78% ±16 | 20% ±8 |

`77% ±6 (20)`: the mean over 20 functions; a change smaller than ±6 points is noise. The ± is half the 90% interval
the number stays in when the functions are drawn again (bootstrap), and never less than what one function weighs.

| per language | all | go | python | rust | typescript |
|---|---:|---:|---:|---:|---:|
| callers recall | 99% ±1 (124) | 98% ±3 (39) | — | 100% ±3 (46) | 100% ±3 (39) |
| callers precision | 96% ±3 (124) | 94% ±6 (39) | — | 94% ±5 (46) | 100% ±3 (39) |
| callers recall, picked blind | 87% ±6 (103) | 84% ±11 (30) | — | 86% ±10 (36) | 89% ±9 (37) |
| shared names: recall | 73% ±7 (108) | 70% ±8 (90) | — | 84% ±12 (18) | — |
| shared names: recall with maybe | 94% ±4 (108) | 94% ±4 (90) | — | 90% ±8 (18) | — |
| shared names: precision | 99% ±2 (86) | 99% ±3 (69) | — | 100% ±6 (17) | — |
| rename ok (tests pass) | 77% ±5 (226) | 78% ±11 (41) | 73% ±8 (84) | 84% ±9 (62) | 77% ±11 (39) |
| tests recall | 72% ±6 (135) | 62% ±12 (29) | 70% ±10 (44) | 74% ±11 (34) | 82% ±7 (28) |
| tests precision | 64% ±6 (135) | 59% ±12 (29) | 44% ±9 (44) | 68% ±12 (34) | 93% ±5 (28) |
| top-10 that fail | 67% ±6 (135) | 65% ±12 (29) | 47% ±9 (44) | 73% ±11 (34) | 93% ±5 (28) |
| … best possible order | 71% ±6 (135) | 72% ±11 (29) | 54% ±10 (44) | 75% ±11 (34) | 93% ±5 (28) |
| caught by the top 10 | 93% ±4 (135) | 100% ±4 (29) | 89% ±8 (44) | 85% ±9 (34) | 100% ±4 (28) |
| look / file | 18% ±4 (226) | 12% ±4 (41) | 20% ±5 (84) | 16% ±6 (62) | 24% ±10 (39) |

| all repos | mean | functions | 90% interval | on each seed | 2 × SD of the seeds | threshold |
|---|---:|---:|---:|---:|---:|---:|
| callers recall | 99.2% | 124 | 98–100% | 99 / 99 / 99 | 0.0 | ±1 |
| callers precision | 95.9% | 124 | 93–98% | 96 / 96 / 97 | 1.0 | ±3 |
| callers recall, picked blind | 86.7% | 103 | 81–92% | 91 / 88 / 80 | 11.2 | ±6 |
| shared names: recall | 72.7% | 108 | 66–79% | 75 / 78 / 81 | 6.2 | ±7 |
| shared names: recall with maybe | 93.6% | 108 | 90–96% | 94 / 94 / 93 | 1.4 | ±4 |
| shared names: precision | 98.8% | 86 | 96–100% | 98 / 98 / 100 | 2.4 | ±2 |
| rename ok (tests pass) | 77.4% | 226 | 73–82% | 78 / 78 / 79 | 1.2 | ±5 |
| tests recall | 71.7% | 135 | 66–77% | 66 / 70 / 69 | 4.2 | ±6 |
| tests precision | 63.8% | 135 | 58–69% | 65 / 65 / 67 | 2.6 | ±6 |
| top-10 that fail | 67.1% | 135 | 62–72% | 69 / 68 / 71 | 2.4 | ±6 |
| … best possible order | 71.4% | 135 | 66–77% | 73 / 73 / 75 | 2.4 | ±6 |
| caught by the top 10 | 92.6% | 135 | 89–96% | 92 / 92 / 95 | 2.8 | ±4 |
| look / file | 18.2% | 226 | 15–22% | 18 / 20 / 20 | 2.2 | ±4 |

Sample `n20-m8-seeds7,8,9`. 14 projects measured in 3 runs (heldout-a, heldout-c, heldout-b); run time together: 112 min; of the compilers' and test suites' answers 209 came from the cache, 845 were asked.
