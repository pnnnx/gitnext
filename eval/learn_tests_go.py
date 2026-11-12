"""learn-tests prototype for Go (cobra, chi): does a per-test coverage map find the tests that fail better than
`gitnext tests-for`, and what does it cost?

Go has no per-test coverage, so every test runs on its own: one test binary per package built with coverage of
the whole module (`go test -c -cover -coverpkg=<module>/...`, the binary keeps the name `go test` gives it — cobra
reads the command line as its own arguments under any other name), then `pkg.test -test.run '^TestX$'
-test.coverprofile=…` per top-level test. A profile block that ran is booked to the innermost function holding its
first line. Ground truth is the eval's (`run.failing_tests`, tests run one at a time, cached in truth.json).

Two samples: the functions whose truth the eval already has (`run.pick` of earlier runs) and random functions of
the library with a body of 3+ lines (no condition on callers). The driver (`measure`) is shared with
`learn_tests_rust.py`.

    GITNEXT=$PWD/target/release/gitnext python3 eval/learn_tests_go.py [--repos cobra,chi] [--n2 12] [--seed 7] \\
        [--pick-from <truth.json of an earlier run>]
"""
from __future__ import annotations

import argparse
import json
import random
import re
import shutil
import sqlite3
import statistics
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import learn_tests as lt  # noqa: E402  metrics, union, rendering helpers of the Python prototype
import run  # noqa: E402  the eval: same checkout, ground truth and cache

TOP = run.TOP
SOURCES = ("gitnext", "coverage", "union")


# ---------------------------------------------------------------- shared driver

def fn_syms(syms: list[dict]) -> list[dict]:
    return [s for s in syms if s["kind"] in ("function", "method") and not s.get("decl")]


def line_owner(syms: list[dict]) -> dict[str, list]:
    """file -> list indexed by line: the innermost function holding that line (or None)."""
    by_file: dict[str, list[dict]] = {}
    for s in fn_syms(syms):
        by_file.setdefault(s["file"], []).append(s)
    out = {}
    for f, ss in by_file.items():
        arr = [None] * (max(s["end_line"] for s in ss) + 2)
        for s in sorted(ss, key=lambda s: s["start_line"] - s["end_line"]):  # widest first, inner ones overwrite
            for i in range(s["start_line"], s["end_line"] + 1):
                arr[i] = s["qualified"]
        out[f] = arr
    return out


def build_map(tests: dict, owner: dict) -> tuple[dict[str, set[str]], dict[str, int]]:
    """tests: id -> {"lines": {file: set(lines)}} → (function -> test ids, test id -> functions it ran)."""
    fmap: dict[str, set[str]] = {}
    width: dict[str, int] = {}
    for tid, t in tests.items():
        ran = set()
        for f, ls in t["lines"].items():
            arr = owner.get(f)
            if arr:
                ran |= {arr[i] for i in ls if i < len(arr) and arr[i]}
        width[tid] = len(ran)
        for q in ran:
            fmap.setdefault(q, set()).add(tid)
    return fmap, width


def cov_order(q: str, fmap, width, name_of) -> list[str]:
    """Bare test names that ran `q`, narrowest first; on a tie the test whose name holds the function's."""
    short = re.split(r"[.:]", q)[-1].lower()
    best: dict[str, tuple] = {}
    for tid in fmap.get(q, ()):
        n = name_of(tid)
        key = (width[tid], 0 if short in n.lower() else 1, n)
        if n not in best or key < best[n]:
            best[n] = key
    return sorted(best, key=best.get)


def summarize(rows: list[dict]) -> dict:
    rs = [r for r in rows if r.get("score")]
    out = {src: {m: lt.mean([r["score"][src][m] for r in rs]) for m in ("recall", "precision", "top_precision", "caught")}
           for src in SOURCES}
    out["functions"] = len(rs)
    return out


def map_sqlite(path: Path, fmap: dict, width: dict) -> int:
    path.unlink(missing_ok=True)
    db = sqlite3.connect(path)
    db.execute("create table test(id integer primary key, name text)")
    db.execute("create table covers(symbol integer, test integer, primary key(symbol, test)) without rowid")
    tid = {c: i for i, c in enumerate(sorted(width))}
    sid = {q: i for i, q in enumerate(sorted(fmap))}
    db.executemany("insert into test values (?,?)", [(i, c) for c, i in tid.items()])
    db.executemany("insert into covers values (?,?)", [(sid[q], tid[c]) for q, cs in fmap.items() for c in cs])
    db.commit()
    db.execute("vacuum")
    db.close()
    return path.stat().st_size


