# Do the commands agree with each other and with the source?

`eval/consistency.py`, 30 functions per project (seed 5). What each column checks is at the top of the script.
A cell is `disagreements / checked`.

| project | callers→callees | callers→impact | callees→callers | callees sure, callers maybe | read | symbols | search | edit | hierarchy | dead: named elsewhere | dead deleted: new compile errors / failing tests |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| cobra | 0 / 59 | 0 / 59 | 0 / 79 | 0 / 79 | 0 / 30 | 0 / 288 | 0 / 30 | 0 / 20 | 0 / 0 | 0 / 0 | — |
| chi | 0 / 19 | 0 / 19 | 0 / 33 | 0 / 33 | 0 / 30 | 0 / 351 | 0 / 30 | 0 / 20 | 0 / 0 | 0 / 0 | — |
| click | 0 / 26 | 0 / 26 | 0 / 32 | 0 / 32 | 0 / 30 | 0 / 761 | 0 / 30 | 0 / 20 | 0 / 94 | 0 / 0 | — |
| sqlparse | 0 / 55 | 0 / 55 | 0 / 49 | 0 / 49 | 0 / 30 | 0 / 287 | 0 / 30 | 0 / 20 | 0 / 54 | 0 / 5 | 0 / 8 (5 deleted) |
| zustand | 0 / 21 | 0 / 21 | 0 / 24 | 0 / 24 | 0 / 30 | 0 / 175 | 0 / 30 | 0 / 20 | 0 / 0 | 0 / 0 | — |
| yaml | 0 / 47 | 0 / 47 | 0 / 36 | 0 / 36 | 0 / 30 | 0 / 440 | 0 / 30 | 0 / 20 | 0 / 32 | 0 / 0 | — |
| jsoup | 0 / 67 | 0 / 67 | 0 / 19 | 0 / 19 | 0 / 30 | 0 / 2661 | 0 / 27 | 0 / 20 | 0 / 200 | 0 / 1 | — |
| json-java | 0 / 51 | 0 / 51 | 0 / 42 | 0 / 42 | 0 / 30 | 0 / 540 | 0 / 30 | 0 / 20 | 0 / 10 | 0 / 1 | — |
| semver | 0 / 45 | 0 / 45 | 0 / 25 | 0 / 25 | 0 / 30 | 0 / 113 | 0 / 30 | 0 / 20 | 0 / 0 | 0 / 0 | — |
| indexmap | 0 / 9 | 0 / 9 | 0 / 30 | 0 / 30 | 0 / 30 | 0 / 1026 | 0 / 29 | 0 / 20 | 0 / 0 | 0 / 57 | 0 / 0 (57 deleted) |

## The disagreements

**sqlparse**
- dead deleted: test_duplicate_linebreaks
- dead deleted: test_except_formatting
- dead deleted: test_format_invalid_where_clause
- dead deleted: test_having_produces_newline
- dead deleted: test_identifier_list
- dead deleted: test_identifier_list_comment_first
- dead deleted: test_identifier_list_with_wrap_after
- dead deleted: test_issue315_utf8_by_default

