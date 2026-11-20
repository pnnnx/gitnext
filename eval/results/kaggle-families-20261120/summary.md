# Models of four other families: the full run

1104 runs, 7.7 hours, two T4 cards; notebook `gitnext-kaggle-families-v2.ipynb`. The notebook's own summary with
paired tests is [report.md](report.md); every run is an `agent-<model>-r<set>-<project>.json` file here (traces and
server logs stayed in the Kaggle output). Run 10 in [../HISTORY.md](../HISTORY.md) has the setup and what it shows.

| model | family | tasks | base → lite | gained / lost | p (sign) |
|---|---|---|---|---|---|
| Gemma 4 12B | Google | 120 | 55 → 84 | 31 / 2 | <0.001 |
| gpt-oss-20b | OpenAI | 120 | 48 → 77 | 39 / 10 | <0.001 |
| Ministral 3 14B | Mistral AI | 120 | 51 → 85 | 39 / 5 | <0.001 |
| Granite 4.1 8B | IBM | 120 | 38 → 85 | 47 / 0 | <0.001 |
| Qwen 3.5 9B (anchor) | Qwen | 72 | 47 → 53 | 10 / 4 | 0.18 |

The four new models together: 192 → 331 of 480, +29.0 points (95% interval +22.9 to +35.0), p < 0.001.
