#!/usr/bin/env python3
"""gitnext eval: check gitnext's answers against the compilers and test suites
of real, popular repositories pinned to exact commits.

Nothing here trusts gitnext or a human judgement. Each check gets its ground
truth from the project itself:

  callers   rename a function's *definition* only and compile: every error the
            compiler reports is a real use of it. Compared with `gitnext callers`
            → recall (uses gitnext found) and precision (callers that are real).
  rename    `gitnext rename <f> <f>Gnx --text`, then the project must still
            compile and its tests must still pass.
  tests     make the function fail on entry and run the whole test suite:
            the tests that fail are the ones that exercise it. Compared with
            `gitnext tests-for` → recall / precision.
  index     time, size, share of calls resolved inside the repo.
  look      tokens of one `gitnext look` vs reading the files it covers.

    python3 eval/run.py                      # all repos in eval/repos.json
    python3 eval/run.py --repos cobra,click --n 6 --mutants 3
    python3 eval/run.py --check              # compare with eval/baseline.json, exit 1 on regression
    python3 eval/run.py --bless              # accept the current results as the baseline

Repositories are cloned into eval/.work (or $GITNEXT_EVAL_WORK) and set up once
(`setup` in repos.json). Results: eval/results/latest.json + latest.md.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORK = Path(os.environ.get("GITNEXT_EVAL_WORK", HERE / ".work"))
GITNEXT = os.environ.get("GITNEXT", shutil.which("gitnext") or str(HERE.parent / "target/release/gitnext"))
TOL = 0.05  # allowed drop of a rate before --check fails


def sh(cmd: str, cwd: Path, timeout: int = 900, env: dict | None = None) -> tuple[int, str]:
    try:
        p = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                           env={**os.environ, **(env or {})}, errors="replace")
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired as e:
        return 124, (e.stdout or "") if isinstance(e.stdout, str) else "timeout"


def gx(args: list[str], cwd: Path, timeout: int = 600) -> tuple[int, str]:
    p = subprocess.run([GITNEXT, *args], cwd=cwd, capture_output=True, text=True, timeout=timeout, errors="replace")
    return p.returncode, p.stdout + p.stderr


def tokens(s: str) -> int:
    return (len(s) + 3) // 4


# ---------------------------------------------------------------- languages
# compile errors → (file, line); test output → failing test names

def go_errors(out: str) -> list[tuple[str, int]]:
    return [(m.group(1).lstrip("./"), int(m.group(2))) for m in re.finditer(r"^\s*([\w./-]+\.go):(\d+):\d+:", out, re.M)]


def ts_errors(out: str) -> list[tuple[str, int]]:
    return [(m.group(1), int(m.group(2))) for m in re.finditer(r"^([\w./@-]+\.(?:ts|tsx|js|jsx|mts|cts))\((\d+),\d+\): error", out, re.M)]


def java_errors(out: str, root: Path) -> list[tuple[str, int]]:
    res = []
    for m in re.finditer(r"\[ERROR\] (/[^:\[]+\.java):\[(\d+),\d+\]", out):
        try:
            res.append((str(Path(m.group(1)).relative_to(root)), int(m.group(2))))
        except ValueError:
            pass
    return res


def rust_errors(out: str) -> list[tuple[str, int]]:
    return [(m.group(1), int(m.group(2))) for m in re.finditer(r"^\s*-->\s+([\w./-]+\.rs):(\d+):\d+", out, re.M)]


def go_failed(out: str) -> set[str]:
    names = set()
    for line in out.splitlines():
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        if ev.get("Action") == "fail" and ev.get("Test"):
            names.add(ev["Test"].split("/")[0])
    return names


def py_failed(out: str) -> set[str]:
    return {m.group(1).split("[")[0].rsplit("::", 1)[-1] for m in re.finditer(r"^(?:FAILED|ERROR) [^\s]+::([^\s]+)", out, re.M)}


def rust_failed(out: str) -> set[str]:
    return {m.group(1).rsplit("::", 1)[-1] for m in re.finditer(r"^test (\S+) \.\.\. FAILED", out, re.M)}


def java_failed(out: str) -> set[str]:
    names = set()
    for m in re.finditer(r"\[ERROR\]\s+(\w+)\.(\w+)(?::\d+)?\s", out):
        names.add(m.group(2))
    for m in re.finditer(r"^\s*(\w+)\(([\w.]+)\)\s", out, re.M):  # junit4 summary lines
        names.add(m.group(1))
    return names


def vitest_failed_files(json_path: Path, root: Path) -> set[str]:
    try:
        d = json.loads(json_path.read_text())
    except (OSError, ValueError):
        return set()
    out = set()
    for f in d.get("testResults", []):
        if f.get("status") == "failed":
            try:
                out.add(str(Path(f["name"]).relative_to(root)))
            except ValueError:
                out.add(f["name"])
    return out


LANG = {
    "go": dict(
        compile="go test -count=1 -run '^$' -gcflags=-e ./... 2>&1",
        errors=lambda out, root: go_errors(out),
        tests="go test -count=1 -json ./... 2>&1",
        failed=lambda out, root: go_failed(out),
        fail_stmt='panic("gitnext-mutant")',
        level="name",
    ),
    "python": dict(
        compile=".venv/bin/python -m compileall -q . >/dev/null && .venv/bin/python -m pytest -q --co -p no:cacheprovider 2>&1 | tail -3",
        errors=None,  # no compiler to name every use
        tests=".venv/bin/python -m pytest -q -rfE -p no:cacheprovider -x --maxfail=10000 2>&1 | tail -400",
        failed=lambda out, root: py_failed(out),
        fail_stmt='raise RuntimeError("gitnext-mutant")',
        level="name",
    ),
    "typescript": dict(
        compile="npx tsc --noEmit -p . --pretty false 2>&1",
        errors=lambda out, root: ts_errors(out),
        tests="npx vitest run --reporter=json --outputFile=.gnx-vitest.json >/dev/null 2>&1; echo done",
        failed=lambda out, root: vitest_failed_files(root / ".gnx-vitest.json", root),
        fail_stmt='throw new Error("gitnext-mutant");',
        level="file",
    ),
    "java": dict(
        compile="mvn -q -B -o test-compile 2>&1",
        errors=lambda out, root: java_errors(out, root),
        tests="mvn -B -o test -Dsurefire.printSummary=true -DtrimStackTrace=true 2>&1 | grep -E 'ERROR|FAIL|Tests run' | head -400",
        failed=lambda out, root: java_failed(out),
        fail_stmt='if (true) throw new RuntimeException("gitnext-mutant");',
        level="name",
    ),
    "rust": dict(
        compile="cargo check --tests --message-format human 2>&1",
        errors=lambda out, root: rust_errors(out),
        tests="cargo test --no-fail-fast 2>&1",
        failed=lambda out, root: rust_failed(out),
        fail_stmt='panic!("gitnext-mutant");',
        level="name",
    ),
}


# ---------------------------------------------------------------- repo plumbing

def prepare(repo: dict) -> Path:
    root = WORK / repo["name"]
    if not root.exists():
        WORK.mkdir(parents=True, exist_ok=True)
        code, out = sh(f"git clone -q {repo['url']} {root}", WORK, timeout=1800)
        if code:
            raise RuntimeError(f"clone {repo['url']}: {out[-400:]}")
    sh(f"git fetch -q --depth 1 origin {repo['commit']} 2>/dev/null; git checkout -q -f {repo['commit']}", root)
    restore(root)
    marker = root / ".gnx-setup-done"
    if repo.get("setup") and not marker.exists():
        code, out = sh(repo["setup"], root, timeout=3600)
        if code:
            raise RuntimeError(f"setup {repo['name']}: {out[-600:]}")
        marker.write_text("ok")
    return root


def restore(root: Path) -> None:
    sh("git checkout -q -- . && git clean -qfd -e .venv -e node_modules -e target -e .gitnext -e .gnx-setup-done -e .gnx-vitest.json", root)


def symbols(root: Path) -> list[dict]:
    code, out = gx(["--json", "symbols"], root)
    return json.loads(out) if code == 0 else []


def callers(root: Path, q: str) -> tuple[set[str], set[str]]:
    code, out = gx(["--json", "callers", q], root)
    if code:
        return set(), set()
    d = json.loads(out)
    sure, maybe = set(), set()
    for entry in d if isinstance(d, list) else [d]:
        for c, _line in entry.get("callers", []):
            sure.add(c["qualified"])
        for c in entry.get("maybe", []) or []:
            maybe.add((c[0] if isinstance(c, list) else c)["qualified"])
    return sure, maybe


def tests_for(root: Path, q: str) -> tuple[set[str], set[str], set[str]]:
    """(test names, test files, names found through the call graph only)"""
    code, out = gx(["tests-for", q], root)
    names, files, graph = set(), set(), set()
    for m in re.finditer(r"^(\S+)\s+(\S+?):\d+ \(([^)]*)\)", out, re.M):
        n = m.group(1).rsplit("::", 1)[-1]
        names.add(n)
        files.add(m.group(2))
        if "hop" in m.group(3) or "direct" in m.group(3):
            graph.add(n)
    return names, files, graph


def enclosing(syms: list[dict], file: str, line: int) -> str | None:
    best = None
    for s in syms:
        if s["file"] == file and s["kind"] != "module" and s["start_line"] <= line <= s["end_line"]:
            if best is None or (s["end_line"] - s["start_line"]) < (best["end_line"] - best["start_line"]):
                best = s
    if best is None:
        mods = [s for s in syms if s["file"] == file and s["kind"] == "module"]
        return mods[0]["qualified"] if mods else None
    return best["qualified"]


def pick(syms: list[dict], root: Path, n: int, seed: int) -> list[dict]:
    names: dict[str, int] = {}
    for s in syms:
        names[s["name"]] = names.get(s["name"], 0) + 1
    cands = [
        s for s in syms
        if s["kind"] in ("function", "method") and not s.get("test") and not s.get("decl") and s.get("visibility")
        and names[s["name"]] == 1 and len(s["name"]) >= 4 and s["end_line"] - s["start_line"] >= 2
        and not s["name"].startswith(("_", "Test", "test", "__")) and s["name"] not in ("main", "init", "constructor", "toString", "equals", "hashCode", "fmt", "new", "default")
        and s["name"] != s.get("parent", "").rsplit("::", 1)[-1]  # constructors
    ]
    rng = random.Random(seed)
    rng.shuffle(cands)
    out = []
    for s in cands:
        sure, maybe = callers(root, s["qualified"])
        if sure:
            s["_callers"], s["_maybe"] = sure, maybe
            out.append(s)
        if len(out) >= n:
            break
    return out


def name_col(text_line: str, name: str) -> int:
    m = re.search(rf"(?<![\w$]){re.escape(name)}(?![\w$])", text_line)
    return m.start() if m else -1


def edit_line(root: Path, file: str, line: int, fn) -> bool:
    p = root / file
    lines = p.read_text(errors="replace").split("\n")
    new = fn(lines[line - 1])
    if new is None or new == lines[line - 1]:
        return False
    lines[line - 1] = new
    p.write_text("\n".join(lines))
    return True


def body_start(root: Path, s: dict, lang: str) -> int | None:
    """Line after the opening `{` (or `:` in Python) of a function."""
    lines = (root / s["file"]).read_text(errors="replace").split("\n")
    for i in range(s["start_line"] - 1, min(s["end_line"], len(lines))):
        t = lines[i].split("//")[0].rstrip()
        if lang == "python":
            if t.endswith(":") and not t.lstrip().startswith(("@", "#")):
                return i + 2
        elif t.endswith("{"):
            return i + 2
    return None


def indent_of(root: Path, file: str, line: int) -> str:
    lines = (root / file).read_text(errors="replace").split("\n")
    for l in lines[line - 1: line + 5]:
        if l.strip():
            return l[: len(l) - len(l.lstrip())]
    return "    "


# ---------------------------------------------------------------- checks

def check_callers(root: Path, repo: dict, syms: list[dict], s: dict) -> dict | None:
    L = LANG[repo["lang"]]
    if not L["errors"]:
        return None
    ok = edit_line(root, s["file"], s["start_line"] + _def_offset(root, s), lambda l: _rename_token(l, s["name"], s["name"] + "Gnxbroken"))
    if not ok:
        return None
    _, out = sh(L["compile"], root, timeout=1200)
    restore(root)
    sites = L["errors"](out, root)
    modules = {x["qualified"] for x in syms if x["kind"] == "module"}
    file_of = {x["qualified"]: x["file"] for x in syms}
    hits = {(f, q) for f, ln in sites if (q := enclosing(syms, f, ln)) and q != s["qualified"]}
    # an error on an import line (Java static import, TS re-export) is not a call, and javac then
    # stops reporting the uses in that file: callers found there are not counted as wrong
    import_files = {f for f, q in hits if q in modules}
    truth = {q for f, q in hits if q not in modules}
    found = s["_callers"]
    if not truth and not import_files:
        return {"symbol": s["qualified"], "skipped": "compiler reported no uses"}
    ok_found = {c for c in found if c in truth or file_of.get(c) in import_files}
    return {
        "symbol": s["qualified"], "truth": sorted(truth), "found": sorted(found), "maybe": sorted(s["_maybe"]), "import_files": sorted(import_files),
        "recall": len(found & truth) / len(truth) if truth else None, "precision": len(ok_found) / len(found) if found else 1.0,
        "recall_with_maybe": len((found | s["_maybe"]) & truth) / len(truth) if truth else None,
    }


def _def_offset(root: Path, s: dict) -> int:
    """Lines between the symbol's start (decorators, annotations, doc) and the line with its name."""
    lines = (root / s["file"]).read_text(errors="replace").split("\n")
    for i in range(s["start_line"] - 1, min(s["end_line"], len(lines))):
        if name_col(lines[i], s["name"]) >= 0 and not lines[i].lstrip().startswith(("@", "//", "#", "*", "/*")):
            return i - (s["start_line"] - 1)
    return 0


