"""learn-tests prototype (Python, click): does a per-test coverage map find the tests that fail better than
`gitnext tests-for`, and what does it cost?

One pytest run with coverage per test (pytest-cov, `--cov-context=test`) gives "test -> lines it ran";
gitnext's function bounds turn that into "function -> tests that run it". Ground truth is the eval's: the tests
that fail when the function raises on entry (`run.failing_tests`). Two truths: the eval's (it keeps the last
400 lines of pytest output, so a function that breaks hundreds of tests loses some) and the full one.

Two samples: `run.pick` (functions the call graph already finds callers for, as in the eval) and a random one
without that condition: coverage is meant for the functions the graph does not see.

    GITNEXT=$PWD/target/release/gitnext python3 eval/learn_tests.py [--n 20] [--n2 16] [--seed 7] [--repeat 3]
"""
from __future__ import annotations

import argparse
import json
import random
import re
import sqlite3
import statistics
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import run  # noqa: E402  the eval: same checkout, sample, ground truth and metrics

TOP = run.TOP
TESTS = ".venv/bin/python -m pytest -q -p no:cacheprovider -x --maxfail=10000"  # the eval's run, without the tail
# the full truth: every FAILED / ERROR line, and a test module that does not import (the function runs at
# import and raises) does not stop the whole run — with the eval's command it does, and "no test fails"
FULL = TESTS + " -rfE --tb=no --continue-on-collection-errors 2>&1"
COLLECT = "collect:"  # marks a test file that failed to import; such files are counted, not scored


def full_failed(out: str) -> set[str]:
    return run.py_failed(out) | {COLLECT + m.group(1) for m in re.finditer(r"^ERROR (\S+\.py) - ", out, re.M)}


def lines_of(numbits: bytes) -> set[int]:
    """coverage.py numbits: bit i of byte j is line 8*j + i."""
    return {j * 8 + i for j, b in enumerate(numbits) if b for i in range(8) if b >> i & 1}


def test_name(ctx: str) -> str:
    """`tests/test_x.py::TestA::test_b[1]|run` (pytest-cov) or `test_x.TestA.test_b` (coverage.py
    test_function) -> `test_b`: the name `run.py_failed` gives a failing test."""
    c = ctx.split("|")[0].split("[")[0]
    return c.rsplit("::", 1)[-1] if "::" in c else c.rsplit(".", 1)[-1]


def coverage_run(root: Path, out: Path, mode: str) -> float:
    """One test run with coverage per test; returns seconds. mode: `pytest-cov` (setup/run/teardown of each
    test by node id) or `test_function` (coverage.py's own switch: it changes the label only when the next
    test function starts, so fixture setup is booked to the previous test)."""
    rc = out.with_suffix(".rc")
    data = out.resolve()
    data.unlink(missing_ok=True)
    if mode == "pytest-cov":
        rc.write_text(f"[run]\nsource = src\ndata_file = {data}\n")
        cmd = f"{TESTS} --cov --cov-config={rc} --cov-context=test --cov-report="
    else:
        rc.write_text(f"[run]\nsource = src\ndata_file = {data}\ndynamic_context = test_function\n")
        cmd = TESTS.replace(".venv/bin/python -m pytest", f".venv/bin/python -m coverage run --rcfile={rc} -m pytest")
    t = time.time()
    _, o = run.sh(cmd + " >/dev/null 2>&1; echo $?", root, timeout=3600)
    secs = time.time() - t
    if not data.exists():
        raise RuntimeError(f"no coverage data ({mode}): {o[-300:]}")
    return secs


def read_coverage(root: Path, data: Path) -> dict[str, dict[int, set[int]]]:
    """file (relative) -> context -> lines."""
    db = sqlite3.connect(data)
    files = {i: str(Path(p).resolve().relative_to(root.resolve())) for i, p in db.execute("select id, path from file")}
    ctxs = dict(db.execute("select id, context from context"))
    out: dict[str, dict[int, set[int]]] = {}
    for f, c, bits in db.execute("select file_id, context_id, numbits from line_bits"):
        out.setdefault(files[f], {}).setdefault(ctxs[c], set()).update(lines_of(bits))
    db.close()
    return out


def body_lines(root: Path, syms: list[dict]) -> dict[str, set[int]]:
    """qualified -> lines of its body that run only when it is called: from the line after the `:` to the
    end, minus the bodies of functions nested in it (they run when *those* are called). The `def` line and
    decorators run at import and say nothing."""
    fns = [s for s in syms if s["kind"] in ("function", "method") and s["file"].endswith(".py")]
    starts = {s["qualified"]: run.body_start(root, s, "python") for s in fns}
    by_file: dict[str, list[dict]] = {}
    for s in fns:
        by_file.setdefault(s["file"], []).append(s)
    out = {}
    for s in fns:
        b = starts[s["qualified"]]
        if not b:
            continue
        lines = set(range(b, s["end_line"] + 1))
        for o in by_file[s["file"]]:
            ob = starts[o["qualified"]]
            if o is not s and ob and s["start_line"] < o["start_line"] and o["end_line"] <= s["end_line"]:
                lines -= set(range(ob, o["end_line"] + 1))
        out[s["qualified"]] = lines
    return out


