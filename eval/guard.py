"""Loop protection for the agent loop (eval/agent.py).

A small model gets stuck in ways a large one rarely does: it repeats the same call,
flips between two versions of a file, retries a fix that fails with the same error, or
fills its answer with one repeated line. Each of those burns the whole step budget and
ends in nothing. `Guard` watches the run and steps in:

  repeat        the same call with the same arguments, nothing changed since → not run
                again; the model gets the earlier result and is told to do something else
  no progress   N steps without new information or a change in the files → one nudge,
                then stop
  same error    the same build / test error K times → a hint from `gitnext locate` at the
                second, stop at the K-th
  see-saw       edit → undo → the same edit → the files go back to the last state where
                the tests passed (or to the start)
  repetition    a repeated line or chunk in the model's text is cut (`trim_repeats`)
  budget        a soft limit ("wrap up") and a hard one, in steps and in tokens
  history       old tool output is replaced by a one-line digest (`compress`)
  hand-off      when the run is stopped: what was tried, what changed, the last error —
                for a stronger model or a person (`handoff`)

Nothing here needs a model: `python3 -m unittest eval/test_guard.py`.
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
from pathlib import Path

NO_PROGRESS_STEPS = 4   # steps without anything new before the nudge (and again before the stop)
SAME_ERROR_LIMIT = 3    # the same error this many times: stop
SOFT_SHARE = 0.7        # of the step / token budget: "wrap up"
CHANGING = {"edit_file", "rename", "replace_symbol", "run", "sh"}  # tools that may change files
RUNNING = {"run", "sh"}                                              # tools whose output starts with `exit N`
VERDICT = {"check"}                                                  # gitnext's own build + tests verdict


def _git(root: Path, *args: str, stdin: str | None = None) -> str:
    try:
        p = subprocess.run(["git", *args], cwd=root, input=stdin, capture_output=True, text=True, errors="replace", timeout=60)
        return p.stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def is_test_command(cmd: str) -> bool:
    """Whether a shell command runs a test suite (its success is what "the code works" means)."""
    return bool(re.search(r"\b(gitnext check|pytest|unittest|go test|cargo (nextest|test)|mvn\b.*\btest|gradlew?\b.*\btest|npm (run )?test|yarn test|pnpm test|vitest|jest|ctest|make (test|check))\b", cmd))


def call_key(name: str, args: dict) -> str:
    return name + " " + json.dumps(args, sort_keys=True, ensure_ascii=False)


def error_signature(out: str) -> str:
    """What makes two failures "the same": the first error lines, without numbers and paths."""
    lines = [l.strip() for l in out.split("\n") if re.search(r"\b(error|Error|ERROR|FAILED|FAIL|panicked|Traceback|AssertionError|assert|undefined|cannot find|not found)\b", l)]
    sig = []
    for l in lines[:3]:
        l = re.sub(r"0x[0-9a-fA-F]+|\d+", "N", l)
        l = re.sub(r"\s+", " ", l)
        sig.append(l[:160])
    return " | ".join(sig)


def trim_repeats(text: str) -> tuple[str, bool]:
    """Cut a model's answer where it starts repeating itself: the same line again and again,
    or one chunk of text looped until the token limit."""
    cut = False
    out: list[str] = []
    run = 0
    for line in text.split("\n"):
        if out and line.strip() and line == out[-1]:
            run += 1
            if run >= 2:  # the third copy and on
                cut = True
                continue
        else:
            run = 0
        out.append(line)
    text = "\n".join(out)
    tail = text[-4000:]
    m = re.search(r"(.{20,400}?)\1{3,}\s*$", tail, re.S)
    if m:
        text = text[: len(text) - len(tail) + m.start()] + m.group(1)
        cut = True
    return (text + "\n… (repetition cut)" if cut else text), cut


def compress(msgs: list, limit: int, keep_last: int = 2) -> None:
    """Replace old tool output by one line (what was asked, how it began, how long it was)
    until the conversation fits `limit` characters. The newest results stay whole."""
    size = lambda: sum(len(m.get("content") or "") + len(json.dumps(m.get("tool_calls") or "")) for m in msgs)
    if size() <= limit:
        return
    asked = {}
    for m in msgs:
        for c in m.get("tool_calls") or []:
            asked[c.get("id")] = f'{c["function"]["name"]} {c["function"].get("arguments", "")[:80]}'
    outputs = [m for m in msgs if m["role"] == "tool" or (m["role"] == "user" and m.get("content", "").startswith("exit "))]
    for m in outputs[: max(len(outputs) - keep_last, 0)]:
        if size() <= limit:
            return
        text = m["content"]
        if len(text) <= 300 or text.startswith("[old output"):
            continue
        first = next((l.strip() for l in text.split("\n") if l.strip()), "")[:160]
        m["content"] = f"[old output{' of ' + asked[m['tool_call_id']] if m.get('tool_call_id') in asked else ''}: {first} … {text.count(chr(10)) + 1} lines]"


class Guard:
    def __init__(self, root: Path, max_steps: int, token_budget: int = 0, locate=None):
        self.root, self.max_steps, self.token_budget = root, max_steps, token_budget
        self.locate = locate              # error text → where gitnext thinks it comes from ("" if nothing)
        self.calls: dict[str, tuple[int, str, str]] = {}   # call → (step, state of the files then, its output)
        self.outputs: set[str] = set()
        self.state = self.fingerprint()
        self.states = [self.state]        # after every change of the files
        self.green = self.snapshot()      # the files when the tests last passed; at first — the start
        self.green_is_start = True
        self.swings = 0
        self.errors: dict[str, int] = {}
        self.idle = 0
        self.nudged = False
        self.soft_said = False
        self.progress = False             # anything new in the current step
        self.stop: str | None = None      # why the run must end
        self.stats = {"repeats": 0, "no_progress": 0, "same_error": 0, "seesaw": 0, "trimmed": 0, "soft_budget": 0}
        self.log: list[str] = []          # one line per distinct action, for the hand-off
        self.last_error = ""

    # -------- the files

    def fingerprint(self) -> str:
        return hashlib.sha1((_git(self.root, "status", "--porcelain") + _git(self.root, "diff", "HEAD")).encode()).hexdigest()

    def snapshot(self) -> str:
        return _git(self.root, "diff", "HEAD", "--binary")

    def restore(self, patch: str) -> None:
        _git(self.root, "checkout", "-q", "--", ".")
        if patch.strip():
            _git(self.root, "apply", "--whitespace=nowarn", "-", stdin=patch)

    # -------- one call

    def before_call(self, step: int, name: str, args: dict) -> str | None:
        """The text to give the model instead of running the call, or None to run it."""
        prev = self.calls.get(call_key(name, args))
        if name == "finish" or prev is None or prev[1] != self.state:
            return None
        self.stats["repeats"] += 1
        return (f"Not run again: you made this exact call at step {prev[0]} and the files are exactly as they were then. It returned:\n"
                f"{prev[2][:600]}\nDo something different: look at another function or file, make an edit, or finish with what you know.")

    def after_call(self, step: int, name: str, args: dict, out: str) -> str:
        """Record the result; returns the output, with a note when the guard stepped in."""
        key = call_key(name, args)
        if key not in self.calls:
            self.log.append(f"step {step}: {key[:140]}")
        before = self.state
        h = hashlib.sha1(out.encode()).hexdigest()
        if h not in self.outputs:
            self.outputs.add(h)
            self.progress = True
        if name in CHANGING:
            fp = self.fingerprint()
            if fp != self.state:
                self.progress = True
                self.state = fp
                if len(self.states) >= 2 and fp == self.states[-2]:
                    self.swings += 1
                self.states.append(fp)
                if self.swings >= 2:  # A → B → A → B: back to solid ground
                    self.restore(self.green)
                    self.state = self.fingerprint()
                    self.states, self.swings = [self.state], 0
                    self.stats["seesaw"] += 1
                    where = "the start of the task" if self.green_is_start else "the last state where the tests passed"
                    out += (f"\n\nNOTE: you keep switching between the same two versions of the code. The files were put back to {where}. "
                            "Neither version works — try a different change.")
        self.calls[key] = (step, before, out)
        if name in VERDICT:  # `gitnext check`: its verdict is the tests' word
            out = ("exit 0\n" if "verdict: ok" in out else "exit 1\n") + out if "verdict:" in out else out
        if name in RUNNING or name in VERDICT:
            code = re.match(r"exit (-?\d+)", out)
            if code and code.group(1) == "0":
                if name in VERDICT or is_test_command(args.get("command", "")):
                    self.green, self.green_is_start = self.snapshot(), False
            elif code:
                sig = error_signature(out)
                if sig:
                    self.last_error = sig
                    n = self.errors[sig] = self.errors.get(sig, 0) + 1
                    if n == 2:
                        self.stats["same_error"] += 1
                        hint = (self.locate(out[-2000:]) if self.locate else "").strip()
                        out += "\n\nNOTE: this is the same error as before — the last change did not fix it." + (f" gitnext points at:\n{hint[:800]}" if hint else " Read the error again and look at the code it names.")
                    elif n >= SAME_ERROR_LIMIT:
                        self.stats["same_error"] += 1
                        self.stop = "same_error"
        return out

    # -------- one step

    def end_step(self, step: int, tokens: int) -> str | None:
        """Call after every model turn. Returns a message to add to the conversation, if any;
        sets `stop` when the run must end."""
        idle, self.progress = not self.progress, False
        self.idle = self.idle + 1 if idle else 0
        if self.token_budget and tokens >= self.token_budget:
            self.stop = self.stop or "budget"
            return None
        if self.idle >= NO_PROGRESS_STEPS:
            self.stats["no_progress"] += 1
            self.idle = 0
            if self.nudged:
                self.stop = self.stop or "no_progress"
                return None
            self.nudged = True
            return (f"The last {NO_PROGRESS_STEPS} steps brought nothing new and changed nothing. Stop repeating them. "
                    "Either finish now with what you know, or take a step you have not taken yet.")
        if not self.soft_said and (step >= SOFT_SHARE * self.max_steps or (self.token_budget and tokens >= SOFT_SHARE * self.token_budget)):
            self.soft_said = True
            self.stats["soft_budget"] = 1
            return f"You have used {step} of {self.max_steps} steps. Wrap up: make the final change if one is left, then finish with your answer."
        return None

    # -------- when it is over

    def handoff(self, task: str) -> str:
        """What a stronger model or a person needs to pick the task up."""
        changed = _git(self.root, "diff", "HEAD", "--stat").strip()
        return "\n".join([
            f"Task: {task}",
            f"Stopped: {self.stop or 'ran out of steps'}.",
            "Tried (distinct actions):", *[f"  {l}" for l in self.log[-15:]],
            f"Files changed so far:\n{changed}" if changed else "No file was changed.",
            f"Last error: {self.last_error}" if self.last_error else "No failing command was seen.",
        ])
