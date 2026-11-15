# Baselines on the functions of the run `latest`

The search for the name: `git grep -w <name>` in the files of the language; every hit is put into the function around it (callers) or counted for the test whose body has it (tests). The truth is the run's own: the compiler's uses, the tests that fail when the function throws. ± is half of a 90% bootstrap interval over the functions.

## Callers — names that occur once

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 232 | 100% ±0 | 89% ±3 | 100% ±0 | 99% ±1 |
| go | 71 | 100% ±0 | 92% ±4 | 100% ±0 | 98% ±2 |
| rust | 37 | 100% ±0 | 94% ±4 | 99% ±1 | 100% ±0 |
| java | 85 | 100% ±0 | 83% ±5 | 100% ±0 | 100% ±0 |
| typescript | 39 | 100% ±0 | 91% ±6 | 100% ±0 | 100% ±0 |

## Callers — methods that share a name

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 108 | 100% ±0 | 32% ±4 | 83% ±5 | 99% ±1 |
| go | 11 | 100% ±0 | 23% ±4 | 73% ±23 | 100% ±0 |
| rust | 31 | 100% ±0 | 27% ±7 | 80% ±11 | 99% ±1 |
| java | 56 | 100% ±0 | 36% ±6 | 90% ±6 | 99% ±1 |
| typescript | 10 | 100% ±0 | 32% ±19 | 65% ±25 | 100% ±0 |

## Callers — whether or not gitnext sees a caller

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 163 | 100% ±0 | 89% ±3 | 99% ±1 | 100% ±0 |
| go | 40 | 100% ±0 | 95% ±4 | 100% ±0 | 99% ±1 |
| rust | 26 | 100% ±0 | 89% ±8 | 100% ±0 | 100% ±0 |
| java | 63 | 100% ±0 | 84% ±5 | 98% ±3 | 100% ±0 |
| typescript | 34 | 100% ±0 | 89% ±7 | 100% ±0 | 100% ±0 |

## The tests of a function

"Caught": at least one of the first 10 tests named fails when the function throws. "Ten at random": the chance of that for ten tests drawn from all tests of the project; their precision is the share of the project's tests that fail.

| | functions | ten at random: caught | ten at random: precision | search: caught | search: recall | search: precision | gitnext: caught | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|---|---|---|---|
| all | 167 | 61% ±5 | 24% ±3 | 47% ±6 | 12% ±3 | 43% ±6 | 79% ±5 | 49% ±5 | 50% ±5 |
| go | 37 | 57% ±10 | 21% ±7 | 57% ±14 | 19% ±9 | 52% ±13 | 84% ±9 | 79% ±9 | 47% ±10 |
| rust | 22 | 34% ±10 | 8% ±4 | 27% ±16 | 13% ±8 | 27% ±16 | 73% ±14 | 53% ±14 | 43% ±13 |
| java | 43 | 59% ±10 | 22% ±6 | 63% ±12 | 8% ±5 | 55% ±12 | 95% ±5 | 49% ±11 | 61% ±10 |
| typescript | 22 | 90% ±7 | 43% ±8 | 23% ±14 | 10% ±8 | 21% ±14 | 41% ±18 | 19% ±9 | 34% ±15 |
| python | 43 | 64% ±9 | 29% ±7 | 44% ±12 | 11% ±6 | 44% ±12 | 81% ±9 | 36% ±9 | 53% ±10 |