def build_map(cov: dict, bodies: dict[str, set[int]], syms: list[dict]) -> tuple[dict[str, set[str]], dict[str, int], dict]:
    """(function -> test contexts that run its body, context -> how many functions it runs, stats)."""
    file_of = {s["qualified"]: s["file"] for s in syms}
    fmap: dict[str, set[str]] = {}
    width: dict[str, int] = {}
    import_only = outside = never = 0
    for q, lines in bodies.items():
        per = cov.get(file_of[q], {})
        hit = {c.split("|")[0] for c, ls in per.items() if c and ls & lines}  # one test: setup + call + teardown
        ran_outside = bool(per.get("", set()) & lines)
        outside += ran_outside  # ran outside any test: import or collection time
        import_only += ran_outside and not hit
        never += not hit and not ran_outside
        if hit:
            fmap[q] = hit
            for c in hit:
                width[c] = width.get(c, 0) + 1
    return fmap, width, {"functions_run_outside_tests": outside, "functions_run_outside_tests_only": import_only,
                         "functions_never_run": never}


def cov_order(q: str, fmap, width, file_of) -> list[str]:
    """Tests that run `q`, narrowest first (fewest functions run), then the test file named after the module."""
    mod = Path(file_of[q]).stem.lstrip("_")
    best: dict[str, tuple] = {}
    for c in fmap.get(q, ()):
        n = test_name(c)
        key = (width[c], 0 if f"test_{mod}" in c.split("::")[0] else 1, n)
        if n not in best or key < best[n]:
            best[n] = key
    return sorted(best, key=best.get)


def union(gx: list[str], cov: list[str]) -> list[str]:
    """Both agree first (in tests-for order), then the rest of coverage, then the rest of tests-for."""
    c = set(cov)
    both = [t for t in gx if t in c]
    seen = set(both)
    return both + [t for t in cov if t not in seen] + [t for t in gx if t not in c]


def score(order: list[str], truth: set[str]) -> dict:
    """The eval's metrics (run.check_tests)."""
    pred = set(order)
    tp = len(pred & truth)
    top = order[:TOP]
    hits = sum(1 for x in top if x in truth)
    return {"recall": tp / len(truth), "precision": tp / len(pred) if pred else 0.0, "predicted": len(pred),
            "top_hits": hits, "top_n": len(top), "top_precision": hits / len(top) if top else 0.0,
            "caught": 1.0 if hits else 0.0}


def pick_random(syms: list[dict], root: Path, n: int, seed: int, skip: set[str]) -> list[dict]:
    """Functions of the library (src/) with a body of 3+ lines, no condition on callers."""
    c = [s for s in syms if s["kind"] in ("function", "method") and s["file"].startswith("src/")
         and not s.get("decl") and s["qualified"] not in skip
         and (b := run.body_start(root, s, "python")) and s["end_line"] - b + 1 >= 3]
    random.Random(seed).shuffle(c)
    return c[:n]


def truths(root: Path, repo: dict, s: dict, base_eval: set[str], base_full: set[str]) -> tuple[dict, dict]:
    key = f"{repo['name']}@{repo['commit'][:12]}"
    ev = run.cached(f"{key}:tests:{s['qualified']}", lambda: run.failing_tests(root, repo, s, base_eval))
    py = run.LANG["python"]
    saved = py["tests"], py["failed"]
    py["tests"], py["failed"] = FULL, lambda out, root: full_failed(out)
    try:
        full = run.cached(f"{key}:tests-full2:{s['qualified']}", lambda: run.failing_tests(root, repo, s, base_full))
    finally:
        py["tests"], py["failed"] = saved
    return ev, full


def mean(v):
    return round(statistics.mean(v), 3) if v else None


def summarize(rows: list[dict], truth: str) -> dict:
    out = {}
    rs = [r for r in rows if r.get(truth)]
    for src in ("gitnext", "coverage", "union"):
        out[src] = {m: mean([r[truth][src][m] for r in rs]) for m in ("recall", "precision", "top_precision", "caught")}
    out["functions"] = len(rs)
    return out


def pct(x):
    return "—" if x is None else f"{round(100 * x)}%"


