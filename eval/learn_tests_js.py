"""learn-tests prototype for TypeScript (zustand, yaml): why does `gitnext tests-for` find 3% of the tests that fail,
and does a per-test-file coverage map do better, and what does it cost?
The eval's truth for TS is a test *file* (`level="file"` in `run.LANG["typescript"]`), so the map is per file too:
every test file runs on its own with vitest's v8 coverage (`vitest run <file> --coverage.enabled …`,
`coverage-final.json`, istanbul format). A statement that ran is booked to the innermost function whose *body*
holds its line (from `run.body_start` to the end): the line of a declaration runs when the module is imported, so
counting it would book every function of every imported module to the test.
`tests-for` is scored by file as in `run.check_tests`. The driver is `learn_tests_go.measure`, with the test name
being the test file.
`--causes` writes, for every function whose failing test files `tests-for` misses, how each missed file reaches the
code (package name through the vitest alias, `index.ts` re-export, a relative import of `src/`, a helper), whether
gitnext sees test symbols in it, and what `tests-for` named instead.

    GITNEXT=$PWD/target/release/gitnext python3 eval/learn_tests_js.py [--repos zustand,yaml] [--n2 12] [--seed 7] \\
        [--pick-from <truth.json of an earlier run>] [--causes]
"""
from __future__ import annotations

import json
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import learn_tests_go as lg  # noqa: E402  the shared driver of 005
import run  # noqa: E402  the eval: same checkout, ground truth and cache

TS = run.LANG["typescript"]
COV_OPTS = ("--coverage.enabled --coverage.provider=v8 --coverage.reporter=json --coverage.include='src/**' "
            "--coverage.clean=true")


def tests_for_files(root: Path, q: str) -> tuple[list[str], list[str], set[str]]:
    """`run.tests_for` keyed by test file — the eval's TS level (`run.check_tests`)."""
    files, graph = [], set()
    for _, f, how in run.tests_for_rows(root, q):
        if f not in files:
            files.append(f)
        if "hop" in how or "direct" in how:
            graph.add(f)
    return files, files, graph


def body_owner(root: Path, syms: list[dict]) -> dict[str, list]:
    """file -> list indexed by line: the innermost function whose body holds that line (or None)."""
    by_file: dict[str, list[dict]] = {}
    for s in lg.fn_syms(syms):
        by_file.setdefault(s["file"], []).append(s)
    out = {}
    for f, ss in by_file.items():
        arr = [None] * (max(s["end_line"] for s in ss) + 2)
        for s in sorted(ss, key=lambda s: s["start_line"] - s["end_line"]):  # widest first, inner ones overwrite
            b = run.body_start(root, s, "typescript") or s["start_line"] + 1
            for i in range(s["start_line"], s["end_line"] + 1):
                # the inner function's header runs when the outer one does: it stays the outer one's
                if i >= b:
                    arr[i] = s["qualified"]
        out[f] = arr
    return out


def read_cov(path: Path, root: Path) -> dict[str, set[int]]:
    """coverage-final.json → file (relative) -> lines of statements that ran."""
    out: dict[str, set[int]] = {}
    if not path.exists():
        return out
    for f, v in json.loads(path.read_text()).items():
        try:
            rel = str(Path(f).relative_to(root))
        except ValueError:
            continue
        ls = {v["statementMap"][k]["start"]["line"] for k, n in v["s"].items() if n}
        if ls:
            out[rel] = ls
    return out


def test_files(root: Path) -> list[str]:
    """Test files of the plain run (`.gnx-vitest.json`, written by the eval's test command)."""
    d = json.loads((root / ".gnx-vitest.json").read_text())
    return sorted(str(Path(f["name"]).relative_to(root)) for f in d.get("testResults", []))


def js_collect(root: Path, syms: list[dict], work: Path) -> tuple[dict, dict]:
    files = test_files(root)
    owner = body_owner(root, syms)
    tests: dict = {}
    run_s = 0.0
    json_bytes = 0
    for f in files:
        d = work / "cov" / f.replace("/", "_")
        t = time.time()
        code, _ = run.sh(f"npx vitest run {f} {COV_OPTS} --coverage.reportsDirectory={d} >/dev/null 2>&1", root,
                         timeout=900)
        run_s += time.time() - t
        p = d / "coverage-final.json"
        json_bytes += p.stat().st_size if p.exists() else 0
        tests[f] = {"name": f, "failed": bool(code), "lines": read_cov(p, root)}
    # the cheap alternative: one run of the whole suite with coverage (no per-file map)
    t = time.time()
    run.sh(f"npx vitest run {COV_OPTS} --coverage.reportsDirectory={work}/cov-all >/dev/null 2>&1", root, timeout=1800)
    whole_s = time.time() - t
    lg.line_owner = lambda _syms: owner  # measure books lines through line_owner: use the body-only one
    return tests, {"run_one_by_one_s": round(run_s, 1), "map_s": round(run_s, 1), "whole_suite_coverage_s": round(whole_s, 1),
                   "coverage_json_bytes": json_bytes, "test_files": len(files)}


