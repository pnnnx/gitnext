# The rename of a language server on the functions of the run `latest`

`gopls rename` at the name in the definition, then the same check as for the tool: the project builds and the tests that passed before still pass. ± is half of a 90% bootstrap interval over the functions.

| | renames | gopls: builds and tests pass | gitnext: builds and tests pass | both | only gopls | only gitnext | neither |
|---|---|---|---|---|---|---|---|
| all | 81 | 96% ±4 | 93% ±4 | 73 | 5 | 2 | 1 |
| cobra | 46 | 93% ±5 | 91% ±7 | 40 | 3 | 2 | 1 |
| chi | 35 | 100% ±0 | 94% ±7 | 33 | 2 | 0 | 0 |

## Where the server's rename did not pass

- `cobra` `main::Command::Execute` — tests: 
- `cobra` `main::Command::UseLine` — tests: 
- `cobra` `main::flagCompError::Error` — rename: gopls: cobra/completions.go:52:25: renaming this method "Error" to "ErrorGnx"
	would make *github.com/spf13/cobra.flagCompError no longer assignable to interfac
