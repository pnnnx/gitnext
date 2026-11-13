#!/usr/bin/env python3
"""The tables of an agent run over several models: solved, steps and tokens by model, kind and mode;
the same task in two modes as a pair (sign test, bootstrap over tasks).

    python3 eval/agent_stats.py eval/results/agent-kaggle-20261113.json

The file is a list of the rows `eval/agent.py` writes, each with `model` and `set` (the task set, 0-based) added.
"""
import json, sys, statistics, collections, random, math
rows = json.load(open(sys.argv[1]))
MODELS = ["qwen3.5-4b", "qwen3.5-9b", "qwen3.5-27b"]; MODES = ["base", "gitnext", "lite"]; TASKS = ["find", "callers", "fix", "rename"]
by = collections.defaultdict(list)
for r in rows:
    by[(r["model"], r["task"], r["mode"])].append(r); by[(r["model"], "all", r["mode"])].append(r)
def cell(rs):
    ok = sum(bool(r.get("passed")) for r in rs); st = [r["steps"] for r in rs if r.get("steps")]
    tok = [r["tokens_in"] for r in rs if r.get("tokens_in")]; sec = [r["seconds"] for r in rs if r.get("seconds")]
    return ok, len(rs), statistics.median(st) if st else 0, sum(tok) / max(1, len(tok)) / 1000, sum(sec) / max(1, len(sec))
print("errors (no steps):", collections.Counter((r["model"], r["mode"]) for r in rows if not r.get("steps")))
for m in MODELS:
    print(f"\n== {m}")
    for t in TASKS + ["all"]:
        print(f"  {t:8}", " | ".join(f"{md}: {c[0]:2}/{c[1]} st {c[2]:>4.0f} tok {c[3]:>5.1f}k {c[4]:>4.0f}s" for md in MODES for c in [cell(by[(m, t, md)])]))
# paired: the same task (repo, set, task) in two modes
def paired(m, a, b, task=None):
    key = lambda r: (r["repo"], r["set"], r["task"])
    A = {key(r): r for r in rows if r["model"] == m and r["mode"] == a and (task is None or r["task"] == task)}
    B = {key(r): r for r in rows if r["model"] == m and r["mode"] == b and (task is None or r["task"] == task)}
    ks = sorted(set(A) & set(B))
    win = sum(bool(B[k]["passed"]) and not A[k]["passed"] for k in ks); lose = sum(bool(A[k]["passed"]) and not B[k]["passed"] for k in ks)
    n = win + lose
    p = sum(math.comb(n, i) for i in range(0, min(win, lose) + 1)) * 2 / 2 ** n if n else 1.0
    rng = random.Random(1); d = [bool(B[k]["passed"]) - bool(A[k]["passed"]) for k in ks]
    bs = sorted(sum(rng.choice(d) for _ in d) / len(d) for _ in range(4000))
    both = [k for k in ks if A[k].get("tokens_in") and B[k].get("tokens_in")]
    ratio = statistics.median(A[k]["tokens_in"] / B[k]["tokens_in"] for k in both) if both else 0
    solved = [k for k in ks if A[k]["passed"] and B[k]["passed"] and A[k].get("tokens_in") and B[k].get("tokens_in")]
    r2 = statistics.median(A[k]["tokens_in"] / B[k]["tokens_in"] for k in solved) if solved else 0
    return len(ks), win, lose, min(1.0, p), sum(d) / len(d), bs[100], bs[3899], ratio, r2, len(solved)
print("\npaired, the same 72 tasks: n, +wins, -losses, sign-test p, diff [95% CI], median token ratio (all), (both solved, n)")
for m in MODELS:
    for a, b in (("base", "gitnext"), ("base", "lite"), ("gitnext", "lite")):
        n, w, l, p, d, lo, hi, ra, r2, ns = paired(m, a, b)
        print(f"  {m:12} {a:>7} -> {b:7} n={n} +{w} -{l} p={p:.4f} diff {d*100:+.0f} pp [{lo*100:+.0f}, {hi*100:+.0f}]  tokens x{ra:.1f} (both solved x{r2:.1f}, n={ns})")
print("\nby repo, solved of 12 per mode:")
for m in MODELS:
    print(" ", m, " ".join(f"{repo}:" + "/".join(str(sum(bool(r['passed']) for r in rows if r['model'] == m and r['repo'] == repo and r['mode'] == md)) for md in MODES) for repo in ["click", "sqlparse", "semver", "indexmap", "cobra", "chi"]))
print("\nguard stops:", {m: collections.Counter((r["mode"], (r.get("guard") or {}).get("stopped")) for r in rows if r["model"] == m and (r.get("guard") or {}).get("stopped")) for m in MODELS})