def js_library(file: str) -> bool:
    return (file.startswith("src/") and file.endswith((".ts", ".tsx")) and not file.endswith(".d.ts")
            and not run.LANG["typescript"].get("unseen", lambda f: False)(file))


# ---------------------------------------------------------------- causes

IMPORT = re.compile(r"""(?:from|import|require\()\s*['"]([^'"]+)['"]""")


def how_reached(root: Path, test: str, target: str, pkg: str) -> str:
    """How a test file reaches the file of the function: the first matching import kind."""
    src = (root / test).read_text(errors="replace")
    mods = IMPORT.findall(src)
    stem = Path(target).with_suffix("").as_posix()
    kinds = []
    for m in mods:
        if m == pkg or m.startswith(pkg + "/"):
            kinds.append("package name (vitest alias) → index re-export" if m == pkg else f"package subpath `{m}`")
        elif m.startswith("."):
            r = (root / test).parent.joinpath(m).resolve()
            try:
                rel = r.relative_to(root).with_suffix("").as_posix()
            except ValueError:
                continue
            if rel == stem:
                kinds.append("relative import of the function's file")
            elif rel.startswith("src"):
                kinds.append("relative import of another src file")
            else:
                kinds.append(f"helper `{rel}`")
    return "; ".join(dict.fromkeys(kinds)) or "no import of the library"


def ancestors(root: Path, q: str, hops: int = 8, maybe: bool = False) -> set[str]:
    """Library functions that reach `q` through the call graph (`gitnext callers`, sure — or also possible — edges)."""
    seen, front = {q}, [q]
    for _ in range(hops):
        got = run.callers_many(root, front)
        nxt = []
        for sure, poss in got.values():
            for c in sure | (poss if maybe else set()):
                if c not in seen:
                    seen.add(c)
                    nxt.append(c)
        if not nxt:
            break
        front = nxt
    return seen


def test_modules(root: Path, q: str, hops: int = 8, maybe: bool = False) -> dict[str, int]:
    """Test files whose `module` symbol reaches `q` in the real call graph (`callers --tests`). The graph books a
    call inside `it('…', () => …)` to the test file's module; `tests-for` skips it (not callable, locate.rs)."""
    seen, front, files = {q}, [q], {}
    for hop in range(1, hops + 1):
        code, out = run.gx(["--json", "callers", "--tests", *front], root)
        nxt = []
        for entry in (json.loads(out) if not code else []):
            for c in [x[0] for x in entry.get("callers", [])] + ([x[0] if isinstance(x, list) else x for x in entry.get("maybe") or []] if maybe else []):
                if c["file"].startswith("tests/") or ".test." in c["file"]:
                    files.setdefault(c["file"], hop)
                if c["qualified"] not in seen:
                    seen.add(c["qualified"])
                    nxt.append(c["qualified"])
        if not nxt:
            break
        front = nxt
    return files


def file_calls(text: str, names: set[str]) -> bool:
    """The test file's own code calls one of these names (`f(`, `.f(`, `new F(`) — what the graph would see if the
    file, arrow callbacks included, were one test symbol."""
    return any(re.search(rf"(?:\bnew\s+|[.\s(!]|^){re.escape(n)}\s*[(<]", text, re.M) for n in names)


