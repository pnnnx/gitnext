#!/usr/bin/env python3
"""Baselines on the functions of a run: what the plain search for a name gives where gitnext was asked.

The truth is the one the run kept (the compiler's uses, the tests that fail when the function throws); nothing is
compiled or run here, only `git grep` over the same commit.

  callers  the functions that have the name as a word (`git grep -w`), each hit put into the function around it
  tests    the tests whose body has the name as a word, in the order of the files — and ten tests drawn at random

    python3 eval/baselines.py [run]      # a name in eval/results (default: latest) → results/<run>.baselines.{md,json}
"""
import json
import math
import random
import subprocess
import sys
from pathlib import Path

import run as ev

EXT = {"go": ["*.go"], "rust": ["*.rs"], "java": ["*.java"], "typescript": ["*.ts", "*.tsx", "*.mts"], "python": ["*.py"]}
TOP = ev.TOP
BOOT = 2000


def grep(root: Path, name: str, lang: str) -> list[tuple[str, int]]:
    """Every line that has the name as a word: (file, line)."""
    out = subprocess.run(["git", "grep", "-nwF", "--no-color", "-e", name, "--", *EXT[lang]], cwd=root, capture_output=True, text=True).stdout
    hits = []
    for row in out.splitlines():
        file, line, _ = row.split(":", 2)
        hits.append((file, int(line)))
    return hits


def half(values: list[float]) -> float:
    """Half the width of the 90% bootstrap interval of the mean."""
    if len(values) < 2:
        return 0.0
    rng = random.Random(1)
    means = sorted(sum(rng.choice(values) for _ in values) / len(values) for _ in range(BOOT))
    return (means[int(0.95 * BOOT) - 1] - means[int(0.05 * BOOT)]) / 2


def cell(values: list) -> str:
    v = [x for x in values if x is not None]
    return f"{100 * sum(v) / len(v):.0f}% ±{100 * half(v):.0f}" if v else "—"


def callers_rows(root: Path, repo: dict, syms: list[dict], rows: list[dict], hits_of=None, prefix: str = "grep") -> list[dict]:
    """`hits_of(symbol)`: the places (file, line) an answerer names for the symbol; the default is the search for its name."""
    L = ev.LANG[repo["lang"]]
    by_name = {x["qualified"]: x for x in syms}
    modules = {x["qualified"] for x in syms if x["kind"] == "module"}
    file_of = {x["qualified"]: x["file"] for x in syms}
    unseen = L.get("unseen", lambda f: False)
    out = []
    for r in rows:
        if "truth" not in r:
            continue
        truth, imports = set(r["truth"]), set(r["import_files"])
        name = r["symbol"].rsplit("::", 1)[-1]
        hits = hits_of(by_name[r["symbol"]]) if hits_of else grep(root, name, repo["lang"])
        if hits is None:
            continue
        found = {q for f, ln in hits if (q := ev.enclosing(syms, f, ln)) and q != r["symbol"] and q not in modules}
        found = {q for q in found if not unseen(file_of.get(q, ""))}
        ok = {q for q in found if q in truth or file_of.get(q) in imports}
        out.append({
            "symbol": r["symbol"], "lang": repo["lang"],
            f"{prefix}_recall": len(found & truth) / len(truth) if truth else None, f"{prefix}_precision": len(ok) / len(found) if found else None,
            "recall": r["recall"], "precision": r["precision"],
        })
    return out


