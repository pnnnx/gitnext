#!/usr/bin/env python3
"""A plain tool-calling agent on a local model (llama.cpp / any OpenAI-compatible
server), to measure what gitnext gives an agent — without loca or any other harness.

Modes (tools the model gets):
  base     list_dir, read_file, grep, edit_file, run, finish
  gitnext  base + look, rename, replace_symbol (gitnext's own tools)
  ctx      base, and `gitnext context` output appended to the task
  both     gitnext tools + context

Tasks are generated from the eval repos (eval/repos.json, set up by eval/run.py)
and checked by the project itself:
  find      "where is X defined"           → the answer names the right file
  callers   "who calls X"                  → recall against the compiler's list
  fix       a seeded bug makes tests fail  → the test suite passes again
  rename    "rename X to Y everywhere"     → it compiles and no old name is left

    python3 eval/agent.py --host http://127.0.0.1:8080 --repos click,cobra \\
        --tasks find,callers,fix,rename --modes base,gitnext,ctx --repeat 2
Results: eval/results/agent-latest.json (+ .md).
"""
from __future__ import annotations

import argparse
import json
import os
import random
import re
import subprocess
import time
import urllib.request
from pathlib import Path

import run as ev  # eval/run.py: repos, languages, compiler / test plumbing

HERE = Path(__file__).resolve().parent
MAX_OUT = 6000  # chars of tool output shown to the model
CTX_CHARS = int(os.environ.get("AGENT_CTX_CHARS", "40000"))  # ~ the model's context: older tool output is cut beyond it


# ---------------------------------------------------------------- tools

def tool(name, desc, props, req):
    return {"type": "function", "function": {"name": name, "description": desc,
            "parameters": {"type": "object", "properties": props, "required": req}}}


S = {"type": "string"}
BASE_TOOLS = [
    tool("list_dir", "List files in a directory of the project.", {"path": S}, ["path"]),
    tool("read_file", "Read a file (optionally lines start..end).", {"path": S, "start": {"type": "integer"}, "end": {"type": "integer"}}, ["path"]),
    tool("grep", "Search the project for a regular expression; returns file:line: text.", {"pattern": S, "path": S}, ["pattern"]),
    tool("edit_file", "Replace an exact piece of text in a file (old must occur once).", {"path": S, "old": S, "new": S}, ["path", "old", "new"]),
    tool("run", "Run a shell command in the project directory (tests, build).", {"command": S}, ["command"]),
    tool("finish", "Finish the task with the final answer.", {"answer": S}, ["answer"]),
]
GITNEXT_TOOLS = [
    tool("look", "gitnext: everything about a function, class, file or error in one call — its code, who calls it, what it calls, the tests that reach it and how to run them. Use this first.", {"what": S}, ["what"]),
    tool("rename", "gitnext: rename a function/class everywhere (definition, calls, imports, tests, comments).", {"old": S, "new": S}, ["old", "new"]),
    tool("replace_symbol", "gitnext: replace the whole code of one function/class by name, without reading the file.", {"symbol": S, "code": S}, ["symbol", "code"]),
]


def call_tool(name: str, a: dict, root: Path, lang: str) -> str:
    def path(p):
        q = (root / (p or ".")).resolve()
        if root.resolve() not in q.parents and q != root.resolve():
            raise ValueError("path outside the project")
        return q
    try:
        if name == "list_dir":
            p = path(a.get("path"))
            return "\n".join(sorted(x.name + ("/" if x.is_dir() else "") for x in p.iterdir() if not x.name.startswith(".") and x.name not in ("node_modules", "target", ".venv")))[:MAX_OUT]
        if name == "read_file":
            lines = path(a["path"]).read_text(errors="replace").split("\n")
            s, e = int(a.get("start") or 1), int(a.get("end") or len(lines))
            return "\n".join(f"{i}\t{l}" for i, l in enumerate(lines[s - 1:e], s))[:MAX_OUT]
        if name == "grep":
            p = subprocess.run(["grep", "-rnE", "--exclude-dir=node_modules", "--exclude-dir=.venv", "--exclude-dir=target", "--exclude-dir=.gitnext", "--exclude-dir=.git", a["pattern"], str(path(a.get("path")))],
                               capture_output=True, text=True, errors="replace")
            return p.stdout.replace(str(root) + "/", "")[:MAX_OUT] or "no matches"
        if name == "edit_file":
            f = path(a["path"])
            t = f.read_text(errors="replace")
            if t.count(a["old"]) != 1:
                return f"error: old text occurs {t.count(a['old'])} times (must be exactly once)"
            f.write_text(t.replace(a["old"], a["new"]))
            return "ok"
        if name == "run":
            code, out = ev.sh(a["command"], root, timeout=600)
            return f"exit {code}\n{out[-MAX_OUT:]}"
        if name == "look":
            return ev.gx(["look", a["what"]], root)[1][:MAX_OUT]
        if name == "rename":
            return ev.gx(["rename", a["old"], a["new"], "--text"], root)[1][:MAX_OUT]
        if name == "replace_symbol":
            p = subprocess.run([ev.GITNEXT, "edit", "replace", a["symbol"]], input=a["code"], cwd=root, capture_output=True, text=True)
            return (p.stdout + p.stderr)[:MAX_OUT]
    except Exception as e:  # the model sees the error and can try again
        return f"error: {e}"
    return f"unknown tool {name}"