def _rename_token(line: str, old: str, new: str) -> str | None:
    c = name_col(line, old)
    return None if c < 0 else line[:c] + new + line[c + len(old):]


def check_rename(root: Path, repo: dict, s: dict, run_tests: bool, baseline_failed: set[str] = frozenset()) -> dict:
    L = LANG[repo["lang"]]
    new = s["name"] + ("_gnx" if "_" in s["name"] or s["name"].islower() else "Gnx")
    code, out = gx(["rename", s["qualified"], new, "--text"], root)
    if code:
        restore(root)
        return {"symbol": s["qualified"], "ok": False, "stage": "rename", "detail": out[-300:]}
    gx(["index"], root)
    left = sh(f"git grep -nw {s['name']} -- . ':!*.md' | head -5", root)[1].strip()
    ccode, cout = sh(L["compile"], root, timeout=1500)
    compiled = ccode == 0 and "error" not in cout.lower()[-400:] if repo["lang"] != "python" else ccode == 0
    tests_ok, failed = None, set()
    if compiled and run_tests:
        tcode, tout = sh(L["tests"], root, timeout=1800)
        failed = L["failed"](tout, root) - baseline_failed
        tests_ok = not failed
    restore(root)
    gx(["index"], root)
    return {"symbol": s["qualified"], "ok": compiled and tests_ok is not False, "compiled": compiled, "tests_ok": tests_ok,
            "tests_failed": sorted(failed)[:8] if tests_ok is False else [],
            "old_name_left": left[:300], "detail": "" if compiled else cout[-500:]}


