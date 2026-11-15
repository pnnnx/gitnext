#!/usr/bin/env python3
"""The upper bound for callers: what a language server answers on the functions of a run.

A language server has the compiler's own view of the code, which gitnext does without. For every function of the run
it is asked for the references of the definition (gopls: its command line; rust-analyzer: the language server protocol;
TypeScript: tsserver's own protocol); every reference is put into the function around it and compared with the run's truth.

    python3 eval/lsp_baseline.py [run]     # → results/<run>.lsp.{md,json}; a language whose server is not installed is skipped
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import baselines as bl
import run as ev


def def_place(root: Path, s: dict) -> tuple[int, int] | None:
    """Line (1-based) and column (0-based) of the name in the symbol's definition."""
    lines = (root / s["file"]).read_text(errors="replace").split("\n")
    i = s["start_line"] - 1 + ev._def_offset(root, s)
    col = ev.name_col(lines[i], s["name"]) if i < len(lines) else -1
    return (i + 1, col) if col >= 0 else None


class Wire:
    """Messages with a Content-Length header over a process's pipes."""

    def __init__(self, cmd: list[str], cwd: Path):
        self.p = subprocess.Popen(cmd, cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)

    def read(self) -> dict:
        length = 0
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("the server closed its output")
            line = line.strip()
            if not line:
                if length:
                    break
                continue
            if line.lower().startswith(b"content-length:"):
                length = int(line.split(b":")[1])
        return json.loads(self.p.stdout.read(length))

    def close(self) -> None:
        self.p.kill()


class Lsp(Wire):
    def __init__(self, cmd, root: Path, options: dict | None = None):
        super().__init__(cmd, root)
        self.id = 0
        self.quiet = False
        self.request("initialize", {"processId": os.getpid(), "rootUri": root.as_uri(), "capabilities": {"experimental": {"serverStatusNotification": True}},
                                    "initializationOptions": options or {},
                                    "workspaceFolders": [{"uri": root.as_uri(), "name": root.name}]})
        self.notify("initialized", {})

    def send(self, msg: dict) -> None:
        body = json.dumps({"jsonrpc": "2.0", **msg}).encode()
        self.p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.p.stdin.flush()

    def notify(self, method: str, params: dict) -> None:
        self.send({"method": method, "params": params})

    def request(self, method: str, params: dict):
        self.id += 1
        self.send({"id": self.id, "method": method, "params": params})
        while True:
            m = self.read()
            if m.get("method") == "experimental/serverStatus":
                self.quiet = bool(m["params"].get("quiescent"))
            if "id" in m and "method" in m:        # the server asks the client something: say there is nothing
                self.send({"id": m["id"], "result": None})
            elif m.get("id") == self.id:
                return m.get("result")

    def wait_ready(self, limit: int = 900) -> None:
        end = time.time() + limit
        while not self.quiet and time.time() < end:
            m = self.read()
            if m.get("method") == "experimental/serverStatus":
                self.quiet = bool(m["params"].get("quiescent"))
            elif "id" in m and "method" in m:
                self.send({"id": m["id"], "result": None})


def go_hits(root: Path):
    gopls = shutil.which("gopls") or str(Path.home() / "go/bin/gopls")
    if not Path(gopls).exists():
        return None

    def hits(s: dict):
        at = def_place(root, s)
        if not at:
            return None
        out = subprocess.run([gopls, "references", f"{s['file']}:{at[0]}:{at[1] + 1}"], cwd=root, capture_output=True, text=True, timeout=600).stdout
        found = []
        for row in out.splitlines():
            m = re.match(r"(.+?):(\d+):\d+", row)
            if m:
                found.append((os.path.relpath(m.group(1), root), int(m.group(2))))
        return found
    return hits, lambda: None


def rust_hits(root: Path):
    ra = shutil.which("rust-analyzer") or subprocess.run(["rustup", "which", "rust-analyzer"], capture_output=True, text=True).stdout.strip()
    if not ra or not Path(ra).exists():
        return None
    # all features, as the run's compiler has them: code behind a feature is code too
    server = Lsp([ra], root, {"cargo": {"features": "all"}})
    server.wait_ready()

    def hits(s: dict):
        at = def_place(root, s)
        if not at:
            return None
        got = server.request("textDocument/references", {"textDocument": {"uri": (root / s["file"]).as_uri()}, "position": {"line": at[0] - 1, "character": at[1]},
                                                         "context": {"includeDeclaration": False}})
        return [(os.path.relpath(x["uri"].removeprefix("file://"), root), x["range"]["start"]["line"] + 1) for x in got or []]
    return hits, server.close


