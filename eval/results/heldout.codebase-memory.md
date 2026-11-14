# Codebase-Memory on the functions of the run `heldout`

`codebase-memory-mcp 0.12.1`. Its `CALLS` edges into each sampled function against the compiler's uses, next to gitnext on the same functions. ± is half of a 90% bootstrap interval over the functions; precision is over the functions for which the tool names a caller.

## Callers — names that occur once

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 124 | 71% ±6 | 85% ±5 | 99% ±1 | 95% ±3 |
| go | 39 | 76% ±9 | 73% ±10 | 98% ±2 | 91% ±6 |
| rust | 46 | 62% ±12 | 84% ±10 | 100% ±0 | 94% ±5 |
| typescript | 39 | 77% ±11 | 100% ±0 | 100% ±0 | 100% ±0 |

## Callers — methods that share a name

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 108 | 10% ±5 | 72% ±16 | 73% ±7 | 99% ±2 |
| go | 90 | 7% ±4 | 56% ±21 | 70% ±7 | 99% ±2 |
| rust | 18 | 29% ±16 | 88% ±19 | 84% ±11 | 100% ±0 |

## Callers — whether or not gitnext sees a caller

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 103 | 60% ±8 | 83% ±6 | 87% ±6 | 94% ±4 |
| go | 30 | 62% ±13 | 73% ±12 | 84% ±11 | 89% ±9 |
| rust | 36 | 52% ±14 | 77% ±14 | 86% ±10 | 91% ±7 |
| typescript | 37 | 65% ±13 | 100% ±0 | 89% ±8 | 100% ±0 |
