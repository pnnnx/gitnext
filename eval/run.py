#!/usr/bin/env python3
"""gitnext eval: check gitnext's answers against the compilers and test suites
of real, popular repositories pinned to exact commits.

Nothing here trusts gitnext or a human judgement. Each check gets its ground
truth from the project itself:

  callers   rename a function's *definition* only and compile: every error the
            compiler reports is a real use of it; fix those uses and compile again
            until it builds (javac and rustc stop at the first failing class /
            crate, one pass misses the rest). Compared with `gitnext callers`
            → recall (uses gitnext found) and precision (callers that are real).
            Checked twice: for functions with a unique name, and for methods
            whose name several types share (`common` — only the receiver's type
            tells them apart).
            And once more for functions picked without asking gitnext (`blind`):
            the two samples above hold only functions it finds a caller for, so
            they cannot show a function whose callers it misses entirely.
  rename    `gitnext rename <f> <f>Gnx --text`, then the project must still
            compile and its tests must still pass.
  tests     make the function fail on entry and run the whole test suite:
            the tests that fail are the ones that exercise it. A test is its
            name, in Python `file::name`, in TypeScript its file. Compared with
            `gitnext tests-for --all` → recall / precision, and with the 10 tests
            it puts first → how many of those fail (top-10 precision) and whether
            any does (caught).
  index     time, size, share of calls resolved inside the repo.
  look      tokens of one `gitnext look` vs reading the files it covers.

    python3 eval/run.py                      # all repos in eval/repos.json: 20 functions, 8 mutants, seeds 7,8,9
    python3 eval/run.py --repos cobra,click --n 6 --mutants 3 --seed 7
    python3 eval/run.py --check              # compare with eval/baseline.json, exit 1 on regression
    python3 eval/run.py --bless              # accept the current results as the baseline of this sample

Which difference is real. Every number is a mean over the functions that the seeds
picked (a function picked twice counts once). Next to it stands "±": half the 90%
interval the mean stays in when the functions are drawn again (bootstrap), and at
least what one function weighs. A change smaller than that is noise; `--check`
uses the same thresholds. latest.md also shows each number on every seed's own sample.

Repositories are cloned into eval/.work (or $GITNEXT_EVAL_WORK) and set up once
(`setup` in repos.json). What the compilers and test suites said is kept in
eval/.work/truth.json: per repo commit and function, and for a rename per distinct
edit, so a second run only asks gitnext again (`--no-cache` asks everything again).
Results: eval/results/latest.json + latest.md + latest.noise.json.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import math
import os
import random
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORK = Path(os.environ.get("GITNEXT_EVAL_WORK", HERE / ".work"))
GITNEXT = os.environ.get("GITNEXT", shutil.which("gitnext") or str(HERE.parent / "target/release/gitnext"))
TOL = 0.05  # allowed drop of a rate before --check fails, where the baseline has no threshold of its own
RATES = ("recall", "precision", "_ok", "caught", "with_maybe")  # the summary keys --check watches
CACHE_VERSION = 2  # bump when what truth.json stores changes its meaning: CI keys its cache on this number
MAX_TRIES = 2  # mutants: at most MAX_TRIES × --mutants functions of a sample are tried
SAMPLE_FROM: dict | None = None  # --sample-from: repo → the earlier run whose functions are measured again
BOOT = 2000  # bootstrap resamples behind every "±"


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


CACHE_FILE = WORK / "truth.json"
CACHE: dict = {}
USE_CACHE = True


CACHE_STATS = {"cached": 0, "asked": 0}


def _load_cache() -> None:
    try:
        d = json.loads(CACHE_FILE.read_text())
    except (OSError, ValueError):
        return
    secs = {**d.pop("_seconds", {}), **CACHE.get("_seconds", {})}
    CACHE.update(d)
    CACHE["_seconds"] = secs


def cached(key: str, make, keep=lambda v: True):
    """Ground truth of a pinned commit does not change: ask the compiler / tests once.
    An answer that fails `keep` (a suite that did not start) is not stored. The file is read again
    under a lock before every write, so two runs (`--repos a` and `--repos b`) can share it.
    `_seconds` in the file: how long each answer took — what a cold run costs."""
    if not CACHE:
        _load_cache()
    if USE_CACHE and key in CACHE:
        CACHE_STATS["cached"] += 1
        return CACHE[key]
    CACHE_STATS["asked"] += 1
    t0 = time.time()
    val = make()
    if not keep(val):
        return val
    CACHE_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(CACHE_FILE.with_suffix(".lock"), "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        _load_cache()
        CACHE[key] = val
        CACHE.setdefault("_seconds", {})[key] = round(time.time() - t0, 1)
        tmp = CACHE_FILE.with_suffix(".tmp")
        tmp.write_text(json.dumps(CACHE, ensure_ascii=False))
        os.replace(tmp, CACHE_FILE)
    return val


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


def go_failing_alone(root: Path) -> set[str]:
    """Go tests that fail, each run on its own. A panic kills the whole test binary, so one
    `go test` run names only the first test that reached the broken function.
    The binary keeps the name `go test` gives it (`cobra.test`): a project may look at it — cobra
    reads the command line as its own arguments under any other name, and seven tests fail."""
    failed = set()
    _, dirs = sh("go list -f '{{.Dir}}' ./...", root)
    for d in [Path(x) for x in dirs.split() if x.startswith("/")]:
        out = d / ".gnx-bin"
        sh(f"rm -rf {out}; mkdir {out} && go test -c -o {out}/ .", d, timeout=900)
        for binary in out.glob("*.test"):  # none: no tests in this package
            _, names = sh(f"{binary} -test.list '^(Test|Example|Fuzz)'", d)
            for n in [x for x in names.split() if re.match(r"(Test|Example|Fuzz)\w*$", x)]:
                code, _ = sh(f"{binary} -test.run '^{n}$' -test.count=1", d, timeout=300)
                if code:
                    failed.add(n)
        shutil.rmtree(out, ignore_errors=True)
    return failed


def py_failed(out: str) -> set[str]:
    # `FAILED tests/a.py::Class::test_x[a::b c] - message` → test_x: the id of a parametrized case may hold anything
    return {m.group(1).rsplit("::", 1)[-1] for m in re.finditer(r"^(?:FAILED|ERROR) ([^\s\[]+::[^\s\[]+)", out, re.M)}


PY_TESTS: dict[str, dict[str, list[str]]] = {}  # repo root → test file → its tests (py_collect), filled by run_repo


def py_collect(root: Path) -> dict[str, list[str]]:
    """Every test of the untouched commit, by file, as `file::test`."""
    _, out = sh(".venv/bin/python -m pytest --co -q -p no:cacheprovider 2>&1", root, timeout=1200)
    by_file: dict[str, set[str]] = {}
    for m in re.finditer(r"^([^\s:\[]+\.py)::([^\s\[]+)", out, re.M):
        by_file.setdefault(m.group(1), set()).add(f"{m.group(1)}::{m.group(2).rsplit('::', 1)[-1]}")
    return {f: sorted(v) for f, v in by_file.items()}


def py_failed_ids(out: str, root: Path) -> set[str]:
    """Failing tests as `file::test`: two files may hold a test of the same name, a bare name would merge
    them. A test module that could not be imported (`ERROR tests/a.py - ImportError`) fails all its tests."""
    ids: set[str] = set()
    for m in re.finditer(r"^(?:FAILED|ERROR) ([^\s:\[]+\.py)(?:::([^\s\[]+))?", out, re.M):
        if m.group(2):
            ids.add(f"{m.group(1)}::{m.group(2).rsplit('::', 1)[-1]}")
        else:
            ids.update(PY_TESTS.get(str(root), {}).get(m.group(1), []))
    return ids


def rust_failed(out: str) -> set[str]:
    return {m.group(1).rsplit("::", 1)[-1] for m in re.finditer(r"^test (\S+) \.\.\. FAILED", out, re.M)}


def java_failed(out: str) -> set[str]:
    names = set()
    # surefire 3: `[ERROR] org.x.FooTest.parses(boolean)[2] -- Time elapsed: 0.01 s <<< ERROR!`
    for m in re.finditer(r"^\[ERROR\]\s+[\w.$]+\.(\w+)(?:\([^)]*\))?(?:\[\d+\])?\s+--\s+Time elapsed.*<<< (?:ERROR|FAILURE)!", out, re.M):
        names.add(m.group(1))
    for m in re.finditer(r"\[ERROR\]\s+(\w+)\.(\w+)(?::\d+)?\s", out):  # the summary: `FooTest.parses:22 » Runtime`
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
        ran=lambda out, root: '"Action":"pass"' in out or '"Action":"fail"' in out,
        failed=lambda out, root: go_failed(out),
        fail_stmt='panic("gitnext-mutant")',
        failing_alone=go_failing_alone,
        tests_key="tests2",  # under "tests": answers taken with a renamed test binary (see go_failing_alone)
        # `./...` skips directories that start with `_` or `.` and `testdata` (chi's `_examples`): the compiler
        # never sees a caller there, so it can be neither confirmed nor refuted
        unseen=lambda file: any(d.startswith(("_", ".")) or d == "testdata" for d in file.split("/")[:-1]),
        level="name",
    ),
    "python": dict(
        compile=".venv/bin/python -m compileall -q . >/dev/null && .venv/bin/python -m pytest -q --co -p no:cacheprovider 2>&1 | tail -3",
        errors=None,  # no compiler to name every use
        # the whole summary, no tracebacks: `| tail -400` cut the list wherever hundreds of tests failed
        # and all of the suite: without --continue-on-collection-errors a test module that no longer imports stops the run
        # before any test ("Interrupted: 1 error during collection") — it looked like "no test fails"
        tests=".venv/bin/python -m pytest -q -rfE --tb=no -p no:cacheprovider -x --maxfail=10000 --continue-on-collection-errors 2>&1",
        tests_key="tests4",  # "tests": cut at 400 lines; "tests2": names from inside parametrized ids; "tests3": stopped at a collection error
        collect=py_collect,
        ids=py_failed_ids,
        ran=lambda out, root: bool(re.search(r"\d+ (passed|failed|error)", out)),
        failed=lambda out, root: py_failed(out),
        fail_stmt='raise RuntimeError("gitnext-mutant")',
        level="name",
    ),
    "typescript": dict(
        compile="npx tsc --noEmit -p . --pretty false 2>&1",
        errors=lambda out, root: ts_errors(out),
        tests="rm -f .gnx-vitest.json; npx vitest run --reporter=json --outputFile=.gnx-vitest.json >/dev/null 2>&1; echo done",
        ran=lambda out, root: (root / ".gnx-vitest.json").exists(),
        failed=lambda out, root: vitest_failed_files(root / ".gnx-vitest.json", root),
        fail_stmt='if (Date.now() > 0) throw new Error("gitnext-mutant");',  # a bare `throw` makes the rest unreachable: tsc rejects it
        level="file",
    ),
    "java": dict(
        # every class again: maven recompiles only the changed files, the callers in other files would pass unchecked
        compile="rm -rf target/classes target/test-classes target/maven-status; mvn -q -B -o test-compile 2>&1",
        errors=lambda out, root: java_errors(out, root),
        tests="mvn -B -o test -Dsurefire.printSummary=true -DtrimStackTrace=true 2>&1 | grep -E '<<<|Tests run:|ERROR\\] +[A-Za-z]|BUILD'",
        ran=lambda out, root: bool(re.search(r"Tests run: [1-9]", out)),
        failed=lambda out, root: java_failed(out),
        fail_stmt='if (true) throw new RuntimeException("gitnext-mutant");',
        level="name",
    ),
    "rust": dict(
        # all features: code behind a feature (indexmap's rayon and serde modules) is code gitnext indexes too
        compile="cargo check --tests --all-features --message-format human 2>&1",
        uses_key="uses2",  # "uses": without the features
        unseen=lambda file: file.startswith("benches/"),  # `cargo check --benches` needs nightly where benches use #![feature(test)]
        errors=lambda out, root: rust_errors(out),
        tests="cargo test --no-fail-fast 2>&1",
        ran=lambda out, root: "test result:" in out,
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


def baseline_tests(root: Path, L: dict) -> dict:
    """The suite on the untouched commit: did it run, and which tests fail already (those are not ours).
    Asked the same way a mutant is: where its tests run one at a time, the ones failing alone count too."""
    _, tout = sh(L["tests"], root, timeout=2400)
    failed = L.get("ids", L["failed"])(tout, root)
    if "failing_alone" in L:
        failed = failed | L["failing_alone"](root)
    return {"ran": bool(L["ran"](tout, root)), "failed": sorted(failed), "tail": tout[-300:]}


def symbols(root: Path) -> list[dict]:
    code, out = gx(["--json", "symbols"], root)
    return json.loads(out) if code == 0 else []


CALLERS_MEMO: dict | None = None  # run_repo turns it on: the tree is clean there, and in a big repo one call takes seconds


def _callers_of(entry: dict) -> tuple[set[str], set[str]]:
    sure, maybe = set(), set()
    for c, _line in entry.get("callers", []):
        sure.add(c["qualified"])
    for c in entry.get("maybe", []) or []:
        maybe.add((c[0] if isinstance(c, list) else c)["qualified"])
    return sure, maybe


def callers_many(root: Path, qs: list[str]) -> dict[str, tuple[set[str], set[str]]]:
    """`gitnext callers` for several functions in one run: in a big repo every run costs a second or two.
    A gitnext that takes one name at a time, or does not find one of the names, is asked name by name."""
    memo = CALLERS_MEMO if CALLERS_MEMO is not None else {}

    def ask(names: list[str]) -> bool:
        code, out = gx(["--json", "callers", *names], root)
        try:
            d = json.loads(out) if not code else None
        except ValueError:
            d = None
        if d is None:
            return False
        got: dict[str, tuple[set[str], set[str]]] = {n: (set(), set()) for n in names}
        for entry in d if isinstance(d, list) else [d]:
            name = names[0] if len(names) == 1 else (entry.get("symbol") or {}).get("qualified")
            if name in got:
                sure, maybe = _callers_of(entry)
                got[name][0].update(sure)
                got[name][1].update(maybe)
        for n in names:
            memo[(str(root), n)] = got[n]
        return True

    new = [q for q in dict.fromkeys(qs) if (str(root), q) not in memo]
    if len(new) > 1:
        ask(new)
    for q in new:
        if (str(root), q) not in memo and not ask([q]):
            memo[(str(root), q)] = (set(), set())
    return {q: memo[(str(root), q)] for q in qs}


def callers(root: Path, q: str) -> tuple[set[str], set[str]]:
    return callers_many(root, [q])[q]


def tests_for_rows(root: Path, q: str) -> list[tuple[str, str, str]]:
    """What `gitnext tests-for --all` names, most likely first: (test name, test file, how it was found)."""
    code, out = gx(["tests-for", q, "--all"], root)
    if code:  # a gitnext without `--all` lists everything anyway
        code, out = gx(["tests-for", q], root)
    return [(m.group(1).rsplit("::", 1)[-1], m.group(2), m.group(3)) for m in re.finditer(r"^(\S+)\s+(\S+?):\d+ \(([^)]*)\)", out, re.M)]


def tests_for(root: Path, q: str) -> tuple[list[str], list[str], set[str]]:
    """(test names, test files — both most likely first, names found through the call graph only)"""
    names, files, graph = [], [], set()
    for n, f, how in tests_for_rows(root, q):
        if n not in names:
            names.append(n)
        if f not in files:
            files.append(f)
        if "hop" in how or "direct" in how:
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


def pick(syms: list[dict], root: Path, n: int, seed: int, common: bool = False, blind: bool = False) -> list[dict]:
    """Functions to check. Default: names that occur once in the repo, with a caller gitnext knows of.
    `common`: methods whose name several types share (`release`, `parse`…) — only the receiver's type
    tells them apart. `blind`: the first n of the same order whether gitnext sees a caller or not —
    a function whose callers it misses entirely never gets into the default sample."""
    names: dict[str, int] = {}
    for s in syms:
        if s["kind"] in ("function", "method") and not s.get("test"):
            names[s["name"]] = names.get(s["name"], 0) + 1
    all_names: dict[str, int] = {}
    owners: dict[str, list] = {}
    for s in syms:
        all_names[s["name"]] = all_names.get(s["name"], 0) + 1
        owners.setdefault(s["name"], []).append(s.get("parent"))
    overloaded = {n for n, ps in owners.items() if len(set(ps)) < len(ps)}  # same name twice in one type
    cands = [
        s for s in syms
        if s["kind"] in ("function", "method") and not s.get("test") and not s.get("decl") and s.get("visibility")
        and len(s["name"]) >= 4 and s["end_line"] - s["start_line"] >= 2
        and not s["name"].startswith(("_", "Test", "test", "__")) and s["name"] not in ("main", "init", "constructor", "toString", "equals", "hashCode", "fmt", "new", "default")
        and s["name"] != s.get("parent", "").rsplit("::", 1)[-1]  # constructors
        and ((s["kind"] == "method" and 2 <= names[s["name"]] <= 6 and s["name"] not in overloaded) if common else all_names[s["name"]] == 1)
    ]
    rng = random.Random(seed)
    rng.shuffle(cands)
    out = []
    used = set()
    for at in range(0, len(cands), 40):  # gitnext is asked about 40 functions per run
        chunk = cands[at:at + 40]
        found = callers_many(root, [s["qualified"] for s in chunk])
        for s in chunk:
            if s["name"] in used:  # one method per shared name
                continue
            sure, maybe = found[s["qualified"]]
            if blind or sure or (common and maybe):
                s["_callers"], s["_maybe"] = sure, maybe
                out.append(s)
                used.add(s["name"])
            if len(out) >= n:
                return out
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

def use_sites(root: Path, repo: dict, s: dict) -> list | None:
    """Every (file, line) that uses `s`, named by the compiler: rename the definition only, compile,
    rename the uses it complains about, compile again — until nothing new comes up. One pass is not
    enough: javac stops at the first class with errors, rustc at the first crate."""
    L = LANG[repo["lang"]]
    old, new = s["name"], s["name"] + "Gnxbroken"
    if not edit_line(root, s["file"], s["start_line"] + _def_offset(root, s), lambda l: _rename_token(l, old, new)):
        return None
    sites: list[tuple[str, int]] = []
    for _ in range(12):
        _, out = sh(L["compile"], root, timeout=1200)
        progress = False
        for f, ln in dict.fromkeys(L["errors"](out, root)):
            try:
                fixed = edit_line(root, f, ln, lambda l: re.sub(rf"(?<![\w$]){re.escape(old)}(?![\w$])", new, l))
            except (OSError, IndexError):
                continue
            if fixed:  # the line names the function: a use of it
                progress = True
                if (f, ln) not in sites:
                    sites.append((f, ln))
        if not progress:
            break
    restore(root)
    return [list(x) for x in sites]


def check_callers(root: Path, repo: dict, syms: list[dict], s: dict) -> dict | None:
    L = LANG[repo["lang"]]
    if not L["errors"]:
        return None
    sites = cached(f"{repo['name']}@{repo['commit'][:12]}:{L.get('uses_key', 'uses')}:{s['qualified']}", lambda: use_sites(root, repo, s))
    if sites is None:
        return None
    modules = {x["qualified"] for x in syms if x["kind"] == "module"}
    file_of = {x["qualified"]: x["file"] for x in syms}
    hits = {(f, q) for f, ln in sites if (q := enclosing(syms, f, ln)) and q != s["qualified"]}
    # a use on an import line (Java static import, TS re-export) is not a call
    import_files = {f for f, q in hits if q in modules}
    truth = {q for f, q in hits if q not in modules}
    # code the compile command does not build (Rust benches need nightly) can be neither confirmed nor refuted
    seen = lambda c: not L.get("unseen", lambda f: False)(file_of.get(c, ""))
    found = {c for c in s["_callers"] if seen(c)}
    maybe = {c for c in s["_maybe"] if seen(c)}
    if not truth and not import_files:
        return {"symbol": s["qualified"], "skipped": "compiler reported no uses"}
    ok_found = {c for c in found if c in truth or file_of.get(c) in import_files}
    return {
        "symbol": s["qualified"], "truth": sorted(truth), "found": sorted(found), "maybe": sorted(maybe), "import_files": sorted(import_files),
        "recall": len(found & truth) / len(truth) if truth else None, "precision": len(ok_found) / len(found) if found else None,
        "recall_with_maybe": len((found | maybe) & truth) / len(truth) if truth else None,
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

    def verdict() -> dict:
        ccode, cout = sh(L["compile"], root, timeout=1500)
        compiled = ccode == 0 and "error" not in cout.lower()[-400:] if repo["lang"] != "python" else ccode == 0
        v = {"compiled": compiled, "ran": None, "failed": [], "detail": "" if compiled else cout[-500:]}
        if compiled and run_tests:
            tcode, tout = sh(L["tests"], root, timeout=1800)
            v["ran"], v["failed"] = bool(L["ran"](tout, root)), sorted(L.get("ids", L["failed"])(tout, root))
        return v

    # the same edit gets the same verdict from the same commands: the build and the tests are asked once per
    # distinct diff, so a second run costs only gitnext's time, and a changed gitnext pays only for what it edits differently
    diff = sh("git diff --no-color", root)[1]
    how = hashlib.sha1((L["compile"] + (L["tests"] if run_tests else "")).encode()).hexdigest()[:8]
    key = f"{repo['name']}@{repo['commit'][:12]}:rename:{how}:{hashlib.sha1(diff.encode()).hexdigest()}"
    v = cached(key, verdict, keep=lambda v: v["ran"] is not False)  # a suite that did not start: ask again next time
    failed = set(v["failed"]) - baseline_failed
    tests_ok = None if v["ran"] is None else (not failed and v["ran"])  # a suite that did not start has no failures either
    restore(root)
    gx(["index"], root)
    return {"symbol": s["qualified"], "ok": v["compiled"] and tests_ok is not False, "compiled": v["compiled"], "tests_ok": tests_ok,
            "tests_failed": sorted(failed)[:8] if tests_ok is False else [],
            "old_name_left": left[:300], "detail": v["detail"] or ("the test suite did not run" if v["ran"] is False else "")}


def failing_tests(root: Path, repo: dict, s: dict, baseline_failed: set[str]) -> dict:
    """Tests that fail when `s` throws on entry (or why that could not be measured): `failed` — their
    names; `ids`, where the language has them (Python) — the same tests as `file::test`."""
    L = LANG[repo["lang"]]
    at = body_start(root, s, repo["lang"])
    if not at:
        return {"skipped": "no body"}
    ind = indent_of(root, s["file"], at)
    p = root / s["file"]
    lines = p.read_text(errors="replace").split("\n")
    lines.insert(at - 1, ind + L["fail_stmt"])
    p.write_text("\n".join(lines))
    ccode, cout = sh(L["compile"], root, timeout=1500)
    if ccode and repo["lang"] != "python":
        restore(root)
        return {"skipped": "mutant does not compile"}
    ids = None
    if "failing_alone" in L:
        truth = L["failing_alone"](root) - baseline_failed
    else:
        _, tout = sh(L["tests"], root, timeout=2400)
        if not L["ran"](tout, root):
            restore(root)
            return {"skipped": "the test suite did not run"}
        truth = L["failed"](tout, root) - baseline_failed
        if "ids" in L:
            ids = L["ids"](tout, root) - baseline_failed
    restore(root)
    return {"failed": sorted(truth)} if ids is None else {"failed": sorted(truth), "ids": sorted(ids)}


TOP = 10  # tests-for names this many by default


def check_tests(root: Path, repo: dict, s: dict, baseline_failed: set[str]) -> dict | None:
    L = LANG[repo["lang"]]
    t = cached(f"{repo['name']}@{repo['commit'][:12]}:{L.get('tests_key', 'tests')}:{s['qualified']}", lambda: failing_tests(root, repo, s, baseline_failed),
               keep=lambda v: v.get("skipped") != "the test suite did not run")
    if t.get("skipped") == "no body":
        return None
    if "skipped" in t:
        return {"symbol": s["qualified"], "skipped": t["skipped"]}
    # a test is its file (TS), `file::name` (Python: `ids`) or its name
    level = "file" if L["level"] == "file" else "id" if "ids" in t else "name"
    key = {"file": lambda n, f: f, "id": lambda n, f: f"{f}::{n}", "name": lambda n, f: n}[level]
    truth = set(t["ids"] if level == "id" else t["failed"])
    rows = tests_for_rows(root, s["qualified"])
    order = list(dict.fromkeys(key(n, f) for n, f, how in rows))
    graph = {key(n, f) for n, f, how in rows if "hop" in how or "direct" in how}
    pred = set(order)
    if not truth:
        return {"symbol": s["qualified"], "skipped": "no test fails when it throws", "predicted": len(pred)}
    tp = len(pred & truth)
    top = order[:TOP]
    top_hits = sum(1 for x in top if x in truth)
    out = {"symbol": s["qualified"], "level": level, "truth": len(truth), "predicted": len(pred),
           "recall": tp / len(truth), "precision": tp / len(pred) if pred else 0.0,
           # of the 10 tests named first: how many fail, and how many could at best (few tests fail at all)
           "top_hits": top_hits, "top_n": len(top), "top_precision": top_hits / len(top) if top else 0.0,
           "top_best": min(tp, len(top)) / len(top) if top else 0.0, "caught": 1.0 if top_hits else 0.0,
           "missed": sorted(truth - pred)[:8]}
    if level != "file":
        out["recall_graph_only"] = len(graph & truth) / len(truth)
    return out


def look_cost(root: Path, s: dict) -> dict:
    _, out = gx(["look", s["qualified"]], root)
    files = {s["file"]} | {c.split("::")[0] for c in []}
    naive = sum(tokens((root / f).read_text(errors="replace")) for f in files if (root / f).exists())
    return {"symbol": s["qualified"], "look_tokens": tokens(out), "file_tokens": naive}


def run_repo(repo: dict, n: int, mutants: int, seed, tests_in_rename: bool) -> dict:
    """One repo; `seed`: a number or a list of them. A function that several seeds picked is measured
    once. `summary` is over all distinct functions, `summary_seeds` over each seed's own sample, and
    `noise` says how far each number moves when the functions are drawn again."""
    global CALLERS_MEMO
    seeds = [seed] if isinstance(seed, int) else list(seed)
    t0 = time.time()
    root = prepare(repo)
    L = LANG[repo["lang"]]
    ti = time.time()
    code, out = gx(["init"], root, timeout=1800)
    index_s = time.time() - ti
    syms = symbols(root)
    res: dict = {"repo": repo["name"], "lang": repo["lang"], "commit": repo["commit"][:12], "index_seconds": round(index_s, 1),
                 "symbols": sum(1 for s in syms if s["kind"] != "module"), "files": len({s["file"] for s in syms}), "seeds": seeds}
    # how many call edges resolve inside the repo
    m = re.search(r"(\d+) symbols · (\d+) edges", gx(["index"], root)[1])
    res["edges"] = int(m.group(2)) if m else None
    prev = SAMPLE_FROM.get(repo["name"]) if SAMPLE_FROM is not None else None
    if SAMPLE_FROM is not None and prev is None:
        raise RuntimeError(f"--sample-from: no sample of {repo['name']} in that run")
    if prev is not None:  # the functions of the earlier run, not drawn again: a change of the graph moves no function
        if prev.get("commit") != res["commit"]:
            raise RuntimeError(f"--sample-from: {repo['name']} was at {prev.get('commit')}, now at {res['commit']}")
        by_q = {s["qualified"]: s for s in syms}
        again = lambda key: {sd: [by_q[q] for q in prev.get(key, {}).get(str(sd), []) if q in by_q] for sd in seeds}
        plain, common, blind = again("samples"), again("samples_common"), again("samples_blind")
        pool = {s["qualified"]: s for v in (*plain.values(), *common.values(), *blind.values()) for s in v}
        qs = list(pool)
        for at in range(0, len(qs), 40):  # what gitnext says now, asked as pick asks it
            for q, (sure, maybe) in callers_many(root, qs[at:at + 40]).items():
                pool[q]["_callers"], pool[q]["_maybe"] = sure, maybe
    else:
        CALLERS_MEMO = {}
        try:
            plain = {sd: pick(syms, root, n, sd) for sd in seeds}
            common = {sd: pick(syms, root, n, sd, common=True) if L["errors"] else [] for sd in seeds}
            blind = {sd: pick(syms, root, n, sd, blind=True) if L["errors"] else [] for sd in seeds}
        finally:
            CALLERS_MEMO = None

    def distinct(by_seed: dict) -> list[dict]:
        seen: dict[str, dict] = {}
        for sd in seeds:
            for s in by_seed[sd]:
                seen.setdefault(s["qualified"], s)
        return list(seen.values())

    sample, sample_common, sample_blind = distinct(plain), distinct(common), distinct(blind)
    res["sample"] = [s["qualified"] for s in sample]
    res["sample_common"] = [s["qualified"] for s in sample_common]
    res["samples"] = {str(sd): [s["qualified"] for s in plain[sd]] for sd in seeds}
    res["samples_common"] = {str(sd): [s["qualified"] for s in common[sd]] for sd in seeds}
    res["samples_blind"] = {str(sd): [s["qualified"] for s in blind[sd]] for sd in seeds}
    if "collect" in L:  # on the untouched commit, before the first mutant
        PY_TESTS[str(root)] = cached(f"{repo['name']}@{repo['commit'][:12]}:collect", lambda: L["collect"](root), keep=bool)
    # baseline: tests already failing before any change are not ours
    base = cached(f"{repo['name']}@{repo['commit'][:12]}:baseline:{L.get('tests_key', 'tests')}", lambda: baseline_tests(root, L), keep=lambda v: v["ran"])
    if not base["ran"]:  # otherwise "no test failed" would pass every check below
        raise RuntimeError(f"the test suite of {repo['name']} did not run: {base['tail']}")
    baseline_failed = set(base["failed"])  # in the units the checks compare: `ids` where the language has them
    res["baseline_failing_tests"] = sorted(baseline_failed)[:20]
    res["callers"] = [x for s in sample if (x := check_callers(root, repo, syms, s))]
    res["callers_common"] = [x for s in sample_common if (x := check_callers(root, repo, syms, s))]
    # picked whether gitnext sees a caller or not: the functions the compiler finds uses of
    res["callers_blind"] = [x for s in sample_blind if (x := check_callers(root, repo, syms, s))]
    res["rename"] = [check_rename(root, repo, s, tests_in_rename, baseline_failed) for s in sample]
    # mutants: down each seed's sample until `mutants` functions are measured (with the throw some do not
    # compile, some fail no test), trying at most MAX_TRIES × mutants of them
    done: dict[str, dict | None] = {}
    res["mutants"] = {}
    for sd in seeds:
        tried, got = [], 0
        for s in plain[sd][: MAX_TRIES * mutants]:
            if got >= mutants:
                break
            q = s["qualified"]
            if q not in done:
                done[q] = check_tests(root, repo, s, baseline_failed)
            if done[q] is None:
                continue
            tried.append(q)
            got += "recall" in done[q]
        res["mutants"][str(sd)] = tried
    res["tests"] = [x for x in done.values() if x]
    res["look"] = [look_cost(root, s) for s in sample]
    res["seconds"] = round(time.time() - t0)
    res["summary"] = summarize(res)
    res["summary_seeds"] = {str(sd): summarize(res, seed_only(res, sd)) for sd in seeds}
    res["noise"] = noise(metric_values(res), [metric_values(res, seed_only(res, sd)) for sd in seeds])
    return res


def mean(v: list[float]) -> float | None:
    return round(sum(v) / len(v), 3) if v else None


METRIC_NAMES = {
    "callers_recall": "callers recall", "callers_precision": "callers precision", "blind_recall": "callers recall, picked blind",
    "common_recall": "shared names: recall", "common_recall_with_maybe": "shared names: recall with maybe",
    "common_precision": "shared names: precision", "rename_ok": "rename ok (tests pass)",
    "tests_recall": "tests recall", "tests_precision": "tests precision", "tests_top10_precision": "top-10 that fail",
    "tests_top10_best": "… best possible order", "tests_caught": "caught by the top 10", "look_vs_file": "look / file",
}


def metric_values(r: dict, only: dict | None = None) -> dict[str, list[float]]:
    """The per-function values behind every number of a repo. `only` (see seed_only): one seed's functions."""
    def keep(kind: str, rows: list[dict]) -> list[dict]:
        return rows if only is None else [x for x in rows if x["symbol"] in only[kind]]
    cal = [c for c in keep("plain", r["callers"]) if c.get("recall") is not None]
    com = [c for c in keep("common", r.get("callers_common", [])) if c.get("recall") is not None]
    bli = [c for c in keep("blind", r.get("callers_blind", [])) if c.get("recall") is not None]
    tst = [t for t in keep("mutants", r["tests"]) if "recall" in t]
    return {
        "callers_recall": [c["recall"] for c in cal],
        "callers_precision": [c["precision"] for c in cal if c["precision"] is not None],
        # the first functions of the same order, with or without a caller in gitnext's graph
        "blind_recall": [c["recall"] for c in bli],
        # methods whose name several types share: sure callers, and sure + "maybe"
        "common_recall": [c["recall"] for c in com],
        "common_recall_with_maybe": [c["recall_with_maybe"] for c in com],
        "common_precision": [c["precision"] for c in com if c["precision"] is not None],
        "rename_ok": [1.0 if x["ok"] else 0.0 for x in keep("plain", r["rename"])],
        "tests_recall": [t["recall"] for t in tst],
        "tests_precision": [t["precision"] for t in tst],
        "tests_top10_precision": [t["top_precision"] for t in tst],
        "tests_top10_best": [t["top_best"] for t in tst],
        "tests_caught": [t["caught"] for t in tst],
        "look_vs_file": [l["look_tokens"] / max(1, l["file_tokens"]) for l in keep("plain", r["look"])],
    }


