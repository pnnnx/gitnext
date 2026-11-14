# Codebase-Memory on the functions of the run `latest`

`codebase-memory-mcp 0.12.1`. Its `CALLS` edges into each sampled function against the compiler's uses, next to gitnext on the same functions. ± is half of a 90% bootstrap interval over the functions; precision is over the functions for which the tool names a caller.

## Callers — names that occur once

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 232 | 85% ±4 | 96% ±2 | 100% ±0 | 99% ±1 |
| go | 71 | 97% ±3 | 91% ±5 | 100% ±0 | 98% ±2 |
| rust | 37 | 15% ±10 | 86% ±21 | 99% ±1 | 100% ±0 |
| java | 85 | 100% ±1 | 99% ±1 | 100% ±0 | 100% ±0 |
| typescript | 39 | 97% ±4 | 100% ±0 | 100% ±0 | 100% ±0 |

## Callers — methods that share a name

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 108 | 58% ±8 | 85% ±5 | 83% ±5 | 99% ±1 |
| go | 11 | 82% ±18 | 90% ±15 | 73% ±23 | 100% ±0 |
| rust | 31 | 5% ±6 | 38% ±12 | 80% ±11 | 99% ±1 |
| java | 56 | 80% ±8 | 84% ±6 | 90% ±6 | 99% ±1 |
| typescript | 10 | 75% ±22 | 100% ±0 | 65% ±25 | 100% ±0 |

## Callers — whether or not gitnext sees a caller

| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |
|---|---|---|---|---|---|
| all | 163 | 85% ±5 | 98% ±1 | 99% ±1 | 100% ±0 |
| go | 40 | 96% ±4 | 93% ±5 | 100% ±0 | 99% ±1 |
| rust | 26 | 18% ±12 | 100% ±0 | 100% ±0 | 100% ±0 |
| java | 63 | 99% ±1 | 100% ±1 | 98% ±3 | 100% ±0 |
| typescript | 34 | 97% ±4 | 100% ±0 | 100% ±0 | 100% ±0 |