def causes(repo: dict, a) -> dict:
    root = run.prepare(repo)
    run.gx(["init"], root, timeout=1800)
    syms = run.symbols(root)
    by_q = {s["qualified"]: s for s in syms}
    # `symbols` leaves test code out; `--tests` lists it: the named functions of a test file are all the graph
    # has there (the arrow inside `it('…', () => …)` is no symbol)
    _, out = run.gx(["--json", "symbols", "--tests"], root)
    test_syms: dict[str, int] = {}
    for s in json.loads(out):
        if s["file"].startswith("tests/") and s["kind"] in ("function", "method"):
            test_syms[s["file"]] = test_syms.get(s["file"], 0) + 1
    pkg = json.loads((root / "package.json").read_text())["name"]
    key = f"{repo['name']}@{repo['commit'][:12]}:tests:"
    known = json.loads(Path(a.pick_from).read_text()) if a.pick_from else (run._load_cache() or run.CACHE)
    all_tests = test_files(root) if (root / ".gnx-vitest.json").exists() else []
    rows = []
    for k, v in sorted(known.items()):
        if not k.startswith(key) or not v.get("failed"):
            continue
        q = k[len(key):]
        if q not in by_q:
            continue
        truth = set(v["failed"])
        named = run.tests_for_rows(root, q)
        nfiles = list(dict.fromkeys(f for _, f, _ in named))
        miss = sorted(truth - set(nfiles))
        short = lambda qs: {re.split(r"::", x)[-1] for x in qs if x in by_q}  # noqa: E731
        anc_sure, anc_all = short(ancestors(root, q)), short(ancestors(root, q, maybe=True))
        texts = {t: (root / t).read_text(errors="replace") for t in set(all_tests) | truth}
        named_s = {t for t in texts if file_calls(texts[t], anc_sure)} | set(nfiles)
        named_a = {t for t in texts if file_calls(texts[t], anc_all)} | set(nfiles)
        sim = {"sure": sorted(truth & named_s), "sure_or_possible": sorted(truth & named_a),
               "named_sure": len(named_s), "named_sure_or_possible": len(named_a),
               "ancestors_sure": len(anc_sure), "ancestors_all": len(anc_all)}
        gm = {}
        for kind, got in (("sure", test_modules(root, q)), ("sure_or_possible", test_modules(root, q, maybe=True))):
            for hops in (4, 8):  # `tests-for` goes 4 deep by default
                files = {f for f, h in got.items() if h <= hops} | set(nfiles)  # on top of what it names now
                gm[f"{kind}_{hops}"] = sorted(truth & files)
                gm[f"named_{kind}_{hops}"] = len(files)
        rows.append({"graph_with_test_modules": gm, "simulated_file_as_test": sim,"symbol": q, "file": by_q[q]["file"], "truth": len(truth), "named_files": len(nfiles),
                     "hit": len(truth & set(nfiles)), "named": nfiles,
                     "named_how": sorted({h for _, _, h in named}),
                     "missed": [{"test": t, "reach": how_reached(root, t, by_q[q]["file"], pkg),
                                 "test_symbols": test_syms.get(t, 0)} for t in miss]})
    return {"repo": repo["name"], "package": pkg, "test_files": len(all_tests), "test_functions_by_file": test_syms,
            "functions": rows}


CAUSES = (
    ("module4", "граф доходит до модуля тестового файла (вызов в `it(…, () => …)`), `tests-for` его отбрасывает: "
                "не функция (locate.rs, `is_callable`) — 4 перехода, уверенные рёбра"),
    ("possible4", "то же, но через «возможное» ребро (`.method` у получателя неизвестного типа) — 4 перехода"),
    ("deep", "то же, но глубже 4 переходов (5–8)"),
    ("edge", "в графе нет ребра по пути, хотя тестовый файл вызывает функцию-предка по имени (текст)"),
    ("dynamic", "ни граф, ни имя предка в тексте теста: экспорт под другим именем (`export const persist = persistImpl as …`), "
                "вызов через объект (`api.subscribe`), данные набора тестов"),
)


def cause_of(f: dict, t: str) -> str:
    gm, sim = f["graph_with_test_modules"], f["simulated_file_as_test"]
    if t in gm["sure_4"]:
        return "module4"
    if t in gm["sure_or_possible_4"]:
        return "possible4"
    if t in gm["sure_or_possible_8"]:
        return "deep"
    if t in sim["sure_or_possible"]:
        return "edge"
    return "dynamic"


