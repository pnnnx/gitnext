#!/usr/bin/env python3
"""The same means with the project as the unit: the bootstrap draws projects, not functions.

A run has few projects (ten), and functions of one project are alike; an interval over functions says how the number
moves with other functions of these projects, an interval over projects how it moves with other projects.

    python3 eval/cluster_boot.py [run ...]      # names in eval/results (default: latest); several runs are pooled
"""
import json
import random
import sys

import run as ev

BOOT = 5000
names = sys.argv[1:] or ["latest"]
results = [r for n in names for r in json.loads((ev.HERE / "results" / f"{n}.json").read_text()) if "error" not in r]
per_repo = [ev.metric_values(r) for r in results]
metrics = sorted({k for v in per_repo for k in v})
rng = random.Random(1)
mean = lambda xs: sum(xs) / len(xs)
print(f"runs: {', '.join(names)} · {len(results)} projects · 90% intervals, {BOOT} resamples")
print(f"{'metric':28} {'functions':>9} {'mean':>6}  {'over functions':>16}  {'over projects':>16}")
for m in metrics:
    groups = [v.get(m, []) for v in per_repo]
    groups = [g for g in groups if g]
    allv = [x for g in groups for x in g]
    if len(allv) < 5:
        continue
    f = sorted(mean([rng.choice(allv) for _ in allv]) for _ in range(BOOT))
    p = []
    for _ in range(BOOT):
        draw = [x for g in (rng.choice(groups) for _ in groups) for x in g]
        p.append(mean(draw))
    p.sort()
    lo, hi = int(0.05 * BOOT), int(0.95 * BOOT) - 1
    print(f"{m:28} {len(allv):>9} {100 * mean(allv):>5.0f}%  {100 * f[lo]:>6.0f}–{100 * f[hi]:<6.0f}  ±{50 * (f[hi] - f[lo]):<4.0f} {100 * p[lo]:>6.0f}–{100 * p[hi]:<6.0f}  ±{50 * (p[hi] - p[lo]):.0f}  ({len(groups)} projects)")