def measure(repo: dict, collect, plain_cmd: str, library, a) -> dict:
    """One repo. collect(root, syms, work) -> (tests, cost): tests = id -> {"name", "failed", "lines"}."""
    L = run.LANG[repo["lang"]]
    key = f"{repo['name']}@{repo['commit'][:12]}"
    tkey = L.get("tests_key", "tests")
    root = run.prepare(repo)
    run.gx(["init"], root, timeout=1800)
    syms = run.symbols(root)
    by_q = {s["qualified"]: s for s in fn_syms(syms)}
    work = run.WORK / "learn-tests" / repo["name"]
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)

    plain = []
    for _ in range(a.repeat):
        t = time.time()
        _, tout = run.sh(plain_cmd, root, timeout=3600)
        plain.append(time.time() - t)
    together_failed = L["failed"](tout, root)
    tests, cost = collect(root, syms, work)
    owner = line_owner(syms)
    fmap, width = build_map(tests, owner)
    name_of = lambda tid: tests[tid]["name"]  # noqa: E731
    alone_failed = {t["name"] for t in tests.values() if t["failed"]}

    # samples: functions with an answer in the cache first, then random ones of the library
    prefix = f"{key}:{tkey}:"
    if not run.CACHE:
        run._load_cache()
    # the eval's own sample: the keys of the cache as the eval left it (`--pick-from`), not after this script
    # added its random functions to it
    known = json.loads(Path(a.pick_from).read_text()) if a.pick_from else run.CACHE
    cached = [q[len(prefix):] for q in known if q.startswith(prefix)]
    pick = [by_q[q] for q in sorted(cached) if q in by_q]
    cands = [s for s in fn_syms(syms) if not s.get("test") and library(s["file"]) and s["qualified"] not in set(cached)
             and (b := run.body_start(root, s, repo["lang"])) and s["end_line"] - b + 1 >= 3]
    random.Random(a.seed).shuffle(cands)
    rand = cands[:a.n2]
    base = set(run.cached(f"{key}:baseline", lambda: run.baseline_tests(root, L)).get("failed", []))

    rows = []
    for group, ss in (("pick", pick), ("random", rand)):
        for s in ss:
            q = s["qualified"]
            print(f"  {repo['name']} {group} {q}", flush=True)
            tr = run.cached(f"{prefix}{q}", lambda: run.failing_tests(root, repo, s, base),
                            keep=lambda v: v.get("skipped") != "the test suite did not run")
            gx_names, _, _ = run.tests_for(root, q)
            cov = cov_order(q, fmap, width, name_of)
            r = {"group": group, "symbol": q, "file": s["file"]}
            if "skipped" in tr or not tr.get("failed"):
                r.update(skipped=tr.get("skipped") or "no test fails when it throws",
                         predicted_gitnext=len(gx_names), predicted_coverage=len(cov))
                rows.append(r)
                continue
            truth = set(tr["failed"])
            un = lt.union(gx_names, cov)
            r.update(truth=len(truth), truth_doctests=len({t for t in truth if "/" in t or t.endswith(".rs")}),
                     predicted_gitnext=len(gx_names), predicted_coverage=len(cov),
                     missed_by_coverage=sorted(truth - set(cov))[:10], covered_not_failing=sorted(set(cov) - truth)[:10],
                     missed_by_coverage_n=len(truth - set(cov)), covered_not_failing_n=len(set(cov) - truth),
                     top_coverage=cov[:TOP],
                     score={"gitnext": lt.score(gx_names, truth), "coverage": lt.score(cov, truth), "union": lt.score(un, truth)})
            rows.append(r)

    med = lambda v: round(statistics.median(v), 1)  # noqa: E731
    cost.update(repeat=a.repeat, plain_s=med(plain), tests=len(tests),
                map_pairs=sum(len(v) for v in fmap.values()), map_functions=len(fmap),
                map_sqlite_bytes=map_sqlite(work / "map.sqlite", fmap, width),
                functions_with_body=len([s for s in fn_syms(syms) if not s.get("test") and library(s["file"])]),
                functions_never_run=len([s for s in fn_syms(syms) if not s.get("test") and library(s["file"]) and s["qualified"] not in fmap]),
                fail_alone_not_together=sorted(alone_failed - together_failed - base),
                fail_together_not_alone=sorted(together_failed - alone_failed - base))
    cost["slowdown_runs"] = round(cost["map_s"] / cost["plain_s"], 1) if cost["plain_s"] else None
    return {"repo": repo["name"], "lang": repo["lang"], "commit": repo["commit"][:12], "seed": a.seed,
            "gitnext_commit": run.sh("git rev-parse --short HEAD", run.HERE)[1].strip(),
            "functions": rows,
            "summary": {g: summarize([r for r in rows if g == "all" or r["group"] == g]) for g in ("pick", "random", "all")},
            "cost": cost}


def pct(x):
    return lt.pct(x)


