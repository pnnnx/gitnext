#!/bin/bash
# A/B test of gitnext with a real agent (Claude Code): two identical copies of
# a project, the same bug planted in both, gitnext set up in only one.
#
#   bash setup.sh                         # copies ~/Developer/loca → ~/Developer/ab/{with,without}
#   bash setup.sh ~/Developer/loca ~/ab   # other source / destination
set -euo pipefail

SRC="${1:-$HOME/Developer/loca}"
DST="${2:-$HOME/Developer/ab}"

[ -d "$SRC/.git" ] || { echo "error: $SRC is not a git repository"; exit 1; }
[ -e "$DST" ] && { echo "error: $DST already exists — remove it or pass another folder"; exit 1; }
command -v gitnext >/dev/null || { echo "error: gitnext is not on PATH (cargo install --path crates/gitnext-cli)"; exit 1; }

mkdir -p "$DST"
for arm in without with; do
  git clone -q "$SRC" "$DST/$arm"
  cd "$DST/$arm"
  # the same planted bug in both copies: brief() cuts text one character too long
  python3 - <<'PY'
from pathlib import Path
p = Path("loca/agent.py")
s = p.read_text(encoding="utf-8")
bug = "first[:limit - 1].rstrip()"
if bug not in s:
    raise SystemExit("error: loca/agent.py changed — cannot plant the bug (look for brief())")
p.write_text(s.replace(bug, "first[:limit + 1].rstrip()"), encoding="utf-8")
Path("tests/test_brief.py").write_text(
    "import unittest\n\nfrom loca.agent import brief\n\n\n"
    "class BriefTest(unittest.TestCase):\n"
    "    def test_long_text_fits_limit(self):\n"
    "        self.assertEqual(len(brief('x' * 100, 40)), 40)\n\n"
    "    def test_short_text_unchanged(self):\n"
    "        self.assertEqual(brief('Read a file. More text'), 'Read a file')\n",
    encoding="utf-8")
PY
  git add -A
  git -c user.name=ab -c user.email=ab@example.invalid -c core.hooksPath=/dev/null commit -qm "ab: planted bug + failing test"
done

cd "$DST/with"
gitnext init >/dev/null
gitnext setup claude --hooks
git add -A
git -c user.name=ab -c user.email=ab@example.invalid -c core.hooksPath=/dev/null commit -qm "ab: gitnext"

echo
echo "ready:"
echo "  $DST/without   — plain project"
echo "  $DST/with      — same project + gitnext (MCP, CLAUDE.md block, hooks)"
echo "open Claude Code in each folder in a NEW session and paste the same prompt from PROMPT.md"
