<!-- gitnext:agent:start — written by `gitnext setup`, safe to delete -->
## Code navigation: gitnext

This repo is indexed by gitnext (MCP server `gitnext`, or the `gitnext` command). Every extra step re-sends the whole conversation, so prefer one call that answers everything:

1. `look` first. For a function / class / method it returns the code, who calls it, what it calls, the tests that reach it with the command to run them, and what a change would affect — one call instead of read + grep + callers + test search. Pass several targets at once (`look` with `targets: ["Agent.execute", "brief", "loca/config.py"]`, or `gitnext look A B C`).
2. A file or folder in `look` gives every signature with line numbers; an error or traceback gives the code where it happened.
3. Edit by symbol: `replace_symbol` / `insert_after` / `rename_symbol` (rename also fixes imports, `__all__`, tests and `module.func` uses, and lists any text it did not change) — no need to read the file first.
4. New repo: `overview` once. Resuming: `brief`, then `changes`.
5. Worked out what undocumented code does? `annotate` it in one line; later sessions get it for free.
6. A test fails: `why <test>` runs it and returns the failure in a few lines and the code to look at, the most likely culprit first. After a change: `gitnext check` builds and runs the tests of what changed. A long build or test log: `… 2>&1 | gitnext digest` gives what failed, where and the message.

Plain reads are fine for non-code files and small files.
<!-- gitnext:agent:end -->