def seed_only(r: dict, seed) -> dict:
    return {"plain": set(r["samples"][str(seed)]), "common": set(r["samples_common"][str(seed)]),
            "blind": set(r.get("samples_blind", {}).get(str(seed), [])), "mutants": set(r["mutants"][str(seed)])}


def summarize(r: dict, only: dict | None = None) -> dict:
    v = metric_values(r, only)
    return {
        "callers_recall": mean(v["callers_recall"]),
        "callers_precision": mean(v["callers_precision"]),
        "callers_checked": len(v["callers_recall"]),
        "blind_recall": mean(v["blind_recall"]),
        "blind_checked": len(v["blind_recall"]),
        "common_recall": mean(v["common_recall"]),
        "common_precision": mean(v["common_precision"]),
        "common_recall_with_maybe": mean(v["common_recall_with_maybe"]),
        "common_checked": len(v["common_recall"]),
        "rename_ok": mean(v["rename_ok"]),
        "rename_checked": len(v["rename_ok"]),
        "tests_recall": mean(v["tests_recall"]),
        "tests_precision": mean(v["tests_precision"]),
        "tests_top10_precision": mean(v["tests_top10_precision"]),
        "tests_top10_best": mean(v["tests_top10_best"]),
        "tests_caught": mean(v["tests_caught"]),
        "tests_checked": len(v["tests_recall"]),
        "look_vs_file": mean(v["look_vs_file"]),
    }