def render_causes(res: dict) -> str:
    fs = [f for f in res["functions"] if f["missed"]]
    L = [f"# Почему `tests-for` не называет падающие файлы тестов: {res['repo']}", "",
         f"Функций с промахом: {len(fs)} из {len(res['functions'])}; падающих файлов {sum(f['truth'] for f in fs)}, "
         f"названо верно {sum(f['hit'] for f in fs)}, пропущено {sum(len(f['missed']) for f in fs)}. "
         f"Тестовых файлов {res['test_files']}, из них без единой именованной функции "
         f"{res['test_files'] - len(res['test_functions_by_file'])}.", "",
         "Пара «функция, пропущенный файл» по первой подходящей причине:", "",
         "| причина | пар | функций | пример (функция → файл) |", "|---|---:|---:|---|"]
    for k, text in CAUSES:
        pairs = [(f["symbol"], m["test"]) for f in fs for m in f["missed"] if cause_of(f, m["test"]) == k]
        ex = f"`{pairs[0][0].split('::')[-1]}` → `{pairs[0][1]}`" if pairs else ""
        L.append(f"| {text} | {len(pairs)} | {len({p[0] for p in pairs})} | {ex} |")
    no_fn = sum(1 for f in fs for m in f["missed"] if not m["test_symbols"])
    L += ["", f"Из пропущенных пар в файлах без именованных функций: {no_fn}; путь импорта (по парам):"]
    reach: dict[str, int] = {}
    for f in fs:
        for m in f["missed"]:
            k = "по имени пакета" if "package" in m["reach"] else "относительный" if "relative" in m["reach"] else "только помощник / нет"
            reach[k] = reach.get(k, 0) + 1
    L += [f"- {k}: {v}" for k, v in sorted(reach.items(), key=lambda x: -x[1])]
    L += ["", "Оценка починки графа (по функциям, среднее; `named` — сколько файлов назвал бы):", "",
          "| вариант | полнота | точность | функций с ответом |", "|---|---:|---:|---:|"]
    fsa = res["functions"]
    for key, k, text in (("graph_with_test_modules", "sure_4", "модули тестов, уверенные рёбра, 4 перехода"),
                         ("graph_with_test_modules", "sure_or_possible_4", "+ возможные рёбра, 4 перехода"),
                         ("graph_with_test_modules", "sure_8", "модули тестов, уверенные, 8 переходов"),
                         ("graph_with_test_modules", "sure_or_possible_8", "+ возможные, 8 переходов"),
                         ("simulated_file_as_test", "sure_or_possible", "потолок графа: файл вызывает предка по имени (текст, 8 переходов)")):
        rec = sum(len(f[key][k]) / f["truth"] for f in fsa) / len(fsa)
        pr = [len(f[key][k]) / f[key]["named_" + k] for f in fsa if f[key]["named_" + k]]
        L.append(f"| {text} | {rec:.0%} | {(sum(pr) / len(pr) if pr else 0):.0%} | {len(pr)} из {len(fsa)} |")
    now = sum(f["hit"] / f["truth"] for f in fsa) / len(fsa)
    L.append(f"| сейчас (`tests-for`) | {now:.0%} | | |")
    return "\n".join(L) + "\n"


def main() -> None:
    if "--causes" in sys.argv:
        import argparse
        ap = argparse.ArgumentParser()
        ap.add_argument("--repos", default="zustand,yaml")
        ap.add_argument("--pick-from")
        ap.add_argument("--causes", action="store_true")
        ap.add_argument("--render", action="store_true", help="only rewrite the .md files from the .json ones")
        a = ap.parse_args()
        if a.render:
            for name in a.repos.split(","):
                out = run.HERE / "results" / f"learn-tests-{name}"
                res = json.loads(out.with_suffix(".json").read_text())
                out.with_suffix(".md").write_text(lg.render(res).replace("при panic в начале", "при throw в начале"))
                c = out.with_suffix(".causes.json")
                if c.exists():
                    out.with_suffix(".causes.md").write_text(render_causes(json.loads(c.read_text())))
            return
        repos = {r["name"]: r for r in json.loads((run.HERE / "repos.json").read_text())}
        for name in a.repos.split(","):
            res = causes(repos[name], a)
            out = run.HERE / "results" / f"learn-tests-{name}.causes.json"
            out.write_text(json.dumps(res, ensure_ascii=False, indent=1))
            md = render_causes(res)
            out.with_suffix(".md").write_text(md)
            print(md, flush=True)
        return
    run.tests_for = tests_for_files  # measure scores `run.tests_for`: by file, like the eval does for TS
    go_render = lg.render
    lg.render = lambda res: go_render(res).replace("при panic в начале", "при throw в начале")
    lg.main_for("typescript", "zustand,yaml", js_collect, TS["tests"], js_library, __doc__)


if __name__ == "__main__":
    main()
