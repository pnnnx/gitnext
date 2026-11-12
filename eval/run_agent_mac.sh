#!/bin/sh
# The plain agent (eval/agent.py) on a local model, without loca:
# base tools vs + gitnext tools (eleven tools) vs only gitnext's short list (`lite`, six tools),
# judged by each project's own compiler and tests.
#
#   1. model in another terminal:   ~/Developer/loca/bin/llama.sh   (any OpenAI-compatible server)
#   2. sh eval/run_agent_mac.sh
#
# Options: REPOS=click,semver  TASKS=find,callers,fix,rename  MODES=base,gitnext,lite  REPEAT=3  PORT=8080  LEARN=1
#   (other modes: verify, ctx, both, cmd; REPEAT=N is N different task sets per project;
#    LEARN=0: no `gitnext learn-tests` before the `fix` tasks; a Rust project needs
#    `rustup component add llvm-tools-preview` for it, a Python one pytest-cov — installed by the run)
# Projects are cloned into eval/.work (pinned commits, see eval/repos.json); needs git, python3, cargo.
# Results: eval/results/agent-<machine>-<date>.json / .md / .log / .trace.jsonl (every conversation)
set -e
GN=$(cd "$(dirname "$0")/.." && pwd)
REPOS=${REPOS:-click,semver}
TASKS=${TASKS:-find,callers,fix,rename}
MODES=${MODES:-base,gitnext,lite}
REPEAT=${REPEAT:-3}
PORT=${PORT:-8080}
LEARN=${LEARN:-1}

command -v gitnext >/dev/null 2>&1 || { echo "gitnext is not on PATH: cargo install --path $GN/crates/gitnext-cli --force"; exit 1; }
if ! curl -s "http://127.0.0.1:$PORT/health" | grep -q ok; then
  echo "the model server is not answering on port $PORT — start it first (e.g. ~/Developer/loca/bin/llama.sh)"; exit 1
fi
NAME="agent-$(hostname -s 2>/dev/null || echo mac)-$(date +%Y%m%d-%H%M)"
mkdir -p "$GN/eval/results"
cd "$GN/eval"
GITNEXT=$(command -v gitnext) python3 agent.py --host "http://127.0.0.1:$PORT" --repos "$REPOS" --tasks "$TASKS" \
  --modes "$MODES" --repeat "$REPEAT" $([ "$LEARN" = 1 ] && echo --learn) --out "$GN/eval/results/$NAME.json" 2>&1 | tee "$GN/eval/results/$NAME.log"