def noise(vals: dict[str, list[float]], per_seed: list[dict[str, list[float]]]) -> dict:
    """Which difference is real. For every metric: the mean over the functions, the 90% interval it stays
    in when the functions are drawn again (bootstrap), its value on each seed's own sample, and the
    threshold — half that interval, but at least what one function weighs. A change below it is noise."""
    out: dict = {}
    for k, v in vals.items():
        if not v:
            out[k] = None
            continue
        n = len(v)
        rng = random.Random(BOOT)
        boots = sorted(sum(rng.choices(v, k=n)) / n for _ in range(BOOT))
        lo, hi = boots[int(0.05 * BOOT)], boots[int(0.95 * BOOT) - 1]
        half = (hi - lo) / 2
        seeds = [sum(s[k]) / len(s[k]) for s in per_seed if s[k]]
        thr = max(half, 1 / n) if k.endswith(RATES) or k.endswith("best") else half
        out[k] = {"mean": round(sum(v) / n, 3), "functions": n, "boot_lo": round(lo, 3), "boot_hi": round(hi, 3),
                  "boot_half": round(half, 3), "seeds": [round(x, 3) for x in seeds],
                  "seed_sd": round(statistics.stdev(seeds), 3) if len(seeds) > 1 else None,
                  "threshold": math.ceil(thr * 100 - 1e-9) / 100}
    return out


