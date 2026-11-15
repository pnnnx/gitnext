#!/usr/bin/env python3
"""Several runs of eval/run.py over different projects as one: the per-project results joined, the summary over all.

    python3 eval/merge_runs.py <out> <run> <run> ...     # names in eval/results → results/<out>.{json,noise.json,md}
"""
import json
import sys

import run as ev

out, parts = sys.argv[1], sys.argv[2:]
order = [r["name"] for r in json.loads((ev.HERE / "repos-heldout.json").read_text())]
results, secs, truth = [], 0, {"cached": 0, "asked": 0}
config = None
for p in parts:
    results += json.loads((ev.HERE / "results" / f"{p}.json").read_text())
    n = json.loads((ev.HERE / "results" / f"{p}.noise.json").read_text())
    secs += n["seconds"]
    config = config or n["config"]
    for k in truth:
        truth[k] += n["truth"].get(k, 0)
results.sort(key=lambda r: order.index(r["repo"]) if r["repo"] in order else len(order))
agg = ev.scopes(results)
note = (f"Sample `{config}`. {len(results)} projects measured in {len(parts)} runs ({', '.join(parts)}); run time together: {secs // 60} min; "
        f"of the compilers' and test suites' answers {truth['cached']} came from the cache, {truth['asked']} were asked.")
d = ev.HERE / "results"
(d / f"{out}.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
(d / f"{out}.noise.json").write_text(json.dumps({"config": config, "seconds": secs, "truth": truth, "scopes": agg}, ensure_ascii=False, indent=1))
md = ev.render_md(results, agg, note)
(d / f"{out}.md").write_text(md)
print(md)
