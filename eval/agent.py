#!/usr/bin/env python3
"""A plain tool-calling agent on a local model (llama.cpp / any OpenAI-compatible
server), to measure what gitnext gives an agent — without loca or any other harness.

Modes (tools the model gets):
  base     list_dir, read_file, grep, edit_file, run, finish
  gitnext  base + look, rename, replace_symbol, check (gitnext's own tools; `check` with a
           test name tells why it fails); a failing command's long output comes back as its
           digest (what failed, where, the message)
  lite     only look, replace, add_after, rename, check and finish — the short list of
           gitnext's lite MCP profile (a small model chooses better among six tools than eleven) —
           and the gate of `verify`
  ctx      base, and `gitnext context` output appended to the task
  both     gitnext tools + context
  cmd      no tool-calling API at all: the context is in the task and the model answers with
           one shell command (or a short script) in a ```sh block, or `DONE: answer` — for
           models that write tool calls as text; `gitnext` is on PATH
  verify   gitnext tools, and the run may end only when the work is checked: an answer given
           without a look at the code, or a change that `gitnext check` does not pass, goes back
           to the model (a small model stops after one step otherwise). Every mode with the
           gitnext tools sends back an answer given before any tool was called

Every run is watched by a loop guard (eval/guard.py): repeated calls, no progress, the same
error again and again, see-saw edits, repeated text, step and token budgets. A stopped run
leaves a hand-off note; with --escalate-host a stronger model gets the task and that note.

Tasks are generated from the eval repos (eval/repos.json, set up by eval/run.py)
and checked by the project itself:
  find      "where is X defined"           → the answer names the right file
  callers   "who calls X"                  → recall against the compiler's list
  fix       a seeded bug makes tests fail  → the test suite passes again
  rename    "rename X to Y everywhere"     → it compiles and no old name is left

    python3 eval/agent.py --host http://127.0.0.1:8080 --repos click,cobra \\
        --tasks find,callers,fix,rename --modes base,gitnext,lite --repeat 3
`--repeat N` gives N task sets per repo (other functions, another bug), every mode on each.
Results: eval/results/agent-latest.json (+ .md; .trace.jsonl — every conversation).

One task of your own, on any repository (a "task card" for a small model):

    python3 eval/agent.py --task card.json --mode cmd
    card.json: {"repo": "/path/to/project", "prompt": "what to do", "accept": "gitnext check"}

The run passes when the `accept` command exits 0 (default: `gitnext check`). Next to the card
it writes card.result.json (passed, steps, tokens, what the loop guard did, the hand-off note
if it was stopped) and card.patch (the change, as `git diff`).
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
from guard import Guard, compress, trim_repeats

HERE = Path(__file__).resolve().parent
MAX_OUT = 6000  # chars of tool output shown to the model
DIGEST_FROM = 1200  # gitnext modes: a failing command's output longer than this comes back as its digest
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
    tool("check", "gitnext: run the tests. No arguments: after a change — builds the project, runs the tests of what you changed, answers with a verdict. "
                  "With `test`: why that test fails — the failure in a few lines and the code to fix, the most likely culprit first.", {"test": S}, []),
]
# mode `lite`: these and nothing else — what gitnext's lite MCP profile gives a small model (plus `finish`)
LITE_TOOLS = [
    tool("look", "Look at code. what = a function or class name (shows its code, who calls it, its tests), a file or folder (lists what is in it), an error message, or words to find.", {"what": S}, ["what"]),
    tool("replace", "Replace one whole function or class with new code. No need to read the file first.", {"symbol": S, "code": S}, ["symbol", "code"]),
    tool("add_after", "Add new code (a new function) right after an existing function or class.", {"symbol": S, "code": S}, ["symbol", "code"]),
    tool("rename", "Rename a function or class everywhere: definition, calls, imports, tests.", {"old": S, "new": S}, ["old", "new"]),
    tool("check", "A test fails: call this first with test = its name — it runs the test and shows the failure and the code to fix. After your change: call it with no arguments — says ok or what failed.", {"test": S}, []),
    tool("finish", "Finish the task with the final answer.", {"answer": S}, ["answer"]),
]


def digest(out: str) -> str | None:
    """A long failing output through `gitnext digest`: what failed, where, the message — or None
    when gitnext does not recognise it."""
    try:
        d = subprocess.run([ev.GITNEXT, "digest"], input=out, cwd=".", capture_output=True, text=True, timeout=60).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    return d if d.strip() and not d.startswith(("not recognised", "no output")) else None


DEFINES = re.compile(r"^[ \t]*(?:(?:pub(?:\([a-z]+\))?|async|export|default|static|public|private|protected|final|override|unsafe|const)\s+)*"
                     r"(?:def|fn|func|function)\s+(?:\([^)]*\)\s*)?([A-Za-z_][\w]*)", re.M)
TEXT_CODE_MAX = 3  # times per run a function written in the answer is put in place


def code_as_text(text: str, root: Path) -> str | None:
    """A small model finds the bug and writes the corrected function — in its answer, not as a
    `replace` call. The function it wrote is put in place (when the project has a function of that
    name, it is not a test, and the file still parses) and checked; the model gets the verdict.
    The same idea as `text_calls`: what it meant is clear, only the form is wrong."""
    for block in re.findall(r"```[A-Za-z0-9_+-]*[ \t]*\n(.*?)```", text, re.S):
        m = DEFINES.search(block)
        if not m or block.count("\n") < 1:
            continue
        name = m.group(1)
        if name.lower().startswith("test"):
            continue
        code = block[m.start():].rstrip() + "\n"
        before = ev.sh("git diff", root)[1]
        p = subprocess.run([ev.GITNEXT, "edit", "replace", name], input=code, cwd=root, capture_output=True, text=True)
        if p.returncode != 0:
            continue
        where = (p.stdout.strip().split("\n") or [""])[0].removeprefix("ok ").strip()
        if ev.sh("git diff", root)[1] == before:
            # the function as it already is (or the same wrong fix again): a check would say what it said before
            return (f"The function `{name}` you wrote is the same as the code already in the file ({where}): nothing changed, the test still fails. "
                    "One line of it is wrong. Find that line and write the whole function again with that line corrected.")
        verdict = ev.gx(["check"], root, timeout=900)[1][-MAX_OUT:]
        failed = "verdict: FAILED" in verdict
        return (f"The function `{name}` you wrote was put in place ({where}). The check:\n{verdict}\n"
                + ("It failed: write the whole corrected function again." if failed else "If the task is done, answer in one line."))
    return None


def call_tool(name: str, a: dict, root: Path, lang: str, short: bool = False) -> str:
    """`short`: a failing command's long output is given as its digest (the gitnext modes)."""
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
        if name in ("run", "sh"):
            # `gitnext` on PATH: in `cmd` mode the model calls it like any other command
            # the project's own environment first (`pytest` of its .venv, `vitest` of its node_modules), as a developer has it
            env = "".join(f"{root / d}:" for d in (".venv/bin", "node_modules/.bin") if (root / d).is_dir())
            code, out = ev.sh(f'PATH="{env}{Path(ev.GITNEXT).resolve().parent}:$PATH"; ' + a["command"], root, timeout=600)
            if short and code != 0 and len(out) > DIGEST_FROM:
                d = digest(out)
                if d:
                    return f"exit {code}\n{d}(the whole output: {out.count(chr(10)) + 1} lines)"
            return f"exit {code}\n{out[-MAX_OUT:]}"
        if name == "look":
            return ev.gx(["look", a["what"]], root)[1][:MAX_OUT]
        if name == "rename":
            return ev.gx(["rename", a["old"], a["new"], "--text"], root)[1][:MAX_OUT]
        if name == "check":
            test = (a.get("test") or "").strip()
            return ev.gx(["check", test] if test else ["check"], root, timeout=900)[1][-MAX_OUT if not test else None:][:MAX_OUT]
        if name in ("replace_symbol", "replace", "add_after"):
            kind = "insert-after" if name == "add_after" else "replace"
            p = subprocess.run([ev.GITNEXT, "edit", kind, a["symbol"]], input=a["code"], cwd=root, capture_output=True, text=True)
            return (p.stdout + p.stderr)[:MAX_OUT]
    except Exception as e:  # the model sees the error and can try again
        return f"error: {e}"
    return f"unknown tool {name}"


