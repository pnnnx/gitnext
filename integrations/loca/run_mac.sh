#!/bin/sh
# Benchmark on your own machine: loca + a local model, each task without gitnext
# ("off") and with `gitnext context` added before the request ("ctx").
#
#   1. start the model in another terminal:   ~/Developer/loca/bin/llama.sh
#   2. run:                                     sh integrations/loca/run_mac.sh
#
# Your loca repository is not changed: the benchmark runs on a temporary copy
# with the two small patches from this folder (the before_prompt hook, and the
# benchmark's stats file kept outside the project, so the agent's grep does not
# find its own trace). Results: integrations/loca/runs/<machine>-<date>.json
#
# Options (environment): LOCA=path/to/loca  REPEAT=3  TASKS=…  MODES=off,ctx  PORT=8080
set -e

GN=$(cd "$(dirname "$0")/../.." && pwd)
LOCA=${LOCA:-$HOME/Developer/loca}
REPEAT=${REPEAT:-3}
TASKS=${TASKS:-find_definition,big_find_class,failing_test,big_failing_test,big_rename}
MODES=${MODES:-off,ctx}
PORT=${PORT:-8080}
OUT="$GN/integrations/loca/runs"
mkdir -p "$OUT"

command -v gitnext >/dev/null 2>&1 || { echo "gitnext is not on PATH: cargo install --path $GN/crates/gitnext-cli --force"; exit 1; }
if ! gitnext --help 2>/dev/null | grep -qw "libs"; then
  echo "gitnext is too old (no 'libs' command): cargo install --path $GN/crates/gitnext-cli --force"; exit 1
fi
if ! curl -s "http://127.0.0.1:$PORT/health" | grep -q ok; then
  echo "the model server is not answering on port $PORT — start it first: $LOCA/bin/llama.sh"; exit 1
fi

# a throwaway copy of loca with the patches applied
TMP=$(mktemp -d)
mkdir "$TMP/loca"
(cd "$LOCA" && tar cf - --exclude .git --exclude .venv --exclude '__pycache__' --exclude .gitnext --exclude runs .) | (cd "$TMP/loca" && tar xf -)
cd "$TMP/loca"
grep -q before_prompt loca/hooks.py || patch -s -p1 < "$GN/integrations/loca/before_prompt.patch"
grep -q 'loca_home / "stats.json"' bench/run.py || patch -s -p1 < "$GN/integrations/loca/bench_stats_outside.patch"

NAME="$(hostname -s 2>/dev/null || echo mac)-$(date +%Y%m%d-%H%M)"
echo "loca copy: $TMP/loca · model on :$PORT · tasks: $TASKS · modes: $MODES · ×$REPEAT"
echo "results → $OUT/$NAME.json (log: $OUT/$NAME.log)"
python3 "$GN/integrations/loca/bench_gitnext.py" --loca "$TMP/loca" \
  --backend llamacpp --host "http://127.0.0.1:$PORT" --ctx "${LOCA_CTX:-16384}" \
  --tasks "$TASKS" --modes "$MODES" --repeat "$REPEAT" --out "$OUT/$NAME.json" 2>&1 | tee "$OUT/$NAME.log"
