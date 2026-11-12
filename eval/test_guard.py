#!/usr/bin/env python3
"""The loop guard and the agent loop against a scripted "model" — no real model needed.

    python3 -m unittest eval/test_guard.py        (from the repository root)
"""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import agent  # noqa: E402
import guard  # noqa: E402

agent.ev.GITNEXT = "/bin/true"  # `gitnext context` / `locate` answer nothing: the loop itself is under test


def call(name: str, **args) -> dict:
    return {"role": "assistant", "content": "", "tool_calls": [{"id": f"c{time.time_ns()}", "type": "function", "function": {"name": name, "arguments": json.dumps(args)}}]}


def say(text: str) -> dict:
    return {"role": "assistant", "content": text}


class Script:
    """An OpenAI-compatible server that answers with the next scripted message."""

    def __init__(self, replies: list[dict]):
        self.replies, self.requests = list(replies), []
        outer = self

        class H(BaseHTTPRequestHandler):
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                outer.requests.append(body)
                msg = outer.replies.pop(0) if outer.replies else say("")
                data = json.dumps({"choices": [{"message": msg}], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *a):
                pass

        self.server = HTTPServer(("127.0.0.1", 0), H)
        self.host = f"http://127.0.0.1:{self.server.server_port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def seen(self) -> str:
        """Everything the model was shown, as one string."""
        return json.dumps(self.requests[-1]["messages"], ensure_ascii=False) if self.requests else ""

    def close(self):
        self.server.shutdown()


class Case(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "calc.py").write_text("def add(a, b):\n    return a + b\n")
        (self.root / "notes.txt").write_text("one\ntwo\n")
        for c in (["init", "-q"], ["add", "-A"], ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "start"]):
            subprocess.run(["git", *c], cwd=self.root, check=True, capture_output=True)

    def tearDown(self):
        self.tmp.cleanup()

    def run_agent(self, replies, mode="base", steps=25):
        s = Script(replies)
        try:
            res = agent.agent({"prompt": "do the task"}, mode, self.root, "python", s.host, steps, time.time() + 120)
        finally:
            s.close()
        return res, s

    # ---- repeated call

    def test_same_call_is_not_run_twice(self):
        res, s = self.run_agent([call("grep", pattern="add"), call("grep", pattern="add"), call("grep", pattern="add"), call("finish", answer="calc.py")])
        self.assertEqual(res["guard"]["repeats"], 2)
        self.assertIn("Not run again", s.seen())
        self.assertEqual(res["answer"], "calc.py")
        self.assertNotIn("stopped", res)

    def test_same_call_runs_again_after_a_change(self):
        res, _ = self.run_agent([call("run", command="cat calc.py"), call("edit_file", path="calc.py", old="a + b", new="a - b"),
                                 call("run", command="cat calc.py"), call("finish", answer="ok")])
        self.assertNotIn("repeats", res["guard"])

    # ---- no progress

    def test_no_progress_nudges_then_stops(self):
        res, s = self.run_agent([call("read_file", path="notes.txt")] * 12)
        self.assertEqual(res["stopped"], "no_progress")
        self.assertEqual(res["steps"], 1 + 2 * guard.NO_PROGRESS_STEPS)
        self.assertIn("brought nothing new", s.seen())
        self.assertIn("Stopped: no_progress", res["handoff"])
        self.assertIn("read_file", res["handoff"])

    # ---- the same error again

    def test_same_error_hint_then_stop(self):
        fail = "python3 -c \"raise ValueError('boom')\""
        replies = []
        for i in range(5):
            replies += [call("edit_file", path="notes.txt", old="one" if i == 0 else f"v{i - 1}", new=f"v{i}"), call("run", command=fail)]
        res, s = self.run_agent(replies)
        self.assertEqual(res["stopped"], "same_error")
        self.assertEqual(res["tools"]["run"], guard.SAME_ERROR_LIMIT)
        self.assertIn("same error as before", s.seen())
        self.assertIn("ValueError", res["handoff"])

    # ---- see-saw

    def test_seesaw_goes_back_to_the_start(self):
        flip = call("edit_file", path="calc.py", old="a + b", new="a - b")
        flop = call("edit_file", path="calc.py", old="a - b", new="a + b")
        # the same two versions reached by other means (an identical call would be caught as a repeat)
        flip2 = call("run", command="sed -i 's/a + b/a - b/' calc.py")
        mark = call("edit_file", path="notes.txt", old="one", new="uno")  # an edit before the see-saw, never tested: undone too
        res, s = self.run_agent([mark, flip, flop, flip2, call("finish", answer="gave up")])
        self.assertEqual(res["guard"]["seesaw"], 1)
        self.assertIn("same two versions", s.seen())
        self.assertEqual((self.root / "notes.txt").read_text(), "one\ntwo\n")
        self.assertEqual((self.root / "calc.py").read_text(), "def add(a, b):\n    return a + b\n")

    def test_seesaw_goes_back_to_the_last_passing_state(self):
        flip = call("edit_file", path="calc.py", old="a + b", new="a - b")
        flop = call("edit_file", path="calc.py", old="a - b", new="a + b")
        flip2 = call("run", command="sed -i 's/a + b/a - b/' calc.py")
        res, _ = self.run_agent([call("edit_file", path="notes.txt", old="one", new="uno"), call("run", command="grep -q uno notes.txt  # stands in for pytest"),
                                 flip, flop, flip2, call("finish", answer="x")])
        self.assertEqual(res["guard"]["seesaw"], 1)
        self.assertEqual((self.root / "notes.txt").read_text(), "uno\ntwo\n")  # the state the passing tests saw
        self.assertEqual((self.root / "calc.py").read_text(), "def add(a, b):\n    return a + b\n")

    def test_identical_edit_in_the_same_state_is_a_repeat(self):
        flip = call("edit_file", path="calc.py", old="a + b", new="a - b")
        flop = call("edit_file", path="calc.py", old="a - b", new="a + b")
        res, s = self.run_agent([flip, flop, flip, call("finish", answer="x")])
        self.assertEqual(res["guard"]["repeats"], 1)
        self.assertEqual((self.root / "calc.py").read_text(), "def add(a, b):\n    return a + b\n")

    # ---- budgets

    def test_soft_budget_asks_to_wrap_up(self):
        res, s = self.run_agent([call("grep", pattern=f"p{i}") for i in range(8)] + [call("finish", answer="done")], steps=10)
        self.assertEqual(res["guard"]["soft_budget"], 1)
        self.assertIn("Wrap up", s.seen())
        self.assertEqual(res["answer"], "done")

    def test_hard_token_budget_stops(self):
        agent.TOKEN_BUDGET = 350  # the scripted server reports 100 prompt tokens per turn
        try:
            res, _ = self.run_agent([call("grep", pattern=f"p{i}") for i in range(10)])
        finally:
            agent.TOKEN_BUDGET = 0
        self.assertEqual(res["stopped"], "budget")
        self.assertEqual(res["steps"], 4)

    # ---- repeated text, history

    def test_repeated_text_is_cut(self):
        text, cut = guard.trim_repeats("I will check the file.\n" * 40)
        self.assertTrue(cut)
        self.assertEqual(text.count("I will check the file."), 2)
        text, cut = guard.trim_repeats("Let me think. " + "the answer is in calc.py, " * 30)
        self.assertTrue(cut)
        self.assertLess(len(text), 120)
        self.assertEqual(guard.trim_repeats("def f():\n    pass\n\n\ndef g():\n    pass\n"), ("def f():\n    pass\n\n\ndef g():\n    pass\n", False))

    def test_old_output_becomes_one_line(self):
        msgs = [{"role": "system", "content": "s"}, {"role": "user", "content": "task"}]
        for i in range(6):
            msgs.append({"role": "assistant", "content": "", "tool_calls": [{"id": f"c{i}", "function": {"name": "read_file", "arguments": json.dumps({"path": f"f{i}.py"})}}]})
            msgs.append({"role": "tool", "tool_call_id": f"c{i}", "content": f"line of file {i}\n" * 200})
        guard.compress(msgs, 9000)
        outs = [m["content"] for m in msgs if m["role"] == "tool"]
        self.assertTrue(outs[0].startswith("[old output of read_file"))
        self.assertIn("f0.py", outs[0])
        self.assertIn("200 lines", outs[0].replace("201", "200"))
        self.assertEqual(outs[-1], "line of file 5\n" * 200)  # the newest stays whole
        self.assertLessEqual(sum(len(m.get("content") or "") for m in msgs), 9000 + 600)

    # ---- command mode

    def test_command_mode_runs_fenced_commands(self):
        res, s = self.run_agent([say("```sh\ngrep -n add calc.py\n```"), say("I think it is there."), say("DONE: calc.py line 1")], mode="cmd")
        self.assertEqual(res["answer"], "calc.py line 1")
        self.assertEqual(res["tools"], {"sh": 1})
        self.assertNotIn("tools", s.requests[0])  # no tool-calling API in this mode
        self.assertIn("1:def add", s.seen())
        self.assertIn("one command in a ```sh block", s.seen())

    def test_command_mode_repeat_is_caught(self):
        res, s = self.run_agent([say("```sh\ncat notes.txt\n```")] * 3 + [say("DONE: two lines")], mode="cmd")
        self.assertEqual(res["guard"]["repeats"], 2)
        self.assertEqual(res["answer"], "two lines")


    # ---- the gate of `verify` mode

    def fake_gitnext(self) -> str:
        """`gitnext check` passes once calc.py subtracts; every other command says nothing."""
        g = self.root.parent / f"gitnext-{self.root.name}.sh"
        g.write_text("#!/bin/sh\nif [ \"$1\" = check ]; then grep -q 'a - b' calc.py && echo 'verdict: ok' || { echo 'calc.py: test_add failed'; echo 'verdict: failed'; }; fi\n")
        g.chmod(0o755)
        self.addCleanup(g.unlink)
        self.addCleanup(setattr, agent.ev, "GITNEXT", agent.ev.GITNEXT)
        agent.ev.GITNEXT = str(g)
        return str(g)

    def test_verify_sends_back_an_answer_given_without_a_look(self):
        self.fake_gitnext()
        res, s = self.run_agent([say("it is in calc.py"), call("grep", pattern="add"), say("calc.py"), say("calc.py")], mode="verify")
        self.assertEqual(res["gate"], {"tool": 1, "nochange": 1})
        self.assertEqual(res["answer"], "calc.py")
        self.assertIn("You have not looked at the code yet", s.seen())
        self.assertIn("No file was changed", s.seen())

    def test_verify_ends_only_when_the_check_passes(self):
        self.fake_gitnext()
        res, s = self.run_agent([call("edit_file", path="calc.py", old="a + b", new="a * b"), call("finish", answer="fixed"),
                                 call("edit_file", path="calc.py", old="a * b", new="a - b"), call("finish", answer="fixed now")], mode="verify")
        self.assertEqual(res["gate"], {"check": 1})
        self.assertEqual(res["answer"], "fixed now")
        self.assertIn("The check of your change failed", s.seen())
        self.assertIn("test_add failed", s.seen())
        self.assertIn("look", [t["function"]["name"] for t in s.requests[0]["tools"]])  # the gitnext tools are there

    def test_other_modes_have_no_gate(self):
        res, _ = self.run_agent([say("it is in calc.py")], mode="gitnext")
        self.assertEqual(res["answer"], "it is in calc.py")
        self.assertNotIn("gate", res)

    # ---- a task card

    def test_task_card_is_judged_by_its_accept_command(self):
        card = self.root.parent / f"card-{self.root.name}.json"
        card.write_text(json.dumps({"repo": str(self.root), "prompt": "make add subtract", "accept": "grep -q 'a - b' calc.py"}))
        s = Script([say("```sh\nsed -i 's/a + b/a - b/' calc.py\n```"), say("DONE: changed")])
        try:
            res = agent.run_card(card, "cmd", s.host, 10, 2, "")
        finally:
            s.close()
        self.assertTrue(res["passed"])
        self.assertIn("-    return a + b", card.with_suffix(".patch").read_text())
        self.assertTrue(json.loads(card.with_suffix(".result.json").read_text())["passed"])
        # the same card with a model that does nothing fails the accept command
        subprocess.run(["git", "checkout", "-q", "--", "."], cwd=self.root, check=True)
        s = Script([say("DONE: nothing to do")])
        try:
            res = agent.run_card(card, "cmd", s.host, 10, 2, "")
        finally:
            s.close()
        self.assertFalse(res["passed"])
        self.assertEqual(json.loads(card.with_suffix(".trace.json").read_text())[-1]["content"], "DONE: nothing to do")
        for f in (card, card.with_suffix(".patch"), card.with_suffix(".result.json"), card.with_suffix(".trace.json")):
            f.unlink()


if __name__ == "__main__":
    unittest.main()
