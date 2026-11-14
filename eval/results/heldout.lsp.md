# Language servers on the functions of the run `heldout`

References of the definition, asked from gopls (Go), rust-analyzer (Rust) and tsserver (TypeScript); every reference is put into the function around it. The truth is the run's own (the compiler's uses). ± is half of a 90% bootstrap interval over the functions.

## Names that occur once

| | functions | server: recall | server: precision | gitnext: recall | gitnext: precision | search: recall | search: precision |
|---|---|---|---|---|---|---|---|
| all three | 124 | 100% ±0 | 100% ±1 | 99% ±1 | 95% ±3 | 100% ±0 | 84% ±4 |
| go | 39 | 100% ±0 | 100% ±0 | 98% ±2 | 91% ±6 | 100% ±0 | 81% ±8 |
| rust | 46 | 99% ±1 | 99% ±2 | 100% ±0 | 94% ±5 | 100% ±0 | 76% ±7 |
| typescript | 39 | 100% ±0 | 100% ±0 | 100% ±0 | 100% ±0 | 100% ±0 | 96% ±4 |

## Methods that share a name

| | functions | server: recall | server: precision | gitnext: recall | gitnext: precision | search: recall | search: precision |
|---|---|---|---|---|---|---|---|
| all three | 108 | 99% ±1 | 99% ±1 | 73% ±7 | 99% ±2 | 100% ±0 | 51% ±5 |
| go | 90 | 100% ±0 | 98% ±2 | 70% ±7 | 99% ±2 | 100% ±0 | 53% ±5 |
| rust | 18 | 96% ±6 | 100% ±0 | 84% ±11 | 100% ±0 | 100% ±0 | 40% ±9 |

## Time

| project | server | ready after | per function |
|---|---|---|---|
| pflag | gopls | 0.0 s | 0.4 s |
| mux | gopls | 0.0 s | 0.35 s |
| byteorder | rust-analyzer | 10.8 s | 0.15 s |
| either | rust-analyzer | 3.8 s | 0.33 s |
| strsim | rust-analyzer | 3.6 s | 0.06 s |
| ufo | tsserver | 0.0 s | 0.06 s |
| pathe | tsserver | 0.0 s | 0.22 s |
| urfave-cli | gopls | 0.0 s | 0.59 s |
| logrus | gopls | 0.0 s | 0.35 s |
| itertools | rust-analyzer | 6.9 s | 0.1 s |