def scopes(results: list[dict]) -> dict:
    """`noise` per language and for all repos together: the functions are pooled, each counts once."""
    ok = [r for r in results if r.get("summary")]
    groups: dict[str, list[dict]] = {"all": ok} if ok else {}
    for r in ok:
        groups.setdefault(r["lang"], []).append(r)
    out = {}
    for name, rs in groups.items():
        def pool(sd=None) -> dict[str, list[float]]:
            per = [metric_values(r, None if sd is None else seed_only(r, sd)) for r in rs]
            return {k: [x for p in per for x in p[k]] for k in METRIC_NAMES}
        out[name] = noise(pool(), [pool(sd) for sd in rs[0]["seeds"]])
    return out


def render_md(results: list[dict], agg: dict | None = None, note: str = "") -> str:
    def pm(z: dict, k: str, count: bool = False) -> str:  # "77% ±6 (20)"
        e = (z or {}).get(k)
        return "—" if not e else f"{e['mean']:.0%} ±{e['threshold'] * 100:.0f}" + (f" ({e['functions']})" if count else "")
    rows = ["| repo | lang | symbols | index s | callers recall | callers precision | callers recall, picked blind | shared names: recall (+maybe) | shared names: precision | rename ok "
            "| tests recall | tests precision | top-10 fail (best possible) | caught by top-10 | look / file |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for r in results:
        if "error" in r:
            rows.append(f"| {r['repo']} | {r.get('lang','')} | error: {r['error'][:60]} ||||||||||||")
            continue
        z = r.get("noise") or {}
        com = "—" if not z.get("common_recall") else f"{pm(z, 'common_recall')} ({pm(z, 'common_recall_with_maybe')}) ({z['common_recall']['functions']})"
        top = "—" if not z.get("tests_top10_precision") else f"{pm(z, 'tests_top10_precision')} ({pm(z, 'tests_top10_best')})"
        rows.append(f"| {r['repo']} | {r['lang']} | {r['symbols']} | {r['index_seconds']} | {pm(z, 'callers_recall', True)} | "
                    f"{pm(z, 'callers_precision')} | {pm(z, 'blind_recall', True)} | {com} | {pm(z, 'common_precision')} | {pm(z, 'rename_ok', True)} | "
                    f"{pm(z, 'tests_recall', True)} | {pm(z, 'tests_precision')} | {top} | {pm(z, 'tests_caught')} | {pm(z, 'look_vs_file')} |")
    out = "\n".join(rows) + "\n"
    if not agg:
        return out
    out += ("\n`77% ±6 (20)`: the mean over 20 functions; a change smaller than ±6 points is noise. The ± is half the 90% interval\n"
            "the number stays in when the functions are drawn again (bootstrap), and never less than what one function weighs.\n")
    langs = [k for k in agg if k != "all"]
    out += "\n| per language | all | " + " | ".join(langs) + " |\n|---|" + "---:|" * (len(langs) + 1) + "\n"
    for k, label in METRIC_NAMES.items():
        out += f"| {label} | " + " | ".join(pm(agg[s], k, True) for s in ["all", *langs]) + " |\n"
    pct = lambda x: f"{x * 100:.0f}"
    out += ("\n| all repos | mean | functions | 90% interval | on each seed | 2 × SD of the seeds | threshold |\n"
            "|---|---:|---:|---:|---:|---:|---:|\n")
    for k, label in METRIC_NAMES.items():
        e = agg["all"].get(k)
        if e:
            sd = "—" if e["seed_sd"] is None else f"{2 * e['seed_sd'] * 100:.1f}"
            out += (f"| {label} | {e['mean']:.1%} | {e['functions']} | {pct(e['boot_lo'])}–{pct(e['boot_hi'])}% | "
                    f"{' / '.join(pct(x) for x in e['seeds'])} | {sd} | ±{e['threshold'] * 100:.0f} |\n")
    return out + ("\n" + note + "\n" if note else "")


def compare(results: list[dict], baseline: dict) -> list[str]:
    """Rates that dropped by more than their threshold (`thresholds` of the baseline: per repo and metric)."""
    bad = []
    for r in results:
        if "error" in r:  # a repo that did not run is not a pass
            bad.append(f"{r['repo']}: {r['error'][:200]}")
            continue
        b = baseline.get("repos", {}).get(r["repo"], {})
        thr = baseline.get("thresholds", {}).get(r["repo"], {})
        for k, v in (r.get("summary") or {}).items():
            if k.endswith(RATES) and v is not None and b.get(k) is not None and v < b[k] - thr.get(k, TOL) - 1e-9:
                bad.append(f"{r['repo']}: {k} {b[k]:.0%} → {v:.0%} (threshold {thr.get(k, TOL) * 100:.0f})")
    return bad


# --compare: the per-function value of every metric, read from the rows of one kind of check
# (the same formulas as metric_values; None: the function does not count for this metric)
PAIRED = {
    "callers_recall": ("callers", lambda c: c.get("recall")),
    "callers_precision": ("callers", lambda c: c.get("precision") if c.get("recall") is not None else None),
    "blind_recall": ("callers_blind", lambda c: c.get("recall")),
    "common_recall": ("callers_common", lambda c: c.get("recall")),
    "common_recall_with_maybe": ("callers_common", lambda c: c.get("recall_with_maybe") if c.get("recall") is not None else None),
    "common_precision": ("callers_common", lambda c: c.get("precision") if c.get("recall") is not None else None),
    "rename_ok": ("rename", lambda x: 1.0 if x["ok"] else 0.0),
    "tests_recall": ("tests", lambda t: t.get("recall")),
    "tests_precision": ("tests", lambda t: t.get("precision") if "recall" in t else None),
    "tests_top10_precision": ("tests", lambda t: t.get("top_precision") if "recall" in t else None),
    "tests_top10_best": ("tests", lambda t: t.get("top_best") if "recall" in t else None),
    "tests_caught": ("tests", lambda t: t.get("caught") if "recall" in t else None),
    "look_vs_file": ("look", lambda x: x["look_tokens"] / max(1, x["file_tokens"])),
}


def _load_run(p: str) -> list[dict]:
    path = Path(p)
    if not path.exists() and (HERE / "results" / f"{p}.json").exists():
        path = HERE / "results" / f"{p}.json"  # a bare name: eval/results/<name>.json
    data = json.loads(path.read_text())
    if not isinstance(data, list) or not all(isinstance(r, dict) and "repo" in r for r in data):
        sys.exit(f"{path}: not the results of a run (eval/results/<name>.json, a list with one entry per repo)")
    return data


def paired_values(a: list[dict], b: list[dict]) -> tuple[dict, list[str]]:
    """For every metric, the functions both runs measured: (repo, lang, symbol, value in A, value in B).
    And what does not pair: repos or commits that differ, functions only one run has."""
    ra = {r["repo"]: r for r in a if "error" not in r}
    rb = {r["repo"]: r for r in b if "error" not in r}
    notes = []
    for name in sorted(set(ra) ^ set(rb)):
        notes.append(f"{name}: only in {'A' if name in ra else 'B'} (or failed in the other run) — left out")
    out: dict[str, list[tuple]] = {k: [] for k in PAIRED}
    lost: dict[str, list[int]] = {}  # kind of rows → [functions in A, in B, in both]
    for name in sorted(set(ra) & set(rb)):
        x, y = ra[name], rb[name]
        if x.get("commit") != y.get("commit"):
            notes.append(f"{name}: different commits ({x.get('commit')} / {y.get('commit')}) — left out")
            continue
        for kind in {k for k, _ in PAIRED.values()}:
            ia = {row["symbol"]: row for row in x.get(kind, [])}
            ib = {row["symbol"]: row for row in y.get(kind, [])}
            both = [s for s in ia if s in ib]
            c = lost.setdefault(kind, [0, 0, 0])
            c[0] += len(ia)
            c[1] += len(ib)
            c[2] += len(both)
            for k, (kk, get) in PAIRED.items():
                if kk != kind:
                    continue
                for s in both:
                    va, vb = get(ia[s]), get(ib[s])
                    if va is not None and vb is not None:
                        out[k].append((name, x["lang"], s, va, vb))
    for kind, (na, nb, nboth) in sorted(lost.items()):
        if na != nboth or nb != nboth:
            notes.append(f"{kind}: {na} functions in A, {nb} in B, {nboth} in both — "
                         f"{(na + nb - 2 * nboth) / max(1, na + nb):.0%} do not pair (the sample depends on gitnext's answers)")
    return out, notes


def paired_stats(rows: list[tuple]) -> dict | None:
    """The mean difference B − A over the functions, its 90% interval when the functions are drawn again
    (bootstrap, within each repo — a repo keeps its number of functions) and the verdict."""
    if not rows:
        return None
    n = len(rows)
    by_repo: dict[str, list[float]] = {}
    for repo, _, _, va, vb in rows:
        by_repo.setdefault(repo, []).append(vb - va)
    d = [x for v in by_repo.values() for x in v]
    rng = random.Random(BOOT)
    boots = sorted(sum(sum(rng.choices(v, k=len(v))) for v in by_repo.values()) / n for _ in range(BOOT))
    lo, hi = boots[int(0.05 * BOOT)], boots[int(0.95 * BOOT) - 1]
    # the same mean, but for two versions measured on independent samples: what --check and latest.md allow
    a = [r[3] for r in rows]
    rng = random.Random(BOOT)
    ba = sorted(sum(rng.choices(a, k=n)) / n for _ in range(BOOT))
    unpaired = (ba[int(0.95 * BOOT) - 1] - ba[int(0.05 * BOOT)]) / 2
    changed = sum(1 for x in d if abs(x) > 1e-12)
    # a difference whose whole interval is under half a point is real but too small to matter
    verdict = ("same" if not changed else "< 0.5 pt" if max(abs(lo), abs(hi)) < 0.005
               else "better" if lo > 0 else "worse" if hi < 0 else "can't tell")
    return {"functions": n, "changed": changed, "a": sum(a) / n, "b": sum(r[4] for r in rows) / n,
            "diff": sum(d) / n, "lo": lo, "hi": hi, "unpaired_half": unpaired, "verdict": verdict}


def compare_runs(pa: str, pb: str, top: int = 10) -> str:
    """`--compare A B`: the two runs on the functions both measured. A difference counts when its 90%
    interval stays on one side of zero: the noise of the sample cancels out, only functions whose
    answer changed move the number."""
    pairs, notes = paired_values(_load_run(pa), _load_run(pb))
    pts = lambda x: f"{x * 100:+.1f}"
    md = [f"# {Path(pa).stem} (A) → {Path(pb).stem} (B)", "",
          "Paired: each function is measured by both versions, the difference B − A is taken per function, and the "
          "90% interval comes from drawing the functions again (within each repo). `better` / `worse`: the whole interval "
          "is on one side of zero. `can't tell`: it covers zero. `same`: no function changed. `< 0.5 pt`: the whole interval "
          "is within half a point. `changed`: functions whose "
          "value differs — a difference rests on them alone, read them below. `± alone`: the noise of one version on its "
          "own (as in latest.md): two separate runs would need a difference that large.", ""]
    if notes:
        md += ["Not paired:", *[f"- {x}" for x in notes], ""]
    langs = sorted({r[1] for v in pairs.values() for r in v})
    repos = sorted({r[0] for v in pairs.values() for r in v})
    head = "| metric | functions | changed | A | B | B − A, points | 90% interval | ± alone | verdict |"
    sep = "|---|---:|---:|---:|---:|---:|---:|---:|---|"

    def table(title: str, keep) -> list[str]:
        rows = []
        for k, label in METRIC_NAMES.items():
            s = paired_stats([r for r in pairs[k] if keep(r)])
            if s:
                rows.append(f"| {label} | {s['functions']} | {s['changed']} | {s['a']:.1%} | {s['b']:.1%} | {pts(s['diff'])} | "
                            f"{pts(s['lo'])} … {pts(s['hi'])} | ±{s['unpaired_half'] * 100:.1f} | **{s['verdict']}** |")
        return [f"## {title}", "", head, sep, *rows, ""] if rows else []

    md += table("All repos", lambda r: True)
    for lang in langs:
        md += table(lang, lambda r, lang=lang: r[1] == lang)
    # per repo: only the metrics where some function changed
    per = ["## Per repo (metrics where a function changed)", "", "| repo | metric | functions | changed | A | B | B − A | 90% interval | verdict |",
           "|---|---|---:|---:|---:|---:|---:|---:|---|"]
    for repo in repos:
        for k, label in METRIC_NAMES.items():
            s = paired_stats([r for r in pairs[k] if r[0] == repo])
            if s and s["changed"]:
                per.append(f"| {repo} | {label} | {s['functions']} | {s['changed']} | {s['a']:.1%} | {s['b']:.1%} | {pts(s['diff'])} | "
                           f"{pts(s['lo'])} … {pts(s['hi'])} | {s['verdict']} |")
    md += (per + [""]) if len(per) > 4 else ["## Per repo", "", "No function changed its value.", ""]
    ch = [f"## Functions that changed (up to {top} per metric, largest first)", ""]
    for k, label in METRIC_NAMES.items():
        moved = sorted((r for r in pairs[k] if abs(r[4] - r[3]) > 1e-12), key=lambda r: -abs(r[4] - r[3]))
        if moved:
            ch.append(f"- **{label}** ({len(moved)}): " + "; ".join(f"{r[0]} `{r[2]}` {r[3]:.0%} → {r[4]:.0%}" for r in moved[:top]))
    md += ch if len(ch) > 2 else []
    return "\n".join(md) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--compare", nargs=2, metavar=("A", "B"), help="compare two runs (their .json, or names in eval/results) on the\n"
                    "functions both measured: the difference, its 90%% interval and whether B is better, worse or can't tell")
    ap.add_argument("--compare-out", metavar="FILE", help="with --compare: also write the table to this file")
    ap.add_argument("--sample-from", metavar="RUN", help="measure the functions an earlier run picked (its .json, or a name in\n"
                    "eval/results) instead of drawing them again: the sample asks gitnext, so a change of the graph\n"
                    "would move it. Then --compare pairs every function. Same --seeds as that run")
    ap.add_argument("--repos", default="", help="comma-separated names (default: all)")
    ap.add_argument("--n", type=int, default=20, help="functions sampled per repo and seed")
    ap.add_argument("--mutants", type=int, default=8, help="of those, how many get the test-suite check (one that cannot be\n"
                    "measured is replaced by the next of the sample; at most twice as many are tried)")
    ap.add_argument("--seeds", default="7,8,9", help="the samples to draw. The numbers are over all distinct functions;\n"
                    "the seeds show how much a number depends on the sample")
    ap.add_argument("--seed", type=int, help="one sample: the same as --seeds N")
    ap.add_argument("--fast", action="store_true", help="after a rename only compile, skip the test suite (a rename that edits a\n"
                    "string the tests compare against compiles fine and is still wrong)")
    ap.add_argument("--no-cache", action="store_true", help="ask the compilers and test suites again (eval/.work/truth.json)")
    ap.add_argument("--out", default="latest", help="name of the result files in eval/results")
    ap.add_argument("--check", action="store_true", help="exit 1 if a rate dropped below eval/baseline.json by more than its threshold")
    ap.add_argument("--bless", action="store_true", help="accept the results as the baseline of this sample (--n, --mutants, --seeds)")
    args = ap.parse_args()
    if args.compare:
        md = compare_runs(*args.compare)
        if args.compare_out:
            Path(args.compare_out).write_text(md)
        print(md)
        return
    global USE_CACHE, SAMPLE_FROM
    USE_CACHE = not args.no_cache
    if args.sample_from:
        SAMPLE_FROM = {r["repo"]: r for r in _load_run(args.sample_from) if "error" not in r}
    seeds = [args.seed] if args.seed is not None else [int(x) for x in args.seeds.split(",") if x.strip()]
    cfg = f"n{args.n}-m{args.mutants}-seeds{','.join(map(str, seeds))}"  # a baseline belongs to one sample
    repos = json.loads((HERE / "repos.json").read_text())
    if args.repos:
        want = set(args.repos.split(","))
        repos = [r for r in repos if r["name"] in want]
    t0 = time.time()
    results = []
    for repo in repos:
        print(f"== {repo['name']} ({repo['lang']})", flush=True)
        try:
            r = run_repo(repo, args.n, args.mutants, seeds, not args.fast)
        except Exception as e:  # one broken repo must not hide the others
            r = {"repo": repo["name"], "lang": repo["lang"], "error": str(e)}
        results.append(r)
        print(json.dumps(r.get("summary") or r, ensure_ascii=False), flush=True)
    agg = scopes(results)
    secs = round(time.time() - t0)
    note = (f"Sample `{cfg}`: {args.n} functions and {args.mutants} mutants per repo and seed, seeds {', '.join(map(str, seeds))}. "
            f"Run time: {secs // 60} min {secs % 60} s; of the compilers' and test suites' answers {CACHE_STATS['cached']} came from the cache, "
            f"{CACHE_STATS['asked']} were asked now.")
    out = HERE / "results"
    out.mkdir(exist_ok=True)
    (out / f"{args.out}.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
    (out / f"{args.out}.noise.json").write_text(json.dumps({"config": cfg, "seconds": secs, "truth": CACHE_STATS, "scopes": agg}, ensure_ascii=False, indent=1))
    md = render_md(results, agg, note)
    (out / f"{args.out}.md").write_text(md)
    print("\n" + md)
    base_path = HERE / "baseline.json"
    baseline = json.loads(base_path.read_text()) if base_path.exists() else {}
    if not all(isinstance(v, dict) and "repos" in v for v in baseline.values()):
        baseline = {}  # the format before samples had names: one sample, no thresholds
    if args.bless:
        entry = baseline.setdefault(cfg, {"repos": {}, "thresholds": {}})
        for r in results:
            if r.get("summary"):
                entry["repos"][r["repo"]] = r["summary"]
                entry["thresholds"][r["repo"]] = {k: e["threshold"] for k, e in r["noise"].items() if e and k.endswith(RATES)}
        base_path.write_text(json.dumps(baseline, indent=1) + "\n")
        print(f"baseline updated ({cfg})")
    if args.check:
        if cfg not in baseline:
            print(f"no baseline for the sample {cfg} in eval/baseline.json (it has: {', '.join(baseline) or 'none'}); run with --bless first")
            sys.exit(1)
        bad = compare(results, baseline[cfg])
        if bad:
            print("REGRESSIONS:\n  " + "\n  ".join(bad))
            sys.exit(1)
        print("no regressions")


if __name__ == "__main__":
    main()
