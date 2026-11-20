"""Summary of the fix-variant runs (Gemma 4 31B and 12B): solved, paired sign tests, bootstrap intervals.

    python3 summarize.py            the tables of summary.md (from the agent-*.json files next to it)
    python3 summarize.py --json     the same as summary.json
"""
import json, glob, re, math, random, statistics as st, sys
from scipy.stats import wilcoxon
SRC = next((a for a in sys.argv[1:] if not a.startswith("--")), __import__("os").path.dirname(__file__) + "/*")
NAMES = {"lite": "V1 show", "prove": "V2 prove", "try": "V3 try", "combo": "V4 combined"}
DOWN = ("Connection refused", "HTTP Error 5", "Remote end closed", "timed out")

def load(tag):
    rows = []
    for f in sorted(glob.glob(f"{SRC}/agent-{tag}-r*.json")):
        m = re.match(r".*agent-(.+)-r(\d+)-(.+)\.json$", f)
        rows += [{**r, "set": int(m.group(2))} for r in json.load(open(f))]
    return rows

def sign_p(g, l):
    n = g + l
    return 1.0 if n == 0 else min(1.0, 2 * sum(math.comb(n, i) for i in range(min(g, l) + 1)) / 2 ** n)

def boot(d, n=10000, seed=1):
    rnd = random.Random(seed); k = len(d)
    ms = sorted(sum(rnd.choice(d) for _ in range(k)) / k for _ in range(n))
    return ms[int(0.025 * n)], ms[int(0.975 * n)]

out, js = [], {}
for tag, modes, title in (("gemma-4-31b", ["lite", "prove", "try"], "Gemma 4 31B"), ("gemma-4-12b", ["lite", "prove", "combo"], "Gemma 4 12B")):
    every = load(tag)
    rows = [r for r in every if not any(t in str(r.get("error") or "") for t in DOWN)]
    J = js[tag] = {"runs": len(every), "server_down": len(every) - len(rows), "solved": {}, "pairs": {}}
    out.append(f"## {title}\n\n{len(every)} runs; {len(every) - len(rows)} fell on a llama-server crash and are left out.\n")
    out.append("| task | " + " | ".join(NAMES[m] for m in modes) + " |")
    out.append("|---|" + "---|" * len(modes))
    for kind in ("fix", "regress", None):
        cells = []
        for m in modes:
            rs = [r for r in rows if r["mode"] == m and (kind is None or r["task"] == kind)]
            s = sum(bool(r["passed"]) for r in rs)
            J["solved"][f"{kind or 'both'}/{m}"] = [s, len(rs)]
            cells.append(f"{s} of {len(rs)} ({100 * s / len(rs):.0f}%)")
        out.append(f"| {kind or '**both**'} | " + " | ".join(cells) + " |")
    out.append("\n| task | pair | tasks | solved | gained / lost | p (sign) | difference, points (95% interval) | input tokens, median ratio (p, Wilcoxon) |")
    out.append("|---|---|---|---|---|---|---|---|")
    key = lambda r: (r["repo"], r["set"], r["task"], r["target"])
    by = {(key(r), r["mode"]): r for r in rows}
    for kind in ("fix", "regress", None):
        for i, a in enumerate(modes):
            for b in modes[i + 1:]:
                ks = sorted({k for (k, m) in by if (kind is None or k[2] == kind) and (k, a) in by and (k, b) in by})
                x = [bool(by[(k, a)]["passed"]) for k in ks]; y = [bool(by[(k, b)]["passed"]) for k in ks]
                g = sum(yy and not xx for xx, yy in zip(x, y)); l = sum(xx and not yy for xx, yy in zip(x, y))
                d = [int(yy) - int(xx) for xx, yy in zip(x, y)]
                lo, hi = boot(d)
                ta = [by[(k, a)]["tokens_in"] for k in ks]; tb = [by[(k, b)]["tokens_in"] for k in ks]
                rt = st.median([v / u for u, v in zip(ta, tb) if u]); pt = wilcoxon([v - u for u, v in zip(ta, tb)]).pvalue
                p = sign_p(g, l)
                J["pairs"][f"{kind or 'both'}/{a}-{b}"] = dict(n=len(ks), a=sum(x), b=sum(y), gained=g, lost=l, p=p, diff=100 * sum(d) / len(d), ci=[100 * lo, 100 * hi], tokens_ratio=rt, tokens_p=pt)
                out.append(f"| {kind or '**both**'} | {NAMES[a]} → {NAMES[b]} | {len(ks)} | {sum(x)} → {sum(y)} | {g} / {l} | {'<0.001' if p < 0.001 else f'{p:.3f}'} | "
                           f"{100 * sum(d) / len(d):+.1f} ({100 * lo:+.1f} to {100 * hi:+.1f}) | ×{rt:.2f} ({pt:.3f}) |")
    out.append("\nSolved fix · regress by project:\n")
    out.append("| project | " + " | ".join(NAMES[m] for m in modes) + " |")
    out.append("|---|" + "---|" * len(modes))
    for repo in ["click", "semver", "chi", "sqlparse", "indexmap", "cobra"]:
        c = []
        for m in modes:
            c.append(" · ".join(f"{sum(bool(r['passed']) for r in rows if r['repo'] == repo and r['mode'] == m and r['task'] == t)}/{sum(1 for r in rows if r['repo'] == repo and r['mode'] == m and r['task'] == t)}" for t in ("fix", "regress")))
        out.append(f"| {repo} | " + " | ".join(c) + " |")
    if "combo" in modes:
        rs = [r for r in rows if r["mode"] == "combo"]
        out.append(f"\n`try` was called {sum((r.get('tools') or {}).get('try', 0) for r in rs)} times in {len(rs)} V4 runs.")
    out.append("")
if "--json" in sys.argv:
    print(json.dumps(js, indent=1))
else:
    print("\n".join(out))