# ---------------------------------------------------------------- model

def chat(host: str, messages: list, tools: list, max_tokens: int) -> dict:
    body = {"model": "local", "messages": messages, "temperature": 0.2, "max_tokens": max_tokens,
            "repeat_penalty": 1.1,  # llama.cpp: a small model loops on one line without it
            "chat_template_kwargs": {"enable_thinking": False}}
    if tools:
        body["tools"] = tools
    req = urllib.request.Request(f"{host}/v1/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=1800) as r:
        return json.loads(r.read())


def text_calls(content: str, names: set[str]) -> list[dict]:
    """Tool calls a small model wrote as text instead of through the API: a fenced
    ```json {"name": …, "arguments": {…}}```, `<tool_call>{…}</tool_call>`, or `look {"what": "x"}`.
    Returned in the API's own shape, so the conversation continues as if it had used it."""
    dec = json.JSONDecoder()
    found: list[tuple[str, dict]] = []
    i = content.find("{")
    while i >= 0:
        try:
            obj, end = dec.raw_decode(content, i)
        except ValueError:
            i = content.find("{", i + 1)
            continue
        if isinstance(obj, dict):
            inner = obj.get("tool_call") or obj.get("function_call") or obj.get("function") or obj
            before = re.search(r"([A-Za-z_]\w*)\s*[(:]?\s*$", content[:i])
            if isinstance(inner, dict) and inner.get("name") in names:
                args = inner.get("arguments", inner.get("parameters", {}))
                if isinstance(args, str):
                    try:
                        args = json.loads(args)
                    except ValueError:
                        args = {}
                found.append((inner["name"], args if isinstance(args, dict) else {}))
            elif before and before.group(1) in names:  # look {"what": "x"}
                found.append((before.group(1), obj))
        i = content.find("{", end)
    return [{"id": f"text{k}", "type": "function", "function": {"name": n, "arguments": json.dumps(a)}} for k, (n, a) in enumerate(found)]


SYSTEM = ("You are a coding agent working in a software project. Use the tools to inspect and change the code. "
          "Be brief. When done, call finish with the answer. Do not modify tests unless asked.")
CMD_SYSTEM = ("You are a coding agent working in a software project. Answer with ONE shell command (or a short script) in a ```sh block "
              "and nothing else; you will get its output back. Useful commands: `gitnext look <function, class or file>` (its code, who calls it, "
              "its tests — in one call), `gitnext check <test>` (a test fails: the failure in a few lines and the code to look at), "
              "`gitnext rename <old> <new> --text`, `gitnext check` (after a change: builds and runs the tests of what you changed), "
              "cat, grep, sed -i. "
              "When the task is done answer `DONE: <answer>`. Be brief. Do not modify tests unless asked.")
TOKEN_BUDGET = int(os.environ.get("AGENT_TOKEN_BUDGET", "0"))  # prompt tokens per run, 0 = no limit
GITNEXT_MODES = ("gitnext", "both", "verify", "lite")  # modes with the gitnext tools
LITE_SYSTEM = ("You are a coding agent working in a software project. You have five tools for the code: look (read: a function, a file, an error), "
               "replace and add_after (change code), rename, check (a test fails: call it first with the test's name; after a change: with no arguments). "
               "Use them, be brief, and call finish with the answer when done. Do not modify tests unless asked.")


class Gate:
    """The run may end only when the work is checked. A small model stops after one step ("the bug
    is in X") or answers from memory; the gate sends it back, each kind once: use a tool before
    answering (and call a tool instead of writing its name); make the change you were asked for;
    and, as long as files are changed, pass `gitnext check` (up to `CHECKS` times). Nothing here
    knows the task or its answer. Modes `verify` and `lite` have the whole gate; the other modes
    with the gitnext tools only its first part (`full=False`): 5 of the 5 runs lost in the
    find/callers/rename measurement were answers given before any tool was called."""
    CHECKS = 3

    def __init__(self, guard: Guard, check, full: bool = True):
        self.guard, self.check, self.full = guard, check, full
        self.start = guard.fingerprint()
        self.said: dict[str, int] = {}
        # the answer just sent back is to be taken out of the conversation: a small model that sees
        # what it made up repeats it word for word (3 of 8 `fix` runs in lite)
        self.drop = False

    def on_finish(self, tool_steps: int, text: str = "", tools: tuple = ()) -> str | None:
        bare = text.strip(" `\n.()")
        if bare in tools and self.said.get("bare", 0) < 2:
            self.said["bare"] = self.said.get("bare", 0) + 1
            return f"`{bare}` is a tool, not an answer: call it with its arguments."
        if tool_steps == 0 and "tool" not in self.said:
            self.said["tool"] = 1
            self.drop = True
            return "Your first step must be a tool call, not an answer: `check` with the name of the failing test if a test fails, otherwise `look`."
        if not self.full:
            return None
        changed = self.guard.fingerprint() != self.start
        if not changed and "nochange" not in self.said:
            self.said["nochange"] = 1
            return ("No file was changed. If the task asks to change the code, make the change now with the tools; "
                    "if it only asks a question, repeat your answer.")
        if changed and self.said.get("check", 0) < self.CHECKS:
            out = self.check()
            # only a build or a test that fails; "builds, but no test reaches the changed code" is not a failure
            if "verdict: FAILED" in out:
                self.said["check"] = self.said.get("check", 0) + 1
                return "The check of your change failed:\n" + out[-1500:] + "\nFix it, then finish."
        return None


def command_of(content: str) -> str | None:
    """The shell command of a `cmd`-mode answer: the first fenced block, or a `$ command` line."""
    m = re.search(r"```(?:sh|bash|shell|console|zsh)?[ \t]*\n(.*?)```", content, re.S)
    if m:
        return "\n".join(l[2:] if l.startswith("$ ") else l for l in m.group(1).strip().split("\n")) or None
    m = re.search(r"^\$ (.+)$", content, re.M)
    return m.group(1) if m else None


def agent(task: dict, mode: str, root: Path, lang: str, host: str, max_steps: int, deadline: float) -> dict:
    cmd = mode == "cmd"
    tools = [] if cmd else LITE_TOOLS if mode == "lite" else BASE_TOOLS + (GITNEXT_TOOLS if mode in GITNEXT_MODES else [])
    prompt = task["prompt"]
    if task.get("handoff"):  # a weaker model gave up on this task: what it tried
        prompt += "\n\nAn earlier attempt was stopped. Its notes:\n" + task["handoff"]
    if mode in ("ctx", "both", "cmd"):
        _, c = ev.gx(["context", task["prompt"]], root)
        if c.strip():
            prompt += "\n\n" + c.strip()
    msgs = [{"role": "system", "content": CMD_SYSTEM if cmd else LITE_SYSTEM if mode == "lite" else SYSTEM}, {"role": "user", "content": prompt}]
    used = {"tokens_in": 0, "tokens_out": 0, "steps": 0, "tools": {}}

    def locate(err: str) -> str:  # where gitnext thinks an error comes from
        try:
            return subprocess.run([ev.GITNEXT, "locate"], input=err, cwd=root, capture_output=True, text=True, timeout=60).stdout
        except (OSError, subprocess.SubprocessError):
            return ""

    guard = Guard(root, max_steps, TOKEN_BUDGET, locate=locate)
    gate = Gate(guard, lambda: call_tool("check", {}, root, lang), full=mode in ("verify", "lite")) if mode in GITNEXT_MODES else None
    names = tuple(t["function"]["name"] for t in tools) if tools else ()
    answer = ""
    for _ in range(max_steps):
        if time.time() > deadline:
            used["timeout"] = True
            break
        compress(msgs, CTX_CHARS)
        r = chat(host, msgs, tools, 1024)
        u = r.get("usage", {})
        used["tokens_in"] += u.get("prompt_tokens", 0)
        used["tokens_out"] += u.get("completion_tokens", 0)
        m = r["choices"][0]["message"]
        text, cut = trim_repeats(m.get("content") or "")
        if cut:
            guard.stats["trimmed"] += 1
            m = {**m, "content": text}
        calls = m.get("tool_calls") or []
        done = None
        if cmd:
            done = re.search(r"^\s*DONE:\s*(.*)", text, re.S | re.M)
            command = None if done else command_of(text)
            calls = [{"id": "sh", "function": {"name": "sh", "arguments": json.dumps({"command": command})}}] if command else []
        elif not calls:
            # the model wrote the call as text (the server's chat template did not parse it)
            calls = text_calls(text, {t["function"]["name"] for t in tools})
            if calls:
                used["text_calls"] = used.get("text_calls", 0) + len(calls)
                m = {"role": "assistant", "content": "", "tool_calls": calls}
        msgs.append({k: v for k, v in m.items() if k in ("role", "content") or (k == "tool_calls" and not cmd)})
        if not calls:
            if cmd and not done:  # neither a command nor an answer: say what is expected
                used["steps"] += 1
                note = guard.end_step(used["steps"], used["tokens_in"])
                if guard.stop:
                    break
                msgs.append({"role": "user", "content": "Answer with one command in a ```sh block, or `DONE: <answer>`." + (f"\n{note}" if note else "")})
                continue
            answer = done.group(1).strip() if done else text
            if mode in GITNEXT_MODES and used.get("text_code", 0) < TEXT_CODE_MAX:
                # the corrected function is in the answer instead of a `replace` call: put it in place
                applied = code_as_text(text, root)
                if applied:
                    used["text_code"] = used.get("text_code", 0) + 1
                    used["steps"] += 1
                    guard.state = guard.fingerprint()
                    msgs.append({"role": "user", "content": applied})
                    continue
            back = gate.on_finish(used["steps"], text, names) if gate else None
            if back is None:
                break
            if gate.drop:
                gate.drop = False
                msgs.pop()
                msgs[1] = {**msgs[1], "content": msgs[1]["content"] + "\n\n" + back}
            else:
                msgs.append({"role": "user", "content": back})
            continue
        used["steps"] += 1
        finished = False
        for c in calls:
            name = c["function"]["name"]
            try:
                a = json.loads(c["function"].get("arguments") or "{}")
            except ValueError:
                a = {}
            used["tools"][name] = used["tools"].get(name, 0) + 1
            if name == "finish":
                answer, finished = a.get("answer", ""), True
                out = "ok"
            else:
                out = guard.before_call(used["steps"], name, a)
                if out is None:
                    out = guard.after_call(used["steps"], name, a, call_tool(name, a, root, lang, short=cmd or mode in GITNEXT_MODES))
            msgs.append({"role": "user", "content": out} if cmd else {"role": "tool", "tool_call_id": c.get("id", name), "content": out})
        if finished:
            back = gate.on_finish(used["steps"] - 1) if gate else None  # the step that only calls `finish` is not a look at the code
            if back is None:
                break
            msgs.append({"role": "user", "content": back})
            continue
        note = guard.end_step(used["steps"], used["tokens_in"])
        if guard.stop:
            break
        if note:
            msgs.append({"role": "user", "content": note})
    used["answer"] = answer[:500]
    used["guard"] = {k: v for k, v in guard.stats.items() if v}
    if gate and gate.said:
        used["gate"] = gate.said
    used["_msgs"] = msgs  # the whole conversation, for the trace file
    if guard.stop or (not answer and used["steps"] >= max_steps):
        used["stopped"] = guard.stop or "steps"
        used["handoff"] = guard.handoff(task["prompt"])
    return used


# ---------------------------------------------------------------- tasks

def make_tasks(repo: dict, root: Path, kinds: list[str], seed: int) -> list[dict]:
    syms = ev.symbols(root)
    sample = ev.pick(syms, root, 6, seed)
    rng = random.Random(seed)
    tasks = []
    if "find" in kinds and sample:
        s = sample[0]
        tasks.append({"kind": "find", "target": s["qualified"], "prompt": f"In which file is the function `{s['name']}` defined? Answer with the file path.", "file": s["file"]})
    if "callers" in kinds and len(sample) > 1:
        s = sample[1]
        # the compiler's answer where the language has one (gitnext's own answer would favour its modes)
        c = ev.check_callers(root, repo, syms, s) or {}
        truth, judge = (c["truth"], "compiler") if c.get("truth") else (s["_callers"], "gitnext")
        tasks.append({"kind": "callers", "target": s["qualified"], "truth_from": judge,
                      "prompt": f"Which functions or methods call `{s['name']}`? Answer with their names, one per line.",
                      "truth": sorted({q.rsplit("::", 1)[-1] for q in truth})})
    if "rename" in kinds and len(sample) > 2:
        s = sample[2]
        new = s["name"] + ("_v2" if "_" in s["name"] or s["name"].islower() else "V2")
        tasks.append({"kind": "rename", "target": s["qualified"],
                      "prompt": f"Rename the function `{s['name']}` to `{new}` everywhere in the project (definition and every use). The project must still compile.",
                      "old": s["name"], "new": new})
    if "fix" in kinds:
        bug = seed_bug(repo, root, syms, rng)
        if bug:
            tasks.append(bug)
    return tasks


MUTATIONS = [(r"(?<![<>=!])<(?![<=])", "<="), (r" \+ 1\b", " - 1"), (r"(?<![=!<>])==(?!=)", "!="), (r"\btrue\b", "false"), (r"\bTrue\b", "False"),
             (r"(?<![=!<>])!=(?!=)", "=="), (r" is not None\b", " is None"), (r" is None\b", " is not None"), (r"\bfalse\b", "true"), (r"\bFalse\b", "True"),
             (r" - 1\b", " + 1"), (r" and ", " or "), (r" && ", " || ")]
MAX_SEED_RUNS = 30  # test-suite runs spent looking for a mutant that some test notices


BROKEN_BEFORE: dict[str, set] = {}  # per project: the tests that fail on the untouched code (not ours to blame)


def seed_bug(repo: dict, root: Path, syms: list[dict], rng: random.Random) -> dict | None:
    """Change one operator inside a non-test function so that some test that passed starts failing."""
    L = ev.LANG[repo["lang"]]
    if str(root) not in BROKEN_BEFORE:
        BROKEN_BEFORE[str(root)] = set(L["failed"](ev.sh(L["tests"], root, timeout=1800)[1], root))
    before = BROKEN_BEFORE[str(root)]
    funcs = [s for s in syms if s["kind"] in ("function", "method") and not s.get("test") and 4 <= s["end_line"] - s["start_line"] <= 40
             and not s["file"].startswith(("examples/", "example/", "docs/", "benches/", "bench/"))]  # rarely under test
    rng.shuffle(funcs)
    runs = 0
    for s in funcs:
        if runs >= MAX_SEED_RUNS:
            break
        p = root / s["file"]
        lines = p.read_text(errors="replace").split("\n")
        tried = 0
        for ln in range(s["start_line"], s["end_line"]):
            text = lines[ln]
            if text.lstrip().startswith(("//", "#", "*")) or tried >= 2:
                continue
            for pat, rep in MUTATIONS:
                if re.search(pat, text):
                    tried += 1
                    runs += 1
                    lines2 = list(lines)
                    lines2[ln] = re.sub(pat, rep, text, count=1)
                    p.write_text("\n".join(lines2))
                    ccode, _ = ev.sh(L["compile"], root, timeout=900)
                    failed = set()
                    if ccode == 0 or repo["lang"] == "python":
                        _, tout = ev.sh(L["tests"], root, timeout=1800)
                        failed = set(L["failed"](tout, root)) - before
                    if failed:
                        patch = ev.sh("git diff", root)[1]
                        ev.restore(root)
                        name = sorted(failed)[0]
                        return {"kind": "fix", "target": s["qualified"], "patch": patch, "prompt": f"The test `{name}` fails. Find the bug in the code and fix it (do not change the tests).", "failing": sorted(failed)[:10]}
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


def trace_of(msgs: list) -> list:
    """The conversation of one run, small enough to keep: long tool output is cut in the middle."""
    out = []
    for m in msgs:
        text = m.get("content") or ""
        if len(text) > 3000:
            text = text[:2000] + f"\n… ({len(text) - 3000} characters cut) …\n" + text[-1000:]
        row = {"role": m["role"], "content": text}
        if m.get("tool_calls"):
            row["calls"] = [f'{c["function"]["name"]} {c["function"].get("arguments", "")}'[:600] for c in m["tool_calls"]]
        out.append(row)
    return out


def run_card(card_path: Path, mode: str, host: str, steps: int, minutes: float, escalate_host: str) -> dict:
    """One task from a card file: run the agent in the card's repository, judge by the card's
    `accept` command, leave the result and the patch next to the card."""
    card = json.loads(card_path.read_text())
    root = Path(card["repo"]).expanduser().resolve()
    accept = card.get("accept") or "gitnext check"
    lang = card.get("lang", "")
    ev.gx(["index"], root)
    t0 = time.time()
    res = agent({"prompt": card["prompt"]}, mode, root, lang, host, steps, t0 + minutes * 60)
    judge = lambda: call_tool("run", {"command": accept}, root, lang)  # noqa: E731
    verdict = judge()
    res["passed"] = verdict.startswith("exit 0")
    if not res["passed"] and escalate_host and res.get("stopped"):
        up = agent({"prompt": card["prompt"], "handoff": res["handoff"]}, mode, root, lang, escalate_host, steps, time.time() + minutes * 60)
        verdict = judge()
        res["escalated"] = {"steps": up["steps"], "tokens_in": up["tokens_in"], "answer": up["answer"], "passed": verdict.startswith("exit 0")}
        res["passed"] = res["escalated"]["passed"]
    res.update({"mode": mode, "seconds": round(time.time() - t0), "accept": accept, "verdict": verdict[-1500:]})
    card_path.with_suffix(".trace.json").write_text(json.dumps(trace_of(res.pop("_msgs", [])), ensure_ascii=False, indent=1))
    patch = subprocess.run(["git", "diff", "HEAD"], cwd=root, capture_output=True, text=True, errors="replace").stdout
    card_path.with_suffix(".patch").write_text(patch)
    card_path.with_suffix(".result.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
    return res


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--task", default="", help="a task card (JSON: repo, prompt, accept) instead of the generated eval tasks")
    ap.add_argument("--mode", default="cmd", help="with --task: the mode to run it in")
    ap.add_argument("--host", default="http://127.0.0.1:8080")
    ap.add_argument("--repos", default="click,cobra")
    ap.add_argument("--tasks", default="find,callers,fix,rename")
    ap.add_argument("--learn", action="store_true", help="Python projects: `gitnext learn-tests` on the untouched code before the `fix` tasks")
    ap.add_argument("--modes", default="base,gitnext,lite")
    ap.add_argument("--escalate-host", default="", help="a stronger model's server: a run the loop guard stopped is handed to it with the notes")
    ap.add_argument("--repeat", type=int, default=1, help="task sets per repo: each takes other functions and another bug (seed, seed + 1, …)")
    ap.add_argument("--steps", type=int, default=25)
    ap.add_argument("--minutes", type=float, default=20, help="time limit per run")
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--out", default=str(HERE / "results" / "agent-latest.json"))
    args = ap.parse_args()
    if args.task:
        res = run_card(Path(args.task), args.mode, args.host, args.steps, args.minutes, args.escalate_host)
        print(f"{'✓ passed' if res['passed'] else '✗ failed'} · steps {res['steps']} · tokens {res['tokens_in']}/{res['tokens_out']} · {res['seconds']}s"
              + (f" · guard {res['guard']}" if res.get("guard") else "") + (f" · stopped: {res['stopped']}" if res.get("stopped") else "") + (" · escalated" if res.get("escalated") else ""))
        raise SystemExit(0 if res["passed"] else 1)
    repos = [r for r in json.loads((HERE / "repos.json").read_text()) if r["name"] in args.repos.split(",")]
    modes = args.modes.split(",")
    rows = []
    for repo in repos:
        root = ev.prepare(repo)
        ev.gx(["init"], root, timeout=1800)
        if args.learn and "fix" in args.tasks.split(",") and repo["lang"] == "python" and any(m in GITNEXT_MODES for m in modes):
            # what a project does once while its tests pass: `gitnext why` then knows what each test
            # really runs and which of that code changed since (the bug is put in after this)
            py = next((p for p in (".venv/bin/python", "venv/bin/python") if (root / p).exists()), "python3")
            if subprocess.run([py, "-c", "import pytest_cov"], cwd=root, capture_output=True).returncode != 0:
                subprocess.run([py, "-m", "pip", "install", "-q", "pytest-cov"], cwd=root, capture_output=True)
            print(f"{repo['name']}: " + (ev.gx(["learn-tests"], root, timeout=3600)[1].strip().splitlines() or ["learn-tests said nothing"])[0], flush=True)
        elif (root / ".gitnext" / "tests.db").exists():
            (root / ".gitnext" / "tests.db").unlink()  # a map left by another run would change what is measured
        base = ev.sh("git rev-parse HEAD", root)[1].strip()
        clean = lambda: (ev.sh(f"git reset -q --hard {base}", root), ev.restore(root))  # noqa: E731
        for rep in range(args.repeat):
            clean()
            tasks = make_tasks(repo, root, args.tasks.split(","), args.seed + rep)
            for task in tasks:
                for mode in modes:
                    clean()
                    if task["kind"] == "fix":
                        # the bug is a commit: `git diff` and `gitnext check` must not point at it
                        subprocess.run(["git", "apply"], input=task["patch"], text=True, cwd=root)
                        ev.sh("git -c user.name=eval -c user.email=eval@example.invalid -c commit.gpgsign=false commit -qam 'seeded bug'", root)
                    ev.gx(["index"], root)
                    t0 = time.time()
                    try:
                        res = agent(task, mode, root, repo["lang"], args.host, args.steps, t0 + args.minutes * 60)
                        res["passed"] = check(task, repo, root, res)
                        if res.get("stopped") and not res["passed"] and args.escalate_host:
                            # the small model got stuck: the stronger one continues from the files as they are, with its notes
                            up = agent({**task, "handoff": res["handoff"]}, mode, root, repo["lang"], args.escalate_host, args.steps, time.time() + args.minutes * 60)
                            res["escalated"] = {"steps": up["steps"], "tokens_in": up["tokens_in"], "answer": up["answer"], "passed": check(task, repo, root, up)}
                            res["passed"] = res["escalated"]["passed"]
                    except Exception as e:
                        res = {"passed": False, "error": str(e)[:300]}
                    res.update({"repo": repo["name"], "task": task["kind"], "target": task.get("target"), "mode": mode, "repeat": rep, "seconds": round(time.time() - t0)})
                    with Path(args.out).with_suffix(".trace.jsonl").open("a") as f:
                        f.write(json.dumps({k: res.get(k) for k in ("repo", "task", "target", "mode", "repeat", "passed")} | {"messages": trace_of(res.pop("_msgs", []))}, ensure_ascii=False) + "\n")
                    rows.append(res)
                    print(f"{repo['name']:8} {task['kind']:8} {mode:8} {'✓' if res['passed'] else '✗'} steps {res.get('steps')} tok {res.get('tokens_in')}/{res.get('tokens_out')} {res['seconds']}s tools {res.get('tools')}"
                          + (f" code-as-text {res['text_code']}" if res.get("text_code") else "") + (f" guard {res['guard']}" if res.get("guard") else "") + (f" gate {res['gate']}" if res.get("gate") else "") + (f" stopped: {res['stopped']}" if res.get("stopped") else "") + (" escalated" if res.get("escalated") else ""), flush=True)
                    Path(args.out).parent.mkdir(exist_ok=True)
                    Path(args.out).write_text(json.dumps(rows, ensure_ascii=False, indent=1))
        clean()
    # summary per mode
    md = ["| mode | passed | median steps | tokens in (sum) | gitnext tool calls | calls written as text | code written as text | guard stepped in | stopped by guard | sent back by the gate | escalated |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for mode in modes:
        rs = [r for r in rows if r["mode"] == mode]
        if not rs:
            continue
        steps = sorted(r.get("steps") or 0 for r in rs)
        gcalls = sum(sum(v for k, v in (r.get("tools") or {}).items() if k in ("look", "rename", "replace_symbol", "replace", "add_after", "check")) for r in rs)
        md.append(f"| {mode} | {sum(r['passed'] for r in rs)}/{len(rs)} | {steps[len(steps)//2]} | {sum(r.get('tokens_in') or 0 for r in rs)} | {gcalls} | {sum(r.get('text_calls') or 0 for r in rs)} | {sum(r.get('text_code') or 0 for r in rs)} | {sum(sum((r.get('guard') or {}).values()) for r in rs)} | {sum(1 for r in rs if r.get('stopped'))} | {sum(sum((r.get('gate') or {}).values()) for r in rs)} | {sum(1 for r in rs if r.get('escalated'))} |")
    Path(args.out).with_suffix(".md").write_text("\n".join(md) + "\n")
    print("\n".join(md))


if __name__ == "__main__":
    main()
