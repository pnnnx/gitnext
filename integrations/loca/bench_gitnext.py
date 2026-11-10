#!/usr/bin/env python3
"""Eval: does gitnext help a small local agent? Runs loca's own benchmark
(bench/run.py) in several modes and compares pass rate, steps and tokens.

    python3 bench_gitnext.py --loca ~/Developer/loca \\
        --backend llamacpp --host http://127.0.0.1:8080 --ctx 16384 \\
        --tasks fix_typo,failing_test,big --modes off,mcp --out runs/gitnext.json

Modes (each task runs in every mode, interleaved, in a fresh temp copy):
  off  plain loca
  map  `gitnext init` + `gitnext setup loca`: compact code map in AGENTS.md, no tools
       (what `setup loca` does by default)
  mcp  map + `gitnext setup loca --tools`: gitnext MCP server (lite profile: look,
       replace, add_after) and the LOCA.md instruction block
  mcp-core  as mcp, and look/replace are always in the model's tool list
       (needs loca's `core_tools` option, integrations/loca/core_tools.patch)

Besides loca's tasks (`--tasks core`, `hard`, names) there is a `big` group:
tasks inside a copy of loca's own source (~100k tokens) — the kind of repo
where reading files to orient does not fit a 16k context.

Nothing in the loca repository is modified; `gitnext` must be on PATH.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path


def load_run(loca: Path):
    sys.path.insert(0, str(loca / "bench"))
    spec = importlib.util.spec_from_file_location("loca_bench_run", loca / "bench" / "run.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# ------------------------------------------------------------ big-repo tasks
def haystack(loca: Path) -> dict[str, str]:
    files = {}
    for p in sorted((loca / "loca").rglob("*.py")):
        files[str(p.relative_to(loca))] = p.read_text(encoding="utf-8")
    files["pyproject.toml"] = (loca / "pyproject.toml").read_text(encoding="utf-8")
    return files


def big_tasks(loca: Path) -> list[dict]:
    base = haystack(loca)
    agent = base["loca/agent.py"]
    tasks = []

    tasks.append({
        "name": "big_find_class",
        "group": "big",
        "timeout": 900,
        "files": dict(base),
        "readonly": True,
        "prompt": "В каком файле определён класс LoadTools? Ответь только путём к файлу.",
        "expect_output": r"loca/tools/agentic\.py",
        "checks": [],
    })

    bug = "first[:limit - 1].rstrip()"
    if bug in agent:
        files = dict(base)
        files["loca/agent.py"] = agent.replace(bug, "first[:limit + 1].rstrip()")
        files["tests/test_brief.py"] = (
            "import unittest\n\nfrom loca.agent import brief\n\n\n"
            "class BriefTest(unittest.TestCase):\n"
            "    def test_long_text_fits_limit(self):\n"
            "        self.assertEqual(len(brief('x' * 100, 40)), 40)\n\n"
            "    def test_short_text_unchanged(self):\n"
            "        self.assertEqual(brief('Read a file. More text'), 'Read a file')\n\n\n"
            "if __name__ == '__main__':\n    unittest.main()\n")
        tasks.append({
            "name": "big_failing_test",
            "group": "big",
            "timeout": 1200,
            "files": files,
            "prompt": "Тест tests/test_brief.py падает. Найди причину в коде и исправь (тест не меняй).",
            "checks": ["python3 -m unittest -q tests/test_brief.py"],
        })

    if "def estimate_tokens" in agent:
        tasks.append({
            "name": "big_rename",
            "group": "big",
            "timeout": 1200,
            "files": dict(base),
            "prompt": "Переименуй функцию estimate_tokens в approx_tokens везде в проекте. "
                      "Старого имени остаться не должно.",
            "checks": ["! grep -rqw estimate_tokens --include=*.py .",
                       "python3 -c 'import loca.agent as a; a.approx_tokens([])'"],
        })
    return tasks


# ------------------------------------------------------------ modes
def prepare(mode: str, root: Path) -> None:
    if mode == "off":
        return
    env = {**os.environ, "GITNEXT_NO_OVERVIEW": "1"}
    subprocess.run(["gitnext", "init"], cwd=root, env=env, check=True, capture_output=True)
    if mode == "map":
        subprocess.run(["gitnext", "setup", "loca"], cwd=root, env=env, check=True, capture_output=True)
    if mode in ("mcp", "mcp-core"):
        subprocess.run(["gitnext", "setup", "loca", "--tools"], cwd=root, env=env, check=True, capture_output=True)
    if mode == "mcp-core":
        # needs loca with the `core_tools` option (see integrations/loca/core_tools.patch):
        # gitnext's tools sit in the model's tool list instead of behind load_tools
        cfg_path = root / ".loca" / "config.json"
        cfg = json.loads(cfg_path.read_text())
        cfg["core_tools"] = ["mcp__gitnext__look", "mcp__gitnext__replace"]
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2))


def gitnext_calls(result: dict) -> int:
    return sum(n for t, n in (result.get("tools") or {}).items() if t.startswith("mcp__gitnext__"))


def summarize(rows: list[dict], modes: list[str]) -> str:
    out = []
    head = f"{'task':22}" + "".join(f"{m:>26}" for m in modes)
    out.append(head)
    out.append(f"{'':22}" + "".join(f"{'pass steps tok_in  sec':>26}" for _ in modes))
    rows = [r for r in rows if not r.get("infra_error")]
    by = {}
    for r in rows:
        by.setdefault(r["name"], {})[r["mode"]] = r
    for name, per in by.items():
        line = f"{name:22}"
        for m in modes:
            r = per.get(m)
            if not r:
                line += f"{'-':>26}"
                continue
            ok = "✓" if r["passed"] else "✗"
            part = f" {r['partial']}/{r['partial_total']}" if r.get("partial_total") else ""
            line += f"{ok + part:>6}{r.get('steps', 0):>6}{r.get('tokens_in', 0):>8}{r['seconds']:>6.0f}"
        out.append(line)
    out.append("")
    for m in modes:
        rs = [r for r in rows if r["mode"] == m]
        if not rs:
            continue
        passed = sum(r["passed"] for r in rs)
        tin = sum(r.get("tokens_in", 0) for r in rs)
        tout = sum(r.get("tokens_out", 0) for r in rs)
        steps = statistics.median([r.get("steps", 0) for r in rs])
        gx = sum(gitnext_calls(r) for r in rs)
        out.append(f"{m:5} passed {passed}/{len(rs)} · tokens in {tin} / out {tout} · "
                   f"median steps {steps} · time {sum(r['seconds'] for r in rs):.0f}s"
                   + (f" · gitnext tool calls {gx}" if m == "mcp" else ""))
    return "\n".join(out)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--loca", type=Path, required=True, help="path to the loca repository")
    p.add_argument("--modes", default="off,mcp")
    p.add_argument("--tasks", default="core,big", help="names or groups: core, hard, big")
    p.add_argument("--out", type=Path, default=Path("gitnext-bench.json"))
    p.add_argument("--repeat", type=int, default=1)
    args, rest = p.parse_known_args()
    if not shutil.which("gitnext"):
        sys.exit("gitnext is not on PATH (cargo install --path crates/gitnext-cli)")

    run = load_run(args.loca.resolve())
    # agent state and gitnext's own files are not "changes made by the agent"
    run.AGENT_DIRS = tuple(run.AGENT_DIRS) + (".gitnext", ".git")
    rargs = run_parser(run, rest)

    all_tasks = list(run.TASKS) + big_tasks(args.loca.resolve())
    wanted = []
    for t in args.tasks.split(","):
        t = t.strip()
        group = [x for x in all_tasks if x.get("group") == t or (t in run.GROUPS and x["name"] in run.GROUPS[t])]
        wanted += group or [x for x in all_tasks if x["name"] == t]
    if not wanted:
        sys.exit(f"no tasks match {args.tasks}")
    # count_todo counts TODO lines in *all* files: gitnext's map files would change the answer
    wanted = [t for t in wanted if t["name"] != "count_todo"]

    modes = [m.strip() for m in args.modes.split(",")]
    original = run.materialize
    rows = []
    args.out.parent.mkdir(parents=True, exist_ok=True)
    for rep in range(args.repeat):
        for task in wanted:
            for mode in modes:
                def materialize(t, root, _mode=mode):
                    original(t, root)
                    prepare(_mode, root)
                run.materialize = materialize
                t0 = time.time()
                print(f"[{mode}] {task['name']}…", flush=True)
                r = run.run_task(task, rargs)
                r["mode"] = mode
                if not r.get("tokens_in") and r.get("seconds", 0) < 5:
                    # the model server did not answer (crashed, OOM, not started): not a result
                    r["infra_error"] = True
                    print("  ! model server did not answer — check llama-server (memory?); this run is not counted", flush=True)
                r["repeat"] = rep
                rows.append(r)
                mark = "✓" if r["passed"] else "✗"
                print(f"  {mark} {time.time() - t0:.0f}s steps {r.get('steps')} tokens {r.get('tokens_in')}/{r.get('tokens_out')} "
                      f"gitnext calls {gitnext_calls(r)} {r.get('reason', '')}", flush=True)
                args.out.write_text(json.dumps({"modes": modes, "rows": rows}, ensure_ascii=False, indent=1))
    run.materialize = original
    print()
    print(summarize(rows, modes))
    print(f"\nresults: {args.out}")


def run_parser(run, argv):
    """Parse loca bench flags (--backend, --host, --ctx, -m, --timeout…) with its own parser."""
    import argparse as ap
    captured = {}
    real = ap.ArgumentParser.parse_args

    def grab(self, a=None, ns=None):
        captured["args"] = real(self, a, ns)
        raise SystemExit(0)

    ap.ArgumentParser.parse_args = grab
    try:
        run.main(argv)
    except SystemExit:
        pass
    finally:
        ap.ArgumentParser.parse_args = real
    a = captured["args"]
    for k, v in {"keep": False, "verbose": False, "show_output": False, "loca": "", "think": False,
                 "model": None, "ctx": None, "host": None, "timeout": 420, "time_scale": 1.0, "margin": 45}.items():
        if not hasattr(a, k):
            setattr(a, k, v)
    return a


if __name__ == "__main__":
    main()
