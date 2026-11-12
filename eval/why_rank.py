#!/usr/bin/env python3
"""Where does `gitnext why <test>` put the function that is really at fault?

A bug is seeded into a project the way eval/agent.py does it (one operator changed inside a
function, kept if some test starts failing), committed, and `gitnext why <failing test> --json`
is asked for its list. The answer is known: the function that was changed. Measured: how often
it is first, in the first 3, in the first 8 (what the text output lists), or in the list at all.

    GITNEXT=$PWD/target/release/gitnext python3 eval/why_rank.py --repos click,sqlparse,semver,cobra --bugs 12
    … --learn                              `gitnext learn-tests` on the clean code first (Python repos)
    … --variants static,nochange,default   compare (GITNEXT_WHY_RANK, for measuring only): the call graph
                                           alone; learned coverage without the "changed since the map
                                           was learned" mark; everything

The bugs found for a repo are kept in eval/.work/why-bugs-<repo>.json: the next run asks only gitnext.
Results: eval/results/why-rank.json / .md.
"""
from __future__ import annotations

import argparse
import json
import random
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import agent  # noqa: E402
import run as ev  # noqa: E402


def bugs_of(repo: dict, root: Path, n: int, seed: int) -> list[dict]:
    """`n` seeded bugs with different targets: patch, the changed function, a test that fails."""
    store = ev.WORK / f"why-bugs-{repo['name']}.json"
    have = json.loads(store.read_text()) if store.exists() else {}
    key = f"{repo['commit'][:12]}:{seed}"
    bugs = have.get(key, [])
    syms = ev.symbols(root)
    k = len(bugs)
    tries = 0
    while len(bugs) < n and tries < n * 3:
        tries += 1
        b = agent.seed_bug(repo, root, syms, random.Random(seed * 1000 + k))
        k += 1
        if b and not any(x["target"] == b["target"] for x in bugs):
            bugs.append({"target": b["target"], "patch": b["patch"], "tests": b["failing"]})
            have[key] = bugs
            store.write_text(json.dumps(have, ensure_ascii=False, indent=1))
            print(f"  bug {len(bugs)}: {b['target']}  ({len(b['failing'])} failing tests)", flush=True)
    # a test that fails on the untouched code as well (a permission test run as root) explains nothing
    L = ev.LANG[repo["lang"]]
    before = set(L["failed"](ev.sh(L["tests"], root, timeout=1800)[1], root))
    kept = [{**b, "tests": [t for t in b["tests"] if t not in before]} for b in bugs[:n]]
    return [b for b in kept if b["tests"]]


def rank(root: Path, bug: dict, variant: str) -> dict:
    """The place of the changed function in the list for the first failing test `why` can find."""
    env = {"GITNEXT_WHY_RANK": variant} if variant else {}
    import os
    for test in bug["tests"][:3]:
        name = test.split("::")[-1].split("[")[0].split("/")[0]
        p = subprocess.run([ev.GITNEXT, "why", name, "--json"], cwd=root, capture_output=True, text=True, env={**os.environ, **env}, timeout=900)
        try:
            d = json.loads(p.stdout)
        except ValueError:
            continue
        if not d.get("failed"):
            continue
        names = [s["symbol"] for s in d["suspects"]]
        place = names.index(bug["target"]) + 1 if bug["target"] in names else None
        return {"test": name, "place": place, "listed": len(names), "named": bool(place and d["suspects"][place - 1]["named"]), "learned": bool(d.get("learned"))}
    return {"test": None, "place": None, "listed": 0, "named": False, "learned": False}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default="click,sqlparse,semver,cobra")
    ap.add_argument("--bugs", type=int, default=12)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--variants", default="", help="comma-separated GITNEXT_WHY_RANK values; empty = the default order only")
    ap.add_argument("--learn", action="store_true", help="learn the coverage map on the clean code before the bugs are put in")
    ap.add_argument("--out", default=str(HERE / "results" / "why-rank.json"))
    a = ap.parse_args()
    variants = a.variants.split(",") if a.variants else [""]
    repos = [r for r in json.loads((HERE / "repos.json").read_text()) if r["name"] in a.repos.split(",")]
    rows = []
    t0 = time.time()
    for repo in repos:
        print(f"== {repo['name']} ({repo['lang']})", flush=True)
        root = ev.prepare(repo)
        base = ev.sh("git rev-parse HEAD", root)[1].strip()
        clean = lambda: (ev.sh(f"git reset -q --hard {base}", root), ev.restore(root))  # noqa: E731
        clean()
        ev.gx(["index"], root, timeout=1800)
        bugs = bugs_of(repo, root, a.bugs, a.seed)
        clean()
        (root / ".gitnext" / "tests.db").unlink(missing_ok=True)
        if a.learn:
            ev.gx(["index"], root)
            print("  " + (ev.gx(["learn-tests"], root, timeout=3600)[1].strip().splitlines() or ["learn-tests: no output"])[0], flush=True)
        for b in bugs:
            clean()
            subprocess.run(["git", "apply"], input=b["patch"], text=True, cwd=root)
            ev.sh("git -c user.name=eval -c user.email=eval@example.invalid -c commit.gpgsign=false commit -qam 'seeded bug'", root)
            ev.gx(["index"], root)
            for v in variants:
                r = rank(root, b, v)
                rows.append({"repo": repo["name"], "lang": repo["lang"], "target": b["target"], "variant": v or "default", **r})
        clean()
        (root / ".gitnext" / "tests.db").unlink(missing_ok=True)
        ev.gx(["index"], root)
    Path(a.out).write_text(json.dumps(rows, ensure_ascii=False, indent=1))
    md = ["| order | repo | bugs | first | in 3 | in 8 | in the list | empty list | named in the failure | from the learned map |", "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for v in [x or "default" for x in variants]:
        for name in [r["name"] for r in repos] + ["all"]:
            rs = [r for r in rows if r["variant"] == v and (name == "all" or r["repo"] == name)]
            if not rs:
                continue
            n = len(rs)
            top = lambda k: sum(1 for r in rs if r["place"] and r["place"] <= k)  # noqa: E731
            md.append(f"| {v} | {name} | {n} | {top(1)} ({100 * top(1) // n}%) | {top(3)} ({100 * top(3) // n}%) | {top(8)} ({100 * top(8) // n}%) | {top(10**9)} | {sum(1 for r in rs if r['listed'] == 0)} | {sum(1 for r in rs if r['named'])} | {sum(1 for r in rs if r['learned'])} |")
    Path(a.out).with_suffix(".md").write_text("\n".join(md) + f"\n\n{len(rows)} answers in {round(time.time() - t0)} s.\n")
    print("\n".join(md))


if __name__ == "__main__":
    main()
