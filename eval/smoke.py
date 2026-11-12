#!/usr/bin/env python3
"""Every gitnext command and every MCP tool, once, on every eval project: does it answer?

Not a measure of quality (eval/run.py is) — a check that nothing crashes, hangs or comes back
empty on real code in five languages. For each project a function with callers, a class, a file
and a folder are picked from the index and every read-only command is run with them; the commands
that change files run in a throw-away copy of the project; the MCP tools are called over stdio.

    GITNEXT=$PWD/target/release/gitnext python3 eval/smoke.py [--repos semver,click]

Result: eval/results/smoke.md / .json — one row per command that failed, was empty, or was slow.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import run as ev  # noqa: E402

SLOW = 20.0  # seconds


ENV = {**os.environ, "GIT_AUTHOR_NAME": "smoke", "GIT_AUTHOR_EMAIL": "smoke@example.invalid", "GIT_COMMITTER_NAME": "smoke", "GIT_COMMITTER_EMAIL": "smoke@example.invalid"}


def sh(args: list[str], cwd: Path, stdin: str | None = None, timeout: int = 300) -> tuple[int, str, float]:
    t = time.time()
    try:
        p = subprocess.run(args, cwd=cwd, input=stdin, capture_output=True, text=True, timeout=timeout, errors="replace", env=ENV)
        return p.returncode, p.stdout + p.stderr, time.time() - t
    except subprocess.TimeoutExpired:
        return -9, "timed out", time.time() - t


def samples(root: Path) -> dict:
    aside = ("benches/", "bench/", "examples/", "example/", "docs/", "tests/", "test/", "_examples/", "scripts/")
    syms = [s for s in ev.symbols(root) if not s.get("test") and not s["file"].startswith(aside)]
    fns = [s for s in syms if s["kind"] in ("function", "method") and s["end_line"] - s["start_line"] >= 3]
    # a function somebody calls, not too common a name
    names: dict[str, int] = {}
    for s in fns:
        names[s["name"]] = names.get(s["name"], 0) + 1
    fn = None
    for s in sorted(fns, key=lambda s: (names[s["name"]], s["file"], s["start_line"])):
        code, out, _ = sh([ev.GITNEXT, "--json", "callers", s["qualified"]], root)
        if code == 0 and '"file"' in out:
            fn = s
            break
    fn = fn or fns[0]
    cls = next((s for s in syms if s["kind"] in ("class", "struct", "interface", "trait", "enum")), fn)
    with_tests = json.loads(sh([ev.GITNEXT, "--json", "--tests", "symbols"], root)[1] or "[]")
    tests = [s for s in with_tests if s.get("test") and s["kind"] in ("function", "method")]
    # Go: `TestX` is a test and `testX` in a _test.go file is its helper; elsewhere `test_x` / `testX` is the test
    first, second = ("Test", "test") if any(t["file"].endswith("_test.go") for t in tests) else ("test", "Test")
    test = next((s for s in tests if s["name"].startswith(first)), None) or next((s for s in tests if s["name"].startswith(second)), None)
    d = str(Path(fn["file"]).parent)
    return {"fn": fn["qualified"], "name": fn["name"], "file": fn["file"], "line": fn["start_line"] + 1, "dir": d if d != "." else "", "cls": cls["qualified"],
            "test": test["name"] if test else fn["name"], "start": fn["start_line"], "end": fn["end_line"]}


def read_only(s: dict) -> list[tuple[str, list[str], str | None]]:
    g = lambda *a: list(a)  # noqa: E731
    err = f"{s['file']}:{s['line']}: error: something went wrong here"
    return [
        ("status", g("status"), None), ("log", g("log", "-n", "5"), None), ("diff", g("diff"), None), ("index", g("index"), None),
        ("map <dir>", g("map", s["dir"] or "."), None), ("outline <file>", g("outline", s["file"]), None), ("outline <dir>", g("outline", s["dir"] or "."), None),
        ("read", g("read", s["fn"]), None), ("show", g("show", s["fn"]), None), ("search", g("search", s["name"]), None),
        ("search --text", g("search", "--text", s["name"]), None), ("symbols <file>", g("symbols", s["file"]), None),
        ("callers", g("callers", s["fn"]), None), ("callees", g("callees", s["fn"]), None), ("impact", g("impact", s["fn"]), None),
        ("hierarchy", g("hierarchy", s["cls"]), None), ("deps", g("deps", s["dir"] or "."), None),
        ("context", g("context", f"fix the bug in {s['name']}"), None), ("since", g("since", "HEAD"), None),
        ("locate", g("locate", err), None), ("history", g("history", s["fn"]), None), ("changelog", g("changelog", "HEAD~1.."), None),
        ("overview", g("overview"), None), ("activity", g("activity"), None), ("hotspots", g("hotspots"), None), ("dead", g("dead"), None),
        ("cycles", g("cycles"), None), ("entrypoints", g("entrypoints"), None), ("journal", g("journal"), None), ("brief", g("brief"), None),
        ("libs", g("libs"), None), ("look <symbol>", g("look", s["fn"]), None), ("look <file>", g("look", s["file"]), None),
        ("look <error>", g("look", err), None), ("look <3 targets>", g("look", s["fn"], s["cls"], s["file"]), None),
        ("doctor", g("doctor"), None), ("ops", g("ops"), None), ("branches", g("branches"), None), ("timeline", g("timeline"), None),
        ("tests-for", g("tests-for", s["fn"]), None), ("digest", g("digest"), f"{err}\nerror: aborting due to 1 previous error\n"),
        ("--json look", g("--json", "look", s["fn"]), None), ("--json callers", g("--json", "callers", s["fn"]), None),
        ("--json overview", g("--json", "overview"), None), ("--json tests-for", g("--json", "tests-for", s["fn"]), None),
        ("bench", g("bench"), None),
    ]


def changing(s: dict, root: Path) -> list[tuple[str, list[str], str | None]]:
    """Run in a copy: one after another, each builds on the last."""
    code = "\n".join(Path(root / s["file"]).read_text(errors="replace").split("\n")[s["start"] - 1:s["end"]]) + "\n"
    new = s["name"] + ("_smoke" if s["name"].islower() or "_" in s["name"] else "Smoke")
    return [
        ("edit replace", ["edit", "replace", s["fn"]], code), ("rename", ["rename", s["fn"], new], None), ("diff (after rename)", ["diff"], None),
        ("ops (after edits)", ["ops"], None), ("add", ["add"], None), ("status (staged)", ["status"], None), ("msg", ["msg"], None),
        ("commit", ["commit", "-m", "smoke"], None), ("undo", ["undo"], None), ("reset <file>", ["reset", s["file"]], None), ("undo --op 1", ["undo", "--op", "1"], None),
    ]


def mcp(root: Path, s: dict, code: str) -> list[dict]:
    """tools/list, then every tool once; arguments are filled from the schema by the property's name."""
    p = subprocess.Popen([ev.GITNEXT, "mcp"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    n = 0

    def ask(method: str, params: dict) -> dict:
        nonlocal n
        n += 1
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params}) + "\n")
        p.stdin.flush()
        line = p.stdout.readline()
        return json.loads(line) if line.strip() else {"error": {"message": "no answer"}}

    ask("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "0"}})
    tools = ask("tools/list", {}).get("result", {}).get("tools", [])
    new = s["name"] + "_mcp"
    fill = {"symbol": s["fn"], "symbols": [s["fn"]], "target": s["fn"], "targets": [s["fn"], s["file"]], "name": s["fn"], "path": s["file"], "file": s["file"], "dir": s["dir"] or ".",
            "query": s["name"], "task": f"fix the bug in {s['name']}", "text": f"{s['file']}:{s['line']}: error: x", "error": f"{s['file']}:{s['line']}: error: x", "code": code,
            "new_name": new, "new": new, "old": s["fn"], "summary": "smoke test note", "note": "smoke test note", "rev": "HEAD", "range": "HEAD~1..", "test": s["test"], "what": s["fn"], "depth": 2, "limit": 5}
    rows = []
    order = sorted(tools, key=lambda t: {"rename_symbol": 1, "delete_symbol": 2}.get(t["name"], 0))  # the two that take the symbol away go last
    for t in order:
        props = (t.get("inputSchema") or {}).get("properties") or {}
        req = (t.get("inputSchema") or {}).get("required") or []
        if t["name"] == "hierarchy":
            fill["symbol"] = s["cls"]
        elif t["name"] == "annotate":
            fill["items"] = [{"symbol": s["fn"], "summary": "smoke test note"}]
        else:
            fill["symbol"] = s["fn"]
        # a new function after the sample, not a second copy of it
        fill["code"] = code.replace(s["name"], s["name"] + "_added", 1) if t["name"] == "insert_after" else code
        if t["name"] == "delete_symbol":
            fill["symbol"] = s["fn"] + "_added"  # the one `insert_after` put in
        args = {k: fill[k] for k in props if k in fill and (k in req or k in ("symbol", "target", "targets", "path", "query", "task", "test") and t["name"] not in ("check",))}
        missing = [k for k in req if k not in args]
        t0 = time.time()
        r = ask("tools/call", {"name": t["name"], "arguments": args})
        res = r.get("result") or {}
        text = "".join(c.get("text", "") for c in res.get("content", []))
        bad = "error" in r or res.get("isError") or not text.strip()
        rows.append({"cmd": f"mcp {t['name']}", "ok": not bad and not missing, "seconds": round(time.time() - t0, 1), "bytes": len(text),
                     "note": (f"no value for required {missing}; " if missing else "") + ((r.get("error") or {}).get("message") or text.strip().split("\n")[0])[:160]})
    p.stdin.close()
    p.wait(timeout=30)
    return rows


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default="")
    ap.add_argument("--out", default=str(HERE / "results" / "smoke.json"))
    a = ap.parse_args()
    repos = [r for r in json.loads((HERE / "repos.json").read_text()) if not a.repos or r["name"] in a.repos.split(",")]
    rows = []
    for repo in repos:
        root = ev.WORK / repo["name"]
        if not root.exists():
            print(f"{repo['name']}: not checked out, skipped", flush=True)
            continue
        ev.sh(f"git checkout -q -f {repo['commit']}", root)
        ev.restore(root)
        ev.gx(["index"], root, timeout=1800)
        s = samples(root)
        print(f"== {repo['name']} ({repo['lang']}): {s['fn']}  {s['file']}", flush=True)

        def note(name: str, code: int, out: str, secs: float, cwd_note: str = "") -> None:
            crashed = "panicked at" in out or code in (-9, 101, 134, 139)
            # `doctor` exits 1 when something is missing for agents (here: no agent config) and says what
            quiet = name in ("add", "reset <file>")  # like git: nothing to say when it worked
            ok = (code == 0 or name == "doctor") and (out.strip() != "" or quiet) and not crashed
            rows.append({"repo": repo["name"], "lang": repo["lang"], "cmd": name, "ok": ok, "crashed": crashed, "code": code, "seconds": round(secs, 1), "bytes": len(out),
                         "note": "" if ok and secs < SLOW else (out.strip().split("\n") or [""])[-1][:160] or "empty output"})

        for name, args, stdin in read_only(s):
            note(name, *sh([ev.GITNEXT, *args], root, stdin))
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / repo["name"]
            shutil.copytree(root, copy, symlinks=True, ignore=shutil.ignore_patterns("node_modules", "target", ".venv"))
            code_text = "\n".join((root / s["file"]).read_text(errors="replace").split("\n")[s["start"] - 1:s["end"]]) + "\n"
            (copy / ".gitnext" / "ops.jsonl").unlink(missing_ok=True)  # the journal of earlier eval runs: operation numbers start again
            for name, args, stdin in changing(s, root):
                note(name, *sh([ev.GITNEXT, *args], copy, stdin))
            sh(["git", "checkout", "-q", "-f", "HEAD"], copy)
            sh([ev.GITNEXT, "index"], copy)
            for r in mcp(copy, s, code_text):
                rows.append({"repo": repo["name"], "lang": repo["lang"], "crashed": False, "code": 0, **r})
        bad = [r for r in rows if r["repo"] == repo["name"] and not r["ok"]]
        print(f"   {sum(1 for r in rows if r['repo'] == repo['name'])} calls, {len(bad)} not ok: " + ", ".join(r["cmd"] for r in bad)[:300], flush=True)
        ev.restore(root)
    Path(a.out).write_text(json.dumps(rows, ensure_ascii=False, indent=1))
    cmds = sorted({r["cmd"] for r in rows}, key=lambda c: (c.startswith("mcp "), c))
    md = [f"{len(rows)} calls: {len(cmds)} commands and MCP tools × {len({r['repo'] for r in rows})} projects. Not ok: {sum(1 for r in rows if not r['ok'])}, crashed: {sum(1 for r in rows if r['crashed'])}, slower than {SLOW:.0f}s: {sum(1 for r in rows if r['seconds'] >= SLOW)}.", "",
          "| command | ok in | slowest, s | not ok: project — what it said |", "|---|---:|---:|---|"]
    for c in cmds:
        rs = [r for r in rows if r["cmd"] == c]
        bad = [r for r in rs if not r["ok"]]
        md.append(f"| `{c}` | {len(rs) - len(bad)}/{len(rs)} | {max(r['seconds'] for r in rs)} | " + "; ".join(f"{r['repo']} — {r['note']}" for r in bad[:4]).replace("|", "/") + " |")
    Path(a.out).with_suffix(".md").write_text("\n".join(md) + "\n")
    print(md[0])


if __name__ == "__main__":
    main()
