# The rename of a language server on the functions of the run `heldout`

`gopls rename` at the name in the definition, then the same check as for the tool: the project builds and the tests that passed before still pass. ± is half of a 90% bootstrap interval over the functions.

| | renames | gopls: builds and tests pass | gitnext: builds and tests pass | both | only gopls | only gitnext | neither |
|---|---|---|---|---|---|---|---|
| all | 41 | 88% ±9 | 78% ±10 | 28 | 8 | 4 | 1 |
| pflag | 11 | 100% ±0 | 82% ±18 | 9 | 2 | 0 | 0 |
| mux | 5 | 100% ±0 | 100% ±0 | 5 | 0 | 0 | 0 |
| urfave-cli | 19 | 74% ±16 | 84% ±13 | 12 | 2 | 4 | 1 |
| logrus | 6 | 100% ±0 | 33% ±33 | 2 | 4 | 0 | 0 |

## Where the server's rename did not pass

- `urfave-cli` `main::Command::VisibleFlagCategories` — tests: 
- `urfave-cli` `main::Command::VisibleFlags` — tests: 
- `urfave-cli` `main::Command::Walk` — build: # github.com/urfave/cli/v3_test
# [github.com/urfave/cli/v3_test]
./examples_test.go:445:1: ExampleCommand_Walk_visibleTree refers to unknown field or method: C
- `urfave-cli` `main::Command::FullName` — tests: 
- `urfave-cli` `main::Command::VisiblePersistentFlags` — tests: 