def check_tests(root: Path, repo: dict, s: dict, baseline_failed: set[str]) -> dict | None:
    L = LANG[repo["lang"]]
    at = body_start(root, s, repo["lang"])
    if not at:
        return None
    ind = indent_of(root, s["file"], at)
    p = root / s["file"]
    lines = p.read_text(errors="replace").split("\n")
    lines.insert(at - 1, ind + L["fail_stmt"])
    p.write_text("\n".join(lines))
    ccode, cout = sh(L["compile"], root, timeout=1500)
    if ccode and repo["lang"] != "python":
        restore(root)
        return {"symbol": s["qualified"], "skipped": "mutant does not compile"}
    _, tout = sh(L["tests"], root, timeout=2400)
    truth = L["failed"](tout, root) - baseline_failed
    restore(root)
    names, files, graph = tests_for(root, s["qualified"])
    pred = files if L["level"] == "file" else names
    if not truth:
        return {"symbol": s["qualified"], "skipped": "no test fails when it throws", "predicted": len(pred)}
    tp = len(pred & truth)
    out = {"symbol": s["qualified"], "level": L["level"], "truth": len(truth), "predicted": len(pred),
           "recall": tp / len(truth), "precision": tp / len(pred) if pred else 0.0,
           "missed": sorted(truth - pred)[:8]}
    if L["level"] == "name":
        out["recall_graph_only"] = len(graph & truth) / len(truth)
    return out


