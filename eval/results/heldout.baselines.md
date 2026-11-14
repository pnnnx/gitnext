# Baselines on the functions of the run `heldout`

The search for the name: `git grep -w <name>` in the files of the language; every hit is put into the function around it (callers) or counted for the test whose body has it (tests). The truth is the run's own: the compiler's uses, the tests that fail when the function throws. ± is half of a 90% bootstrap interval over the functions.

## Callers — names that occur once

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 124 | 100% ±0 | 84% ±4 | 99% ±1 | 95% ±3 |
| go | 39 | 100% ±0 | 81% ±8 | 98% ±2 | 91% ±6 |
| rust | 46 | 100% ±0 | 76% ±7 | 100% ±0 | 94% ±5 |
| typescript | 39 | 100% ±0 | 96% ±4 | 100% ±0 | 100% ±0 |

## Callers — methods that share a name

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 108 | 100% ±0 | 51% ±5 | 73% ±7 | 99% ±2 |
| go | 90 | 100% ±0 | 53% ±5 | 70% ±7 | 99% ±2 |
| rust | 18 | 100% ±0 | 40% ±9 | 84% ±11 | 100% ±0 |

## Callers — whether or not gitnext sees a caller

| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 103 | 100% ±0 | 85% ±4 | 87% ±6 | 94% ±4 |
| go | 30 | 100% ±0 | 87% ±7 | 84% ±11 | 89% ±9 |
| rust | 36 | 100% ±0 | 76% ±8 | 86% ±10 | 91% ±7 |
| typescript | 37 | 100% ±0 | 92% ±6 | 89% ±8 | 100% ±0 |

## The tests of a function

"Caught": at least one of the first 10 tests named fails when the function throws. "Ten at random": the chance of that for ten tests drawn from all tests of the project; their precision is the share of the project's tests that fail.

| | functions | ten at random: caught | ten at random: precision | search: caught | search: recall | search: precision | gitnext: caught | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|---|---|---|---|
| all | 135 | 52% ±5 | 20% ±4 | 70% ±7 | 33% ±5 | 65% ±6 | 93% ±4 | 72% ±5 | 64% ±5 |
| go | 29 | 55% ±10 | 22% ±9 | 76% ±12 | 14% ±6 | 71% ±13 | 100% ±0 | 62% ±12 | 59% ±12 |
| rust | 34 | 33% ±8 | 5% ±2 | 62% ±15 | 41% ±12 | 54% ±13 | 85% ±10 | 73% ±11 | 68% ±11 |
| typescript | 28 | 98% ±2 | 36% ±7 | 71% ±14 | 38% ±12 | 71% ±14 | 100% ±0 | 82% ±6 | 93% ±5 |
| python | 44 | 37% ±10 | 21% ±9 | 70% ±11 | 37% ±11 | 66% ±11 | 89% ±8 | 70% ±10 | 45% ±9 |
