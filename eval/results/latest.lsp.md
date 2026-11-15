# Language servers on the functions of the run `latest`

References of the definition, asked from gopls (Go), rust-analyzer (Rust) and tsserver (TypeScript); every reference is put into the function around it. The truth is the run's own (the compiler's uses). ± is half of a 90% bootstrap interval over the functions.

## Names that occur once

| | functions | server: recall | server: precision | gitnext: recall | gitnext: precision | search: recall | search: precision |
|---|---|---|---|---|---|---|---|
| all three | 147 | 100% ±0 | 100% ±0 | 100% ±0 | 99% ±1 | 100% ±0 | 92% ±3 |
| go | 71 | 100% ±0 | 100% ±0 | 100% ±0 | 98% ±2 | 100% ±0 | 92% ±4 |
| rust | 37 | 100% ±0 | 100% ±0 | 99% ±1 | 100% ±0 | 100% ±0 | 94% ±4 |
| typescript | 39 | 100% ±0 | 100% ±0 | 100% ±0 | 100% ±0 | 100% ±0 | 91% ±6 |

## Methods that share a name

| | functions | server: recall | server: precision | gitnext: recall | gitnext: precision | search: recall | search: precision |
|---|---|---|---|---|---|---|---|
| all three | 52 | 97% ±3 | 99% ±1 | 75% ±9 | 100% ±1 | 100% ±0 | 27% ±5 |
| go | 11 | 100% ±0 | 95% ±7 | 73% ±23 | 100% ±0 | 100% ±0 | 23% ±4 |
| rust | 31 | 95% ±6 | 100% ±0 | 80% ±11 | 99% ±1 | 100% ±0 | 27% ±7 |
| typescript | 10 | 100% ±0 | 100% ±0 | 65% ±25 | 100% ±0 | 100% ±0 | 32% ±19 |

## Time

| project | server | ready after | per function |
|---|---|---|---|
| cobra | gopls | 0.0 s | 0.62 s |
| chi | gopls | 0.0 s | 0.65 s |
| zustand | tsserver | 0.0 s | 3.99 s |
| yaml | tsserver | 0.0 s | 0.09 s |
| semver | rust-analyzer | 11.0 s | 0.33 s |
| indexmap | rust-analyzer | 9.4 s | 0.05 s |