def look_cost(root: Path, s: dict) -> dict:
    _, out = gx(["look", s["qualified"]], root)
    files = {s["file"]} | {c.split("::")[0] for c in []}
    naive = sum(tokens((root / f).read_text(errors="replace")) for f in files if (root / f).exists())
    return {"symbol": s["qualified"], "look_tokens": tokens(out), "file_tokens": naive}


def run_repo(repo: dict, n: int, mutants: int, seed: int, tests_in_rename: bool) -> dict:
    t0 = time.time()
    root = prepare(repo)
    L = LANG[repo["lang"]]
    ti = time.time()
    code, out = gx(["init"], root, timeout=1800)
    index_s = time.time() - ti
    syms = symbols(root)
    res: dict = {"repo": repo["name"], "lang": repo["lang"], "commit": repo["commit"][:12], "index_seconds": round(index_s, 1),
                 "symbols": sum(1 for s in syms if s["kind"] != "module"), "files": len({s["file"] for s in syms})}
    # how many call edges resolve inside the repo
    m = re.search(r"(\d+) symbols · (\d+) edges", gx(["index"], root)[1])
    res["edges"] = int(m.group(2)) if m else None
    sample = pick(syms, root, n, seed)
    res["sample"] = [s["qualified"] for s in sample]
    # baseline: tests already failing before any change are not ours
    _, tout = sh(L["tests"], root, timeout=2400)
    baseline_failed = L["failed"](tout, root)
    res["baseline_failing_tests"] = sorted(baseline_failed)[:20]
    res["callers"] = [x for s in sample if (x := check_callers(root, repo, syms, s))]
    res["rename"] = [check_rename(root, repo, s, tests_in_rename, baseline_failed) for s in sample]
    res["tests"] = [x for s in sample[:mutants] if (x := check_tests(root, repo, s, baseline_failed))]
    res["look"] = [look_cost(root, s) for s in sample]
    res["seconds"] = round(time.time() - t0)
    res["summary"] = summarize(res)
    return res