def render(res: dict) -> str:
    k = res["cost"]
    L = [f"# learn-tests на {res['repo']} ({res['lang']}): покрытие по тестам против `tests-for`",
         f"{res['repo']} {res['commit']}, gitnext {res['gitnext_commit']}, seed {res['seed']}. Истина — eval "
         "(тесты, падающие при panic в начале функции). pick — функции с ответом в кэше eval, random — случайные.", "",
         "| выборка | функций | источник | полнота | точность | из 10 падают | поймано |", "|---|---:|---|---:|---:|---:|---:|"]
    for g, gname in (("pick", "pick (кэш eval)"), ("random", "случайные"), ("all", "все")):
        s = res["summary"][g]
        for src in SOURCES:
            m = s[src]
            L.append(f"| {gname} | {s['functions']} | {src} | {pct(m['recall'])} | {pct(m['precision'])} | "
                     f"{pct(m['top_precision'])} | {pct(m['caught'])} |")
    L += ["", "## По функциям", "", "| выборка | функция | падают | граф: полн / из 10 | покрытие: полн / точн / из 10 | мимо покрытия |",
          "|---|---|---:|---|---|---|"]
    for r in res["functions"]:
        if "score" not in r:
            L.append(f"| {r['group']} | `{r['symbol']}` | 0 | {r['skipped']} | названо {r['predicted_coverage']} | |")
            continue
        g, c = r["score"]["gitnext"], r["score"]["coverage"]
        L.append(f"| {r['group']} | `{r['symbol']}` | {r['truth']} | {pct(g['recall'])} / {pct(g['top_precision'])} | "
                 f"{pct(c['recall'])} / {pct(c['precision'])} / {pct(c['top_precision'])} | "
                 f"{', '.join(r['missed_by_coverage'][:4])} |")
    L += ["", "## Цена", ""] + [f"- `{x}`: {v}" for x, v in k.items()]
    return "\n".join(L) + "\n"


def main_for(lang: str, default: str, collect, plain_cmd, library, doc: str) -> None:
    ap = argparse.ArgumentParser(description=doc, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default=default)
    ap.add_argument("--n2", type=int, default=12, help="random functions per repo, besides the cached ones")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--repeat", type=int, default=3, help="timed plain test runs")
    ap.add_argument("--pick-from", help="truth.json as the eval left it: its functions are the `pick` group "
                    "(default: the current cache, which also holds the random ones of earlier runs)")
    a = ap.parse_args()
    repos = {r["name"]: r for r in json.loads((run.HERE / "repos.json").read_text())}
    for name in a.repos.split(","):
        res = measure(repos[name], collect, plain_cmd, library, a)
        out = run.HERE / "results" / f"learn-tests-{name}"
        out.with_suffix(".json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
        md = render(res)
        out.with_suffix(".md").write_text(md)
        print("\n" + md, flush=True)


# ---------------------------------------------------------------- Go

def go_profile(path: Path, module: str) -> dict[str, set[int]]:
    """`mod/pkg/file.go:12.30,14.2 1 3` blocks that ran → file (relative) -> first lines of those blocks."""
    out: dict[str, set[int]] = {}
    if not path.exists():
        return out
    for line in path.read_text().splitlines()[1:]:
        m = re.match(r"(.+\.go):(\d+)\.\d+,\d+\.\d+ \d+ (\d+)$", line)
        if m and m.group(3) != "0":
            f = m.group(1)
            f = f[len(module) + 1:] if f.startswith(module + "/") else f
            out.setdefault(f, set()).add(int(m.group(2)))
    return out


def go_collect(root: Path, syms: list[dict], work: Path) -> tuple[dict, dict]:
    module = run.sh("go list -m", root)[1].strip().splitlines()[0]
    _, dirs = run.sh("go list -f '{{.Dir}}' ./...", root)
    tests: dict = {}
    build_s = run_s = 0.0
    prof_bytes = 0
    testmain = []
    for d in [Path(x) for x in dirs.split() if x.startswith("/")]:
        rel = str(d.relative_to(root)) or "."
        out = work / "bin" / rel.replace("/", "_")
        out.mkdir(parents=True, exist_ok=True)
        t = time.time()
        code, o = run.sh(f"go test -c -cover -coverpkg={module}/... -o {out}/ .", d, timeout=1800)
        build_s += time.time() - t
        if code:
            print(f"    build {rel}: {o[-300:]}", flush=True)
        if any("func TestMain(" in p.read_text(errors="replace") for p in d.glob("*_test.go")):
            testmain.append(rel)
        for binary in out.glob("*.test"):
            _, names = run.sh(f"{binary} -test.list '^(Test|Example|Fuzz)'", d)
            for n in [x for x in names.split() if re.match(r"(Test|Example|Fuzz)\w*$", x)]:
                prof = work / "prof" / f"{rel.replace('/', '_')}.{n}.out"
                prof.parent.mkdir(exist_ok=True)
                t = time.time()
                code, _ = run.sh(f"{binary} -test.run '^{n}$' -test.count=1 -test.coverprofile={prof}", d, timeout=300)
                run_s += time.time() - t
                prof_bytes += prof.stat().st_size if prof.exists() else 0
                tests[f"{rel}:{n}"] = {"name": n, "failed": bool(code), "lines": go_profile(prof, module)}
                prof.unlink(missing_ok=True)
    return tests, {"build_s": round(build_s, 1), "run_one_by_one_s": round(run_s, 1), "map_s": round(build_s + run_s, 1),
                   "profiles_bytes": prof_bytes, "packages": len(dirs.split()), "testmain_packages": testmain}


def go_library(file: str) -> bool:
    return file.endswith(".go") and not file.endswith("_test.go") and not run.LANG["go"]["unseen"](file)


if __name__ == "__main__":
    main_for("go", "cobra,chi", go_collect, "go test -count=1 -json ./... 2>&1", go_library, __doc__)
