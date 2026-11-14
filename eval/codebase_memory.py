#!/usr/bin/env python3
"""Another navigation tool on the functions of a run: Codebase-Memory (github.com/DeusData/codebase-memory-mcp),
a Tree-sitter graph served through the Model Context Protocol. It is asked who calls each sampled function and
judged by the run's own truth, the compiler's uses — as gitnext, the name search and the language servers are.

The question: the `CALLS` edges into the function's node, taken with the tool's graph query
(`cli query_graph`); the node is the one with the function's name in the function's file, nearest to its line.
Each caller it names (name, file, line) is put onto the function gitnext has there, so that both tools are
compared in the same units.

    CBM_BIN=<the binary> python3 eval/codebase_memory.py [run]     # → results/<run>.codebase-memory.{md,json}
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import run as ev
from baselines import callers_rows, cell

BIN = os.environ.get("CBM_BIN", "codebase-memory-mcp")


def tool(name: str, args: dict, timeout: int = 900) -> str:
    return subprocess.run([BIN, "cli", "--quiet", name, json.dumps(args)], capture_output=True, text=True, timeout=timeout, errors="replace").stdout


def index(root: Path) -> str | None:
    out = tool("index_repository", {"repo_path": str(root)})
    m = re.search(r'"project"\s*:\s*"([^"]+)"', out)
    return m.group(1) if m else None


def hits_of(project: str, root: Path, syms: list[dict]):
    by_file: dict[str, list[dict]] = {}
    for s in syms:
        if s["kind"] != "module":
            by_file.setdefault(s["file"], []).append(s)

    def mine(name: str, file: str, line: int) -> dict | None:
        """The function gitnext has for a node of the other tool: the same name in the same file, nearest by line."""
        same = [s for s in by_file.get(file, []) if s["name"] == name]
        return min(same, key=lambda s: min(abs(s["start_line"] - line), abs(s["start_line"] + ev._def_offset(root, s) - line))) if same else None

    def hits(s: dict):
        q = (f"MATCH (c)-[:CALLS]->(f) WHERE f.name = '{s['name']}' AND f.file_path = '{s['file']}' "
             "RETURN c.name, c.file_path, c.start_line, f.start_line LIMIT 5000")
        out = tool("query_graph", {"project": project, "query": q})
        if not re.search(r"^rows: \d+", out, re.M):
            return None  # the tool has no answer (an error), which is not "no callers"
        if "has_more: true" in out:
            print(f"  {s['qualified']}: the answer is cut short", flush=True)
        # long paths come shortened: a table of prefixes first ("rows_refs"), then `@N+suffix` in the rows
        prefix = dict(re.findall(r"^  (\d+) (\S+)$", out[: out.index("\nrows: ")], re.M)) if out.startswith("rows_refs") else {}

        def path(p: str) -> str:
            m = re.match(r"@(\d+)(?:\+(.*))?$", p)
            return prefix[m.group(1)] + (m.group(2) or "") if m and m.group(1) in prefix else p

        rows = []
        for row in out[out.index("rows: "):].splitlines()[1:]:
            m = re.match(r'\s+(\S+) (\S+) "(\d+)" "(\d+)"$', row)
            if m:
                rows.append((m.group(1), path(m.group(2)), int(m.group(3)), int(m.group(4))))
        # the node must be this function: another thing of the same name in the file is another node
        rows = [r for r in rows if s["start_line"] <= r[3] <= s["end_line"]]
        if not rows:
            return []
        at = s["start_line"] + ev._def_offset(root, s)
        node = min({r[3] for r in rows}, key=lambda ln: abs(ln - at))
        found = []
        for name, file, line, target in rows:
            if target != node:
                continue
            c = mine(name, file, line)
            if c:
                found.append((c["file"], c["start_line"] + ev._def_offset(root, c)))
            else:
                found.append((file, line))  # no such function for gitnext: whatever stands at that line
        return found
    return hits


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "latest"
    results = [r for r in json.loads((ev.HERE / "results" / f"{name}.json").read_text()) if "error" not in r]
    version = subprocess.run([BIN, "--version"], capture_output=True, text=True).stdout.strip()
    all_rows = {"callers": [], "callers_common": [], "callers_blind": []}
    for r in results:
        if not any(r.get(k) for k in all_rows):
            continue
        repo = {"name": r["repo"], "lang": r["lang"], "commit": r["commit"]}
        root = ev.WORK / r["repo"]
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True).stdout.strip()
        if not head.startswith(r["commit"]):
            print(f"{r['repo']}: the checkout is at {head[:12]}, the run was at {r['commit'][:12]} — skipped")
            continue
        ev.gx(["init"], root, timeout=1800)
        syms = ev.symbols(root)
        project = index(root)
        if not project:
            print(f"{r['repo']}: the other tool did not index it — skipped")
            continue
        ask = hits_of(project, root, syms)
        for scope in all_rows:
            got = callers_rows(root, repo, syms, r.get(scope, []), ask, prefix="other")
            for g in got:
                g["repo"] = r["repo"]
            all_rows[scope] += got
        print(f"{r['repo']}: " + ", ".join(f"{k} {sum(1 for x in v if x['repo'] == r['repo'])}" for k, v in all_rows.items()), flush=True)

    langs = [l for l in ("go", "rust", "java", "typescript", "python") if any(x["lang"] == l for v in all_rows.values() for x in v)]
    md = [f"# Codebase-Memory on the functions of the run `{name}`", "",
          f"`{version}`. Its `CALLS` edges into each sampled function against the compiler's uses, next to gitnext on the same functions. "
          "± is half of a 90% bootstrap interval over the functions; precision is over the functions for which the tool names a caller.", ""]
    titles = {"callers": "Callers — names that occur once", "callers_common": "Callers — methods that share a name",
              "callers_blind": "Callers — whether or not gitnext sees a caller"}
    for scope, title in titles.items():
        rows = all_rows[scope]
        if not rows:
            continue
        md += [f"## {title}", "", "| | functions | Codebase-Memory: recall | Codebase-Memory: precision | gitnext: recall | gitnext: precision |", "|---|---|---|---|---|---|"]
        for label, part in [("all", rows)] + [(l, [x for x in rows if x["lang"] == l]) for l in langs]:
            if part:
                md.append(f"| {label} | {sum(x['other_recall'] is not None for x in part)} | {cell([x['other_recall'] for x in part])} | {cell([x['other_precision'] for x in part])} | "
                          f"{cell([x['recall'] for x in part])} | {cell([x['precision'] for x in part])} |")
        md.append("")
    text = "\n".join(md)
    (ev.HERE / "results" / f"{name}.codebase-memory.md").write_text(text)
    (ev.HERE / "results" / f"{name}.codebase-memory.json").write_text(json.dumps(all_rows, ensure_ascii=False, indent=1))
    print("\n" + text)


if __name__ == "__main__":
    main()