def render(res: dict) -> str:
    L = ["# learn-tests на click: покрытие по тестам против `tests-for`",
         f"click {res['commit']}, gitnext {res['gitnext_commit']}, seed {res['seed']}. "
         "Метрики как в eval: полнота, точность, доля падающих в первых 10, поймано ли первыми 10.", ""]
    for truth, title in (("full", "полная истина (основная)"), ("eval", "истина eval (`tail -400`)")):
        L += [f"## Итог: {title}", "", "| выборка | функций | источник | полнота | точность | из 10 падают | поймано |",
              "|---|---:|---|---:|---:|---:|---:|"]
        for g, gname in (("pick", "pick (есть вызывающие)"), ("random", "случайные"), ("all", "все")):
            s = res["summary"][g][truth]
            for src in ("gitnext", "coverage", "union"):
                m = s[src]
                L.append(f"| {gname} | {s['functions']} | {src} | {pct(m['recall'])} | {pct(m['precision'])} | "
                         f"{pct(m['top_precision'])} | {pct(m['caught'])} |")
        L.append("")
    L += ["## По функциям (полная истина)", "",
          "| выборка | функция | падают (eval) | gitnext: полн / из 10 | покрытие: полн / точн / из 10 | объединение: полн / из 10 |",
          "|---|---|---:|---|---|---|"]
    for r in res["functions"]:
        if not r.get("full"):
            L.append(f"| {r['group']} | `{r['symbol']}` | 0 | {r.get('skipped', '')}: названо "
                     f"{r.get('predicted_gitnext', 0)} | названо {r.get('predicted_coverage', 0)} | |")
            continue
        g, c, u = (r["full"][k] for k in ("gitnext", "coverage", "union"))
        nb = f", не импортир. файлов {len(r['test_files_not_imported'])}" if r["test_files_not_imported"] else ""
        L.append(f"| {r['group']} | `{r['symbol']}` | {r['truth_full']} ({r['truth_eval']}{nb}) | "
                 f"{pct(g['recall'])} / {pct(g['top_precision'])} | {pct(c['recall'])} / {pct(c['precision'])} / "
                 f"{pct(c['top_precision'])} | {pct(u['recall'])} / {pct(u['top_precision'])} |")
    k = res["cost"]
    L += ["", "## Цена", "",
          f"- прогон тестов: без покрытия {k['plain_s']} с, pytest-cov по тестам {k['pytest_cov_s']} с "
          f"(×{k['slowdown']}), coverage `test_function` {k['test_function_s']} с; медиана из {k['repeat']}",
          f"- `.coverage` {k['coverage_db_bytes']} байт; карта «функция → тест»: {k['map_pairs']} пар, "
          f"{k['map_functions']} функций, {k['map_tests']} контекстов; SQLite (2 int на пару) {k['map_sqlite_bytes']} байт",
          f"- тестов собрано {k['items']}, разных имён тестов в карте {k['map_test_names']}",
          f"- функций с телом: {k['functions_with_body']}; исполнено хоть раз вне тестов (импорт, сбор): "
          f"{k['functions_run_outside_tests']}, только вне тестов: {k['functions_run_outside_tests_only']}; "
          f"не исполнено ни разу: {k['functions_never_run']}",
          f"- `test_function` против pytest-cov: пар {k['tf_pairs']} против {k['map_pairs']}; "
          f"тестов, падающих по полной истине, но не найденных: {k['tf_missed']} против {k['cov_missed']}"]
    return "\n".join(L) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--n", type=int, default=20, help="run.pick sample (the eval takes the first 8)")
    ap.add_argument("--n2", type=int, default=24, help="random sample without the callers condition")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--repeat", type=int, default=3, help="timed runs of each kind")
    ap.add_argument("--out", default="learn-tests-click")
    a = ap.parse_args()
    repo = next(r for r in json.loads((run.HERE / "repos.json").read_text()) if r["name"] == "click")
    root = run.prepare(repo)
    run.sh(".venv/bin/pip -q install coverage pytest-cov", root, timeout=1800)
    run.gx(["init"], root, timeout=1800)
    syms = run.symbols(root)
    file_of = {s["qualified"]: s["file"] for s in syms}

    # baselines: tests that already fail are not ours
    _, o = run.sh(run.LANG["python"]["tests"], root, timeout=2400)
    base_eval = run.py_failed(o)
    _, o = run.sh(FULL, root, timeout=2400)
    base_full = full_failed(o)
    items = run.sh(".venv/bin/python -m pytest -q --co -p no:cacheprovider 2>&1 | tail -1", root)[1].strip().split("/")[0]

    # cost and the map
    work = run.WORK / "learn-tests"
    work.mkdir(parents=True, exist_ok=True)
    plain, pcov, tfun = [], [], []
    for _ in range(a.repeat):
        t = time.time()
        run.sh(TESTS + " >/dev/null 2>&1", root, timeout=3600)
        plain.append(time.time() - t)
        pcov.append(coverage_run(root, work / "cov.db", "pytest-cov"))
        tfun.append(coverage_run(root, work / "tf.db", "test_function"))
    bodies = body_lines(root, syms)
    fmap, width, st = build_map(read_coverage(root, work / "cov.db"), bodies, syms)
    tmap, _, _ = build_map(read_coverage(root, work / "tf.db"), bodies, syms)
    msql = work / "map.sqlite"
    msql.unlink(missing_ok=True)
    db = sqlite3.connect(msql)
    db.execute("create table test(id integer primary key, name text)")
    db.execute("create table covers(symbol integer, test integer, primary key(symbol, test)) without rowid")
    tid = {c: i for i, c in enumerate(sorted(width))}
    sid = {q: i for i, q in enumerate(sorted(fmap))}
    db.executemany("insert into test values (?,?)", [(i, c) for c, i in tid.items()])
    db.executemany("insert into covers values (?,?)", [(sid[q], tid[c]) for q, cs in fmap.items() for c in cs])
    db.commit()
    db.execute("vacuum")
    db.close()

    sample = run.pick(syms, root, a.n, a.seed)
    rand = pick_random(syms, root, a.n2, a.seed, {s["qualified"] for s in sample})
    rows = []
    cov_missed = tf_missed = 0
    for group, ss in (("pick", sample), ("random", rand)):
        for s in ss:
            q = s["qualified"]
            print(f"  {group} {q}", flush=True)
            ev, full = truths(root, repo, s, base_eval, base_full)
            r = {"group": group, "symbol": q}
            if full.get("skipped") == "no body":
                continue
            broken = [f[len(COLLECT):] for f in full.get("failed", []) if f.startswith(COLLECT)]
            full = {**full, "failed": [f for f in full.get("failed", []) if not f.startswith(COLLECT)]}
            r["test_files_not_imported"] = broken
            r["eval_says_none_fail"] = "failed" in ev and not ev["failed"]
            gx_names, _, _ = run.tests_for(root, q)
            cov = cov_order(q, fmap, width, file_of)
            if "skipped" in full or not full.get("failed"):
                # nothing fails: every test named is a wasted run
                r.update(skipped=full.get("skipped") or "no test fails when it throws",
                         predicted_gitnext=len(gx_names), predicted_coverage=len(cov))
                rows.append(r)
                continue
            un = union(gx_names, cov)
            tf_names = {test_name(c) for c in tmap.get(q, ())}
            tf_full = set(full["failed"])
            cov_missed += len(tf_full - set(cov))
            tf_missed += len(tf_full - tf_names)
            r.update(truth_full=len(full["failed"]), truth_eval=len(ev.get("failed", [])),
                     missed_by_coverage=sorted(tf_full - set(cov))[:8],
                     covered_not_failing=sorted(set(cov) - tf_full)[:8], top_coverage=cov[:TOP])
            for tname, tr in (("full", tf_full), ("eval", set(ev.get("failed", [])))):
                if tr:
                    r[tname] = {"gitnext": score(gx_names, tr), "coverage": score(cov, tr), "union": score(un, tr)}
            rows.append(r)

    med = lambda v: round(statistics.median(v), 1)  # noqa: E731
    res = {"repo": "click", "commit": repo["commit"][:12], "seed": a.seed,
           "gitnext_commit": run.sh("git rev-parse --short HEAD", run.HERE)[1].strip(),
           "functions": rows,
           "summary": {g: {t: summarize([r for r in rows if g == "all" or r["group"] == g], t) for t in ("full", "eval")}
                       for g in ("pick", "random", "all")},
           "cost": {"repeat": a.repeat, "plain_s": med(plain), "pytest_cov_s": med(pcov), "test_function_s": med(tfun),
                    "slowdown": round(statistics.median(pcov) / statistics.median(plain), 1),
                    "coverage_db_bytes": (work / "cov.db").stat().st_size, "map_sqlite_bytes": msql.stat().st_size,
                    "map_pairs": sum(len(v) for v in fmap.values()), "map_functions": len(fmap), "map_tests": len(width),
                    "map_test_names": len({test_name(c) for c in width}), "items": items,
                    "tf_pairs": sum(len(v) for v in tmap.values()), "tf_missed": tf_missed, "cov_missed": cov_missed,
                    "functions_with_body": len(bodies), **st}}
    out = run.HERE / "results"
    (out / f"{a.out}.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
    md = render(res)
    (out / f"{a.out}.md").write_text(md)
    print("\n" + md)


if __name__ == "__main__":
    main()
