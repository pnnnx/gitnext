#!/bin/bash
# Check the result of one arm: bug fixed, feature added, all tests green.
#   bash verify.sh ~/Developer/ab/with
set -uo pipefail
D="${1:?folder of one arm}"
cd "$D" || exit 1
ok=0; total=0
check() { total=$((total+1)); if eval "$2" >/dev/null 2>&1; then ok=$((ok+1)); echo "✓ $1"; else echo "✗ $1"; fi; }

check "1. bug fixed: tests/test_brief.py passes" "python3 -m pytest -q tests/test_brief.py"
base=$(git log --format=%H --grep='ab: planted bug' -1)
check "1. test file not modified" "git diff --quiet $base -- tests/test_brief.py"
check "2. core_tools handled in the code" "grep -rq --include=*.py core_tools loca/"
check "2. a test mentions core_tools" "grep -rq core_tools tests/"
check "3. estimate_tokens renamed everywhere" "! grep -rqw --include=*.py estimate_tokens . && grep -rq --include=*.py 'def approx_tokens' loca/"
check "4. the whole test suite is green" "python3 -m pytest -q -x"
echo
echo "$ok/$total checks passed"
echo "changed files:"; git status --short | grep -v '__pycache__\|\.pytest_cache\|^?? .gitnext' | sed 's/^/  /'
if [ -d .gitnext ] && command -v gitnext >/dev/null; then
  echo; echo "gitnext usage in this session:"; gitnext journal 2>/dev/null | head -8 | sed 's/^/  /'
fi