# ---------------------------------------------------------------- model

def chat(host: str, messages: list, tools: list, max_tokens: int) -> dict:
    body = json.dumps({"model": "local", "messages": messages, "tools": tools, "temperature": 0.2, "max_tokens": max_tokens,
                       "chat_template_kwargs": {"enable_thinking": False}}).encode()
    req = urllib.request.Request(f"{host}/v1/chat/completions", data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=1800) as r:
        return json.loads(r.read())


SYSTEM = ("You are a coding agent working in a software project. Use the tools to inspect and change the code. "
          "Be brief. When done, call finish with the answer. Do not modify tests unless asked.")


def agent(task: dict, mode: str, root: Path, lang: str, host: str, max_steps: int, deadline: float) -> dict:
    tools = BASE_TOOLS + (GITNEXT_TOOLS if mode in ("gitnext", "both") else [])
    prompt = task["prompt"]
    if mode in ("ctx", "both"):
        _, c = ev.gx(["context", prompt], root)
        if c.strip():
            prompt += "\n\n" + c.strip()
    msgs = [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]
    used = {"tokens_in": 0, "tokens_out": 0, "steps": 0, "tools": {}}
    answer = ""
    for _ in range(max_steps):
        if time.time() > deadline:
            used["timeout"] = True
            break
        fit(msgs)
        r = chat(host, msgs, tools, 1024)
        u = r.get("usage", {})
        used["tokens_in"] += u.get("prompt_tokens", 0)
        used["tokens_out"] += u.get("completion_tokens", 0)
        m = r["choices"][0]["message"]
        msgs.append({k: v for k, v in m.items() if k in ("role", "content", "tool_calls")})
        calls = m.get("tool_calls") or []
        if not calls:
            answer = m.get("content") or ""
            break
        used["steps"] += 1
        done = False
        for c in calls:
            name = c["function"]["name"]
            try:
                a = json.loads(c["function"].get("arguments") or "{}")
            except ValueError:
                a = {}
            used["tools"][name] = used["tools"].get(name, 0) + 1
            if name == "finish":
                answer, done = a.get("answer", ""), True
                out = "ok"
            else:
                out = call_tool(name, a, root, lang)
            msgs.append({"role": "tool", "tool_call_id": c.get("id", name), "content": out})
        if done:
            break
    used["answer"] = answer[:500]
    return used


def fit(msgs: list) -> None:
    """Shorten the oldest tool results until the conversation fits the context (same for every mode)."""
    size = lambda: sum(len(m.get("content") or "") + len(json.dumps(m.get("tool_calls") or "")) for m in msgs)
    for m in msgs:
        if size() <= CTX_CHARS:
            return
        if m["role"] == "tool" and len(m["content"]) > 400:
            m["content"] = m["content"][:300] + "\n… (cut: old output)"


# ---------------------------------------------------------------- tasks

def make_tasks(repo: dict, root: Path, kinds: list[str], seed: int) -> list[dict]:
    syms = ev.symbols(root)
    sample = ev.pick(syms, root, 6, seed)
    rng = random.Random(seed)
    tasks = []
    if "find" in kinds and sample:
        s = sample[0]
        tasks.append({"kind": "find", "prompt": f"In which file is the function `{s['name']}` defined? Answer with the file path.", "file": s["file"]})
    if "callers" in kinds and len(sample) > 1:
        s = sample[1]
        truth = s["_callers"]
        tasks.append({"kind": "callers", "prompt": f"Which functions or methods call `{s['name']}`? Answer with their names, one per line.",
                      "truth": sorted(q.rsplit("::", 1)[-1] for q in truth)})
    if "rename" in kinds and len(sample) > 2:
        s = sample[2]
        new = s["name"] + ("_v2" if "_" in s["name"] or s["name"].islower() else "V2")
        tasks.append({"kind": "rename", "prompt": f"Rename the function `{s['name']}` to `{new}` everywhere in the project (definition and every use). The project must still compile.",
                      "old": s["name"], "new": new})
    if "fix" in kinds:
        bug = seed_bug(repo, root, syms, rng)
        if bug:
            tasks.append(bug)
    return tasks


MUTATIONS = [(r"(?<![<>=!])<(?![<=])", "<="), (r" \+ 1\b", " - 1"), (r"(?<![=!<>])==(?!=)", "!="), (r"\btrue\b", "false"), (r"\bTrue\b", "False")]