def ts_hits(root: Path):
    js = root / "node_modules/typescript/lib/tsserver.js"
    if not js.exists():
        return None
    server = Wire(["node", str(js)], root)
    seq = [0]

    def ask(command: str, args: dict, answer: bool = True):
        seq[0] += 1
        server.p.stdin.write((json.dumps({"seq": seq[0], "type": "request", "command": command, "arguments": args}) + "\n").encode())
        server.p.stdin.flush()
        while answer:
            m = server.read()
            if m.get("type") == "response" and m.get("request_seq") == seq[0]:
                return m.get("body")

    def hits(s: dict):
        at = def_place(root, s)
        if not at:
            return None
        file = str(root / s["file"])
        ask("open", {"file": file}, answer=False)
        body = ask("references", {"file": file, "line": at[0], "offset": at[1] + 1})
        return [(os.path.relpath(x["file"], root), x["start"]["line"]) for x in (body or {}).get("refs", []) if not x.get("isDefinition")]
    return hits, server.close


SERVERS = {"go": ("gopls", go_hits), "rust": ("rust-analyzer", rust_hits), "typescript": ("tsserver", ts_hits)}


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "latest"
    results = [r for r in json.loads((ev.HERE / "results" / f"{name}.json").read_text()) if "error" not in r and r["lang"] in SERVERS]
    grep_rows = json.loads((ev.HERE / "results" / f"{name}.baselines.json").read_text()) if (ev.HERE / "results" / f"{name}.baselines.json").exists() else {}
    rows = {"callers": [], "callers_common": []}
    timing = {}
    for r in results:
        repo = {"name": r["repo"], "lang": r["lang"], "commit": r["commit"]}
        root = (ev.WORK / r["repo"]).resolve()
        t0 = time.time()
        made = SERVERS[r["lang"]][1](root)
        if made is None:
            print(f"{r['repo']}: no {SERVERS[r['lang']][0]} here — skipped")
            continue
        hits, close = made
        ready = time.time() - t0
        syms = ev.symbols(root)
        n = 0
        t1 = time.time()
        for scope in rows:
            got = bl.callers_rows(root, repo, syms, r.get(scope, []), hits_of=hits, prefix="server")
            for g in got:
                g["repo"] = r["repo"]
            rows[scope] += got
            n += len(got)
        close()
        timing[r["repo"]] = {"server": SERVERS[r["lang"]][0], "ready_seconds": round(ready, 1), "seconds_per_function": round((time.time() - t1) / max(n, 1), 2)}
        print(f"{r['repo']}: {n} functions, ready in {ready:.0f}s, {timing[r['repo']]['seconds_per_function']}s each", flush=True)

    md = [f"# Language servers on the functions of the run `{name}`", "",
          "References of the definition, asked from gopls (Go), rust-analyzer (Rust) and tsserver (TypeScript); every reference is put into the "
          "function around it. The truth is the run's own (the compiler's uses). ± is half of a 90% bootstrap interval over the functions.", ""]
    for scope, title in (("callers", "Names that occur once"), ("callers_common", "Methods that share a name")):
        part_all = rows[scope]
        if not part_all:
            continue
        grep_of = {(x["repo"], x["symbol"]): x for x in grep_rows.get(scope, [])}
        md += [f"## {title}", "", "| | functions | server: recall | server: precision | gitnext: recall | gitnext: precision | search: recall | search: precision |", "|---|---|---|---|---|---|---|---|"]
        for label, part in [("all three", part_all)] + [(l, [x for x in part_all if x["lang"] == l]) for l in SERVERS]:
            if part:
                g = [grep_of[(x["repo"], x["symbol"])] for x in part if (x["repo"], x["symbol"]) in grep_of]
                md.append(f"| {label} | {sum(x['recall'] is not None for x in part)} | {bl.cell([x['server_recall'] for x in part])} | {bl.cell([x['server_precision'] for x in part])} | "
                          f"{bl.cell([x['recall'] for x in part])} | {bl.cell([x['precision'] for x in part])} | "
                          f"{bl.cell([x['grep_recall'] for x in g])} | {bl.cell([x['grep_precision'] for x in g])} |")
        md.append("")
    md += ["## Time", "", "| project | server | ready after | per function |", "|---|---|---|---|"]
    md += [f"| {k} | {v['server']} | {v['ready_seconds']} s | {v['seconds_per_function']} s |" for k, v in timing.items()]
    text = "\n".join(md) + "\n"
    (ev.HERE / "results" / f"{name}.lsp.md").write_text(text)
    (ev.HERE / "results" / f"{name}.lsp.json").write_text(json.dumps({"rows": rows, "timing": timing}, ensure_ascii=False, indent=1))
    print("\n" + text)


if __name__ == "__main__":
    main()
