#!/usr/bin/env python3
"""The fix variants V2 (prove), V3 (try) and V4 (combo) without a project or a model:
git, gitnext and the test run are replaced by stand-ins.

    python3 -m unittest eval/test_fix_variants.py        (from the repository root)
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import agent  # noqa: E402

FAILS = ("why: test_x — FAILS (1s)\n  pytest tests/test_x.py::test_x\n✗ test_x  tests/test_x.py:3\n    assert f() == 1\n\n"
         "code the test ran (learned coverage), most likely first (2 of 2):\n"
         " 1. g  a.py:5  (changed since `learn-tests`)\n"
         " 2. f  a.py:1  (changed since `learn-tests`)\n\n"
         "changed in g since `learn-tests` (- as it was, + as it is now):\n  - return b < a\n  + return a > b\n"
         "next: fix `g` (a.py:5)")
SOURCE = "def f():\n    return 2\n\n\ndef g(a, b):\n    return a > b\n"
HUNKS = [{"file": "a.py", "new": 2, "count": 1, "minus": ["    return 1"], "plus": ["    return 2"]},  # the bug, in f
         {"file": "a.py", "new": 6, "count": 1, "minus": ["    return b < a"], "plus": ["    return a > b"]}]  # meant, in g
SYMS = [{"file": "a.py", "start_line": 1, "end_line": 2, "kind": "function", "qualified": "f"},
        {"file": "a.py", "start_line": 5, "end_line": 6, "kind": "function", "qualified": "g"}]


class Combo(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        (self.root / "a.py").write_text(SOURCE)
        self.tried = []

        def passes(root, test):  # the test passes when f returns 1 again
            self.tried.append((root / "a.py").read_text())
            return "return 1" in (root / "a.py").read_text(), ""
        for name, value in {"learned_commit": lambda root: "abcdef1234", "changes_since": lambda root, base: list(HUNKS),
                            "passes": passes, "symbol_at": lambda root, file, line: next((s for s in SYMS if s["start_line"] <= line <= s["end_line"]), None)}.items():
            p = mock.patch.object(agent, name, value)
            p.start()
            self.addCleanup(p.stop)
        p = mock.patch.object(agent.ev, "sh", lambda cmd, root, **k: (0, "a.py:1\nb.py:1\n"))  # `git grep`: the line is not unique
        p.start()
        self.addCleanup(p.stop)

    def test_proof_is_short_and_ordered_by_rank(self):
        out = agent.combo(self.root, "test_x", FAILS)
        # g is ranked first, so it is tried first; f is the cause
        self.assertEqual(len(self.tried), 2)
        self.assertIn("Undoing the change in `f` (a.py:2) makes it pass", out)
        self.assertIn("The other 1 change(s) are meant: keep them.", out)
        self.assertIn("def f():\n    return 1", out)
        self.assertTrue(out.endswith('next: replace(symbol="f", code=<the function above>), then check with no arguments'))
        # V1's view of the same changes is left out
        self.assertNotIn("code the test ran", out)
        self.assertNotIn("changed in g", out)
        self.assertEqual((self.root / "a.py").read_text(), SOURCE)  # every undo was put back

    def test_no_single_cause_gives_v1_view_and_try(self):
        with mock.patch.object(agent, "passes", lambda root, test: (False, "")):
            out = agent.combo(self.root, "test_x", FAILS)
        self.assertIn("changed in g since", out)
        self.assertIn("the cause is not a single change", out)
        self.assertIn("call try with two versions", out)
        self.assertTrue(out.endswith("next: fix `g` (a.py:5)"))

    def test_test_not_run_gives_v1_view_and_try(self):
        out = agent.combo(self.root, "ExampleX", "why: ExampleX — no test named ExampleX\nnext: look at it")
        self.assertIn("call try with two versions", out)
        self.assertTrue(out.endswith("next: look at it"))
        self.assertEqual(self.tried, [])


class TryVersions(unittest.TestCase):
    def count(self, variant: str) -> int:
        calls = []

        class Failed:
            returncode, stdout, stderr = 1, "not found", ""
        with mock.patch.object(agent, "VARIANT", variant), mock.patch.object(agent, "snapshot", lambda root: {}), \
                mock.patch.object(agent, "rollback", lambda root, snap: None), \
                mock.patch.object(agent.subprocess, "run", lambda *a, **k: (calls.append(a), Failed())[1]):
            agent.try_versions(Path("."), {"symbol": "f", "variants": ["a", "b", "c"]})
        return len(calls)

    def test_combo_tries_two_versions_try_three(self):
        self.assertEqual(self.count("combo"), 2)
        self.assertEqual(self.count("try"), 3)


if __name__ == "__main__":
    unittest.main()
