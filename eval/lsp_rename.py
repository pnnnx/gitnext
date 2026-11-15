#!/usr/bin/env python3
"""The rename of a language server on the functions of a run, judged as the tool's rename is judged:
after it the project must build and pass the tests that passed before.

Go only: `gopls rename -w <file>:<line>:<column> <new name>` at the name in the definition.

    python3 eval/lsp_rename.py [run]     # a name in eval/results (default: latest) → results/<run>.lsp-rename.{md,json}
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

import run as ev
import lsp_baseline as lb
from baselines import cell


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "latest"
    results = [r for r in json.loads((ev.HERE / "results" / f"{name}.json").read_text()) if "error" not in r and r["lang"] == "go"]
    gopls = shutil.which("gopls") or str(Path.home() / "go/bin/gopls")
    if not Path(gopls).exists():
        sys.exit("gopls is not installed")
    ev._load_cache()
    tool = ev.gx
    rows = []
    for r in results:
        repo = {"name": r["repo"], "lang": r["lang"], "commit": r["commit"]}
        root = ev.WORK / r["repo"]
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True).stdout.strip()
        if not head.startswith(r["commit"]):
            print(f"{r['repo']}: the checkout is at {head[:12]}, the run was at {r['commit'][:12]} — skipped")
            continue
        L = ev.LANG[r["lang"]]
        base = ev.cached(f"{repo['name']}@{repo['commit'][:12]}:baseline:{L.get('tests_key', 'tests')}", lambda: ev.baseline_tests(root, L), keep=lambda v: v["ran"])
        if not base["ran"]:
            print(f"{r['repo']}: the test suite did not run — skipped")
            continue
        by_name = {s["qualified"]: s for s in ev.symbols(root)}
        for x in r.get("rename", []):
            s = by_name.get(x["symbol"])
            if not s:
                continue

            def server(args, at_root, **kw):  # the rename is the server's; everything else stays the tool's
                if args[:1] != ["rename"]:
                    return tool(args, at_root, **kw)
                at = lb.def_place(at_root, s)
                if not at:
                    return 1, "the name is not on the line of the definition"
                p = subprocess.run([gopls, "rename", "-w", f"{s['file']}:{at[0]}:{at[1] + 1}", args[2]], cwd=at_root, capture_output=True, text=True, timeout=600)
                return p.returncode, p.stdout + p.stderr

            ev.gx = server
            try:
                v = ev.check_rename(root, repo, s, True, set(base["failed"]))
            finally:
                ev.gx = tool
            rows.append({"repo": r["repo"], "symbol": x["symbol"], "server_ok": bool(v["ok"]), "server_stage": v.get("stage") or ("build" if v.get("compiled") is False else "tests" if v.get("tests_ok") is False else ""),
                         "server_detail": (v.get("detail") or "")[:300], "gitnext_ok": bool(x["ok"])})
        mine = [x for x in rows if x["repo"] == r["repo"]]
        print(f"{r['repo']}: {len(mine)} renames, gopls {sum(x['server_ok'] for x in mine)}, gitnext {sum(x['gitnext_ok'] for x in mine)}", flush=True)

    md = [f"# The rename of a language server on the functions of the run `{name}`", "",
          "`gopls rename` at the name in the definition, then the same check as for the tool: the project builds and the tests that passed "
          "before still pass. ± is half of a 90% bootstrap interval over the functions.", "",
          "| | renames | gopls: builds and tests pass | gitnext: builds and tests pass | both | only gopls | only gitnext | neither |", "|---|---|---|---|---|---|---|---|"]
    for label, part in [("all", rows)] + [(n, [x for x in rows if x["repo"] == n]) for n in dict.fromkeys(x["repo"] for x in rows)]:
        if part:
            both = sum(x["server_ok"] and x["gitnext_ok"] for x in part)
            md.append(f"| {label} | {len(part)} | {cell([float(x['server_ok']) for x in part])} | {cell([float(x['gitnext_ok']) for x in part])} | {both} | "
                      f"{sum(x['server_ok'] and not x['gitnext_ok'] for x in part)} | {sum(x['gitnext_ok'] and not x['server_ok'] for x in part)} | "
                      f"{sum(not x['server_ok'] and not x['gitnext_ok'] for x in part)} |")
    bad = [x for x in rows if not x["server_ok"]]
    if bad:
        md += ["", "## Where the server's rename did not pass", ""] + [f"- `{x['repo']}` `{x['symbol']}` — {x['server_stage']}: {x['server_detail'][:160].strip()}" for x in bad]
    text = "\n".join(md) + "\n"
    (ev.HERE / "results" / f"{name}.lsp-rename.md").write_text(text)
    (ev.HERE / "results" / f"{name}.lsp-rename.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1))
    print("\n" + text)


if __name__ == "__main__":
    main()