def mean(v: list[float]) -> float | None:
    return round(sum(v) / len(v), 3) if v else None


def summarize(r: dict) -> dict:
    cal = [c for c in r["callers"] if c.get("recall") is not None]
    tst = [t for t in r["tests"] if "recall" in t]
    looks = r["look"]
    return {
        "callers_recall": mean([c["recall"] for c in cal]),
        "callers_precision": mean([c["precision"] for c in cal]),
        "callers_checked": len(cal),
        "rename_ok": mean([1.0 if x["ok"] else 0.0 for x in r["rename"]]),
        "rename_checked": len(r["rename"]),
        "tests_recall": mean([t["recall"] for t in tst]),
        "tests_precision": mean([t["precision"] for t in tst]),
        "tests_checked": len(tst),
        "look_vs_file": mean([l["look_tokens"] / max(1, l["file_tokens"]) for l in looks]),
    }


def render_md(results: list[dict]) -> str:
    rows = ["| repo | lang | symbols | index s | callers recall | callers precision | rename ok | tests recall | tests precision | look / file |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    fmt = lambda v, n=None: "—" if v is None else (f"{v:.0%}" + (f" ({n})" if n is not None else ""))
    for r in results:
        s = r.get("summary") or {}
        if "error" in r:
            rows.append(f"| {r['repo']} | {r.get('lang','')} | error: {r['error'][:60]} |||||||")
            continue
        rows.append(f"| {r['repo']} | {r['lang']} | {r['symbols']} | {r['index_seconds']} | {fmt(s['callers_recall'], s['callers_checked'])} | "
                    f"{fmt(s['callers_precision'])} | {fmt(s['rename_ok'], s['rename_checked'])} | {fmt(s['tests_recall'], s['tests_checked'])} | "
                    f"{fmt(s['tests_precision'])} | {fmt(s['look_vs_file'])} |")
    return "\n".join(rows) + "\n"


def compare(results: list[dict], baseline: dict) -> list[str]:
    bad = []
    for r in results:
        b = baseline.get(r["repo"], {})
        for k, v in (r.get("summary") or {}).items():
            if k.endswith(("recall", "precision", "_ok")) and v is not None and b.get(k) is not None and v < b[k] - TOL:
                bad.append(f"{r['repo']}: {k} {b[k]:.0%} → {v:.0%}")
    return bad


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default="", help="comma-separated names (default: all)")
    ap.add_argument("--n", type=int, default=8, help="functions sampled per repo")
    ap.add_argument("--mutants", type=int, default=4, help="of those, how many get the test-suite check")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--fast", action="store_true", help="after a rename only compile, skip the test suite (a rename that edits a\n"
                    "string the tests compare against compiles fine and is still wrong)")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--bless", action="store_true")
    args = ap.parse_args()
    repos = json.loads((HERE / "repos.json").read_text())
    if args.repos:
        want = set(args.repos.split(","))
        repos = [r for r in repos if r["name"] in want]
    results = []
    for repo in repos:
        print(f"== {repo['name']} ({repo['lang']})", flush=True)
        try:
            r = run_repo(repo, args.n, args.mutants, args.seed, not args.fast)
        except Exception as e:  # one broken repo must not hide the others
            r = {"repo": repo["name"], "lang": repo["lang"], "error": str(e)}
        results.append(r)
        print(json.dumps(r.get("summary") or r, ensure_ascii=False), flush=True)
    out = HERE / "results"
    out.mkdir(exist_ok=True)
    (out / "latest.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
    md = render_md(results)
    (out / "latest.md").write_text(md)
    print("\n" + md)
    base_path = HERE / "baseline.json"
    if args.bless:
        base_path.write_text(json.dumps({r["repo"]: r["summary"] for r in results if r.get("summary")}, indent=1))
        print("baseline updated")
    if args.check and base_path.exists():
        bad = compare(results, json.loads(base_path.read_text()))
        if bad:
            print("REGRESSIONS:\n  " + "\n  ".join(bad))
            sys.exit(1)
        print("no regressions")


if __name__ == "__main__":
    main()