def tests_rows(root: Path, repo: dict, syms: list[dict], rows: list[dict], truth_of) -> list[dict]:
    L = ev.LANG[repo["lang"]]
    # the tests of the project: what the index marks as test code, narrowed by the language's own naming where it has one
    code, listed = ev.gx(["--json", "symbols", "--tests"], root)
    marked = [s for s in (json.loads(listed) if code == 0 else []) if s.get("test")]
    is_test = {"go": lambda s: s["name"].startswith("Test"), "python": lambda s: s["name"].startswith("test")}.get(repo["lang"], lambda s: True)
    tests = [s for s in marked if s["kind"] in ("function", "method") and is_test(s)]
    test_files = sorted({s["file"] for s in marked})
    out = []
    for r in rows:
        if "truth" not in r:
            continue
        t = truth_of(f"{repo['name']}@{repo['commit'][:12]}:{L.get('tests_key', 'tests')}:{r['symbol']}")
        if not t:
            continue
        key = {"file": lambda s: s["file"], "id": lambda s: f"{s['file']}::{s['name']}", "name": lambda s: s["name"]}[r["level"]]
        truth = set(t["ids"] if r["level"] == "id" else t["failed"])
        universe = set(test_files) if r["level"] == "file" else {key(s) for s in tests}
        name = r["symbol"].rsplit("::", 1)[-1]
        lines: dict[str, set[int]] = {}
        for f, ln in grep(root, name, repo["lang"]):
            lines.setdefault(f, set()).add(ln)
        named = sorted((s for s in tests if any(s["start_line"] <= ln <= s["end_line"] for ln in lines.get(s["file"], ()))), key=lambda s: (s["file"], s["start_line"]))
        order = [f for f in test_files if f in lines] if r["level"] == "file" else list(dict.fromkeys(key(s) for s in named))
        hit = len(set(order) & truth)
        total, k = max(len(universe), len(truth)), len(truth)
        # at least one of ten tests drawn at random fails
        chance = 1 - math.comb(total - k, TOP) / math.comb(total, TOP) if total - k >= TOP else 1.0
        out.append({
            "symbol": r["symbol"], "lang": repo["lang"], "tests_in_project": total, "failing": k,
            "grep_recall": hit / k, "grep_precision": hit / len(order) if order else 0.0, "grep_caught": 1.0 if set(order[:TOP]) & truth else 0.0,
            "random_caught": chance, "random_precision": k / total, "recall": r["recall"], "precision": r["precision"], "caught": r["caught"],
        })
    return out


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "latest"
    results = [r for r in json.loads((ev.HERE / "results" / f"{name}.json").read_text()) if "error" not in r]
    ev._load_cache()
    cache = ev.CACHE if hasattr(ev, "CACHE") else json.loads(ev.CACHE_FILE.read_text())
    all_rows = {"callers": [], "callers_common": [], "callers_blind": [], "tests": []}
    for r in results:
        repo = {"name": r["repo"], "lang": r["lang"], "commit": r["commit"]}
        root = ev.WORK / r["repo"]
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True).stdout.strip()
        if not head.startswith(r["commit"]):
            print(f"{r['repo']}: the checkout is at {head[:12]}, the run was at {r['commit'][:12]} — skipped")
            continue
        syms = ev.symbols(root)
        for scope in ("callers", "callers_common", "callers_blind"):
            got = callers_rows(root, repo, syms, r.get(scope, []))
            for g in got:
                g["repo"] = r["repo"]
            all_rows[scope] += got
        got = tests_rows(root, repo, syms, r.get("tests", []), cache.get)
        for g in got:
            g["repo"] = r["repo"]
        all_rows["tests"] += got
        print(f"{r['repo']}: " + ", ".join(f"{k} {sum(1 for x in v if x['repo'] == r['repo'])}" for k, v in all_rows.items()), flush=True)

    langs = [l for l in ("go", "rust", "java", "typescript", "python") if any(x["lang"] == l for v in all_rows.values() for x in v)]
    md = [f"# Baselines on the functions of the run `{name}`", "",
          "The search for the name: `git grep -w <name>` in the files of the language; every hit is put into the function around it "
          "(callers) or counted for the test whose body has it (tests). The truth is the run's own: the compiler's uses, the tests that "
          "fail when the function throws. ± is half of a 90% bootstrap interval over the functions.", ""]
    titles = {"callers": "Callers — names that occur once", "callers_common": "Callers — methods that share a name",
              "callers_blind": "Callers — whether or not gitnext sees a caller"}
    for scope, title in titles.items():
        rows = all_rows[scope]
        if not rows:
            continue
        md += [f"## {title}", "", "| | functions | search: recall | search: precision | gitnext: recall | gitnext: precision |", "|---|---|---|---|---|---|"]
        for label, part in [("all", rows)] + [(l, [x for x in rows if x["lang"] == l]) for l in langs]:
            if part:
                md.append(f"| {label} | {sum(x['recall'] is not None for x in part)} | {cell([x['grep_recall'] for x in part])} | {cell([x['grep_precision'] for x in part])} | "
                          f"{cell([x['recall'] for x in part])} | {cell([x['precision'] for x in part])} |")
        md.append("")
    rows = all_rows["tests"]
    if rows:
        md += ["## The tests of a function", "",
               f"\"Caught\": at least one of the first {TOP} tests named fails when the function throws. \"Ten at random\": the chance of "
               "that for ten tests drawn from all tests of the project; their precision is the share of the project's tests that fail.", "",
               "| | functions | ten at random: caught | ten at random: precision | search: caught | search: recall | search: precision | gitnext: caught | gitnext: recall | gitnext: precision |",
               "|---|---|---|---|---|---|---|---|---|---|"]
        for label, part in [("all", rows)] + [(l, [x for x in rows if x["lang"] == l]) for l in langs]:
            if part:
                md.append(f"| {label} | {len(part)} | {cell([x['random_caught'] for x in part])} | {cell([x['random_precision'] for x in part])} | {cell([x['grep_caught'] for x in part])} | "
                          f"{cell([x['grep_recall'] for x in part])} | {cell([x['grep_precision'] for x in part])} | {cell([x['caught'] for x in part])} | "
                          f"{cell([x['recall'] for x in part])} | {cell([x['precision'] for x in part])} |")
        md.append("")
    text = "\n".join(md)
    (ev.HERE / "results" / f"{name}.baselines.md").write_text(text)
    (ev.HERE / "results" / f"{name}.baselines.json").write_text(json.dumps(all_rows, ensure_ascii=False, indent=1))
    print("\n" + text)


if __name__ == "__main__":
    main()