def seed_bug(repo: dict, root: Path, syms: list[dict], rng: random.Random) -> dict | None:
    """Change one operator inside a non-test function so that some test fails."""
    L = ev.LANG[repo["lang"]]
    funcs = [s for s in syms if s["kind"] in ("function", "method") and not s.get("test") and 4 <= s["end_line"] - s["start_line"] <= 40]
    rng.shuffle(funcs)
    for s in funcs[:25]:
        p = root / s["file"]
        lines = p.read_text(errors="replace").split("\n")
        for ln in range(s["start_line"], s["end_line"]):
            text = lines[ln]
            if text.lstrip().startswith(("//", "#", "*")):
                continue
            for pat, rep in MUTATIONS:
                if re.search(pat, text):
                    lines2 = list(lines)
                    lines2[ln] = re.sub(pat, rep, text, count=1)
                    p.write_text("\n".join(lines2))
                    ccode, _ = ev.sh(L["compile"], root, timeout=900)
                    failed = set()
                    if ccode == 0 or repo["lang"] == "python":
                        _, tout = ev.sh(L["tests"], root, timeout=1800)
                        failed = L["failed"](tout, root)
                    if failed:
                        patch = ev.sh("git diff", root)[1]
                        ev.restore(root)
                        name = sorted(failed)[0]
                        return {"kind": "fix", "patch": patch, "prompt": f"The test `{name}` fails. Find the bug in the code and fix it (do not change the tests).", "failing": sorted(failed)[:10]}
                    ev.restore(root)
                    break
    return None


def check(task: dict, repo: dict, root: Path, res: dict) -> bool:
    L = ev.LANG[repo["lang"]]
    if task["kind"] == "find":
        return task["file"] in res["answer"] or Path(task["file"]).name in res["answer"]
    if task["kind"] == "callers":
        got = set(re.findall(r"[A-Za-z_][\w]*", res["answer"]))
        hit = len(set(task["truth"]) & got)
        res["recall"] = hit / len(task["truth"])
        return res["recall"] >= 0.99
    if task["kind"] == "rename":
        left = ev.sh(f"git grep -nw {task['old']} -- . ':!*.md' | head -3", root)[1].strip()
        code, out = ev.sh(L["compile"], root, timeout=1200)
        res["left"] = left[:200]
        return code == 0 and not left and bool(ev.sh(f"git grep -nw {task['new']} | head -1", root)[1].strip())
    if task["kind"] == "fix":
        code, _ = ev.sh(L["compile"], root, timeout=1200)
        if code and repo["lang"] != "python":
            return False
        _, tout = ev.sh(L["tests"], root, timeout=1800)
        return not L["failed"](tout, root)
    return False


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8080")
    ap.add_argument("--repos", default="click,cobra")
    ap.add_argument("--tasks", default="find,callers,fix,rename")
    ap.add_argument("--modes", default="base,gitnext,ctx,both")
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--steps", type=int, default=25)
    ap.add_argument("--minutes", type=float, default=20, help="time limit per run")
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--out", default=str(HERE / "results" / "agent-latest.json"))
    args = ap.parse_args()
    repos = [r for r in json.loads((HERE / "repos.json").read_text()) if r["name"] in args.repos.split(",")]
    modes = args.modes.split(",")
    rows = []
    for repo in repos:
        root = ev.prepare(repo)
        ev.gx(["init"], root, timeout=1800)
        tasks = make_tasks(repo, root, args.tasks.split(","), args.seed)
        for rep in range(args.repeat):
            for task in tasks:
                for mode in modes:
                    ev.restore(root)
                    if task["kind"] == "fix":
                        subprocess.run(["git", "apply"], input=task["patch"], text=True, cwd=root)
                    ev.gx(["index"], root)
                    t0 = time.time()
                    try:
                        res = agent(task, mode, root, repo["lang"], args.host, args.steps, t0 + args.minutes * 60)
                        res["passed"] = check(task, repo, root, res)
                    except Exception as e:
                        res = {"passed": False, "error": str(e)[:300]}
                    res.update({"repo": repo["name"], "task": task["kind"], "mode": mode, "repeat": rep, "seconds": round(time.time() - t0)})
                    rows.append(res)
                    print(f"{repo['name']:8} {task['kind']:8} {mode:8} {'✓' if res['passed'] else '✗'} steps {res.get('steps')} tok {res.get('tokens_in')}/{res.get('tokens_out')} {res['seconds']}s tools {res.get('tools')}", flush=True)
                    Path(args.out).parent.mkdir(exist_ok=True)
                    Path(args.out).write_text(json.dumps(rows, ensure_ascii=False, indent=1))
        ev.restore(root)
    # summary per mode
    md = ["| mode | passed | median steps | tokens in (sum) | gitnext tool calls |", "|---|---:|---:|---:|---:|"]
    for mode in modes:
        rs = [r for r in rows if r["mode"] == mode]
        if not rs:
            continue
        steps = sorted(r.get("steps") or 0 for r in rs)
        gcalls = sum(sum(v for k, v in (r.get("tools") or {}).items() if k in ("look", "rename", "replace_symbol")) for r in rs)
        md.append(f"| {mode} | {sum(r['passed'] for r in rs)}/{len(rs)} | {steps[len(steps)//2]} | {sum(r.get('tokens_in') or 0 for r in rs)} | {gcalls} |")
    Path(args.out).with_suffix(".md").write_text("\n".join(md) + "\n")
    print("\n".join(md))


if __name__ == "__main__":
    main()
