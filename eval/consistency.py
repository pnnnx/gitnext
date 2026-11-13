#!/usr/bin/env python3
"""Do gitnext's commands agree with each other and with the source?

The commands that eval/run.py does not measure against a compiler (see results/COVERAGE.md, level 3)
are checked here for what can be checked without one:

  symmetry   if B is among the callers of A, then A is among the callees of B and B is in the impact of A;
             if A is among the callees of B, then B is among the callers of A — and as sure there as here
  read       the code `read` prints is exactly the lines of the file it names, and the name is in them
  symbols    every symbol's lines exist, its name is in them, a method lies inside its parent
  search     a symbol is found by its own name
  edit       `edit replace` with the symbol's own code leaves the file as it was; `edit delete` removes
             the symbol's lines and nothing else
  hierarchy  the bases written in a class's own signature (Python, Java, TypeScript) are in `extends` /
             `implements`, and the class is among the subtypes / implementors of each of them
  dead       what `dead` calls likely dead: no other line of the code names it; and with all of them
             deleted the project still compiles and no test starts failing (Python, Go, Rust, TypeScript)

    python3 eval/consistency.py                    # all projects of eval/repos.json that are set up
    python3 eval/consistency.py --repos click,semver --n 40 --no-dead-run

Writes eval/results/consistency.md and .json. Exit 1 if anything disagrees.
"""
from __future__ import annotations

import argparse
import json
import random
import re
import subprocess
import time
from pathlib import Path

import run as ev

HERE = Path(__file__).resolve().parent


def gj(args: list[str], root: Path):
    """A gitnext command's JSON answer, None if it failed."""
    code, out = ev.gx(["--json", *args], root)
    try:
        return json.loads(out) if code == 0 else None
    except ValueError:
        return None


def lines_of(root: Path, file: str, cache: dict) -> list[str]:
    if file not in cache:
        try:
            cache[file] = (root / file).read_text(errors="replace").split("\n")
        except OSError:
            cache[file] = []
    return cache[file]


def check_symmetry(root: Path, sample: list[dict]) -> dict:
    res = {"functions": 0, "caller_pairs": 0, "not_in_callees": [], "not_in_impact": [], "callee_pairs": 0, "not_in_callers": [], "sure_here_maybe_there": []}
    callers_memo: dict[str, tuple[set, set]] = {}

    def callers_of(q: str) -> tuple[set, set]:  # sure, maybe
        if q not in callers_memo:
            d = gj(["callers", q], root) or []
            callers_memo[q] = ({c[0]["qualified"] for x in d for c in x.get("callers", [])}, {c[0]["qualified"] for x in d for c in x.get("maybe", [])})
        return callers_memo[q]

    callers = lambda q: callers_of(q)[0]  # noqa: E731

    for s in sample:
        a = s["qualified"]
        res["functions"] += 1
        imp = {n["symbol"]["qualified"] for x in (gj(["--tests", "impact", a], root) or []) for n in x.get("impact", [])}
        for b in sorted(callers(a)):
            res["caller_pairs"] += 1
            callees = {c[0] for x in (gj(["callees", b], root) or []) for c in x.get("resolved", [])}
            if a not in callees:
                res["not_in_callees"].append(f"{b} calls {a}")
            if b not in imp:
                res["not_in_impact"].append(f"{b} not in impact of {a}")
        for c in sorted({c[0] for x in (gj(["callees", a], root) or []) for c in x.get("resolved", [])}):
            if c == a:
                continue  # a function that calls itself is not listed among its own callers
            res["callee_pairs"] += 1
            sure, maybe = callers_of(c)
            if a in maybe and a not in sure:
                res["sure_here_maybe_there"].append(f"{a} calls {c}: sure in callees, a guess in callers")
            elif a not in sure:
                res["not_in_callers"].append(f"{a} calls {c}")
    return res


def check_read(root: Path, sample: list[dict]) -> dict:
    res = {"symbols": 0, "wrong": []}
    cache: dict = {}
    for s in sample:
        d = gj(["read", s["qualified"]], root)
        if not d:
            res["wrong"].append(f"{s['qualified']}: no answer")
            continue
        res["symbols"] += 1
        src = d[0]["source"]
        m = re.match(r"(\S+):(\d+)-(\d+) [^\n]*\n```[^\n]*\n(.*)\n```\n$", src, re.S)
        if not m:
            res["wrong"].append(f"{s['qualified']}: unexpected shape")
            continue
        file, a, b, code = m.group(1), int(m.group(2)), int(m.group(3)), m.group(4)
        real = "\n".join(lines_of(root, file, cache)[a - 1:b])
        if code != real:
            res["wrong"].append(f"{s['qualified']}: the code differs from {file}:{a}-{b}")
        elif s["name"] not in code:
            res["wrong"].append(f"{s['qualified']}: its name is not in {file}:{a}-{b}")
        elif not (a <= s["start_line"] and s["end_line"] <= b):
            res["wrong"].append(f"{s['qualified']}: read {a}-{b} does not cover the symbol {s['start_line']}-{s['end_line']}")
    return res


def check_symbols(root: Path, syms: list[dict]) -> dict:
    res = {"symbols": 0, "wrong": []}
    cache: dict = {}
    by_q: dict[str, list[dict]] = {}
    for s in syms:
        by_q.setdefault(s["qualified"], []).append(s)
    for s in syms:
        if s["kind"] == "module" or "$anon" in s["name"]:  # an anonymous class has a made-up name
            continue
        res["symbols"] += 1
        L = lines_of(root, s["file"], cache)
        a, b = s["start_line"], s["end_line"]
        if not (1 <= a <= b <= len(L)):
            res["wrong"].append(f"{s['qualified']}: lines {a}-{b} of a file with {len(L)}")
        elif s["name"] not in "\n".join(L[a - 1:b]):
            res["wrong"].append(f"{s['qualified']}: the name is not in {s['file']}:{a}-{b}")
        else:
            # a method lies inside the type it belongs to, when that type is in the same file (not an `impl` elsewhere)
            ps = [p for p in by_q.get(s.get("parent") or "", []) if p["file"] == s["file"] and p["kind"] != "module"]
            if s["kind"] == "method" and ps and not any(p["start_line"] <= a and b <= p["end_line"] for p in ps) \
                    and not s["file"].endswith((".rs", ".go", ".c", ".cc", ".cpp", ".h", ".hpp")):
                res["wrong"].append(f"{s['qualified']}: {a}-{b} is outside its parent {ps[0]['start_line']}-{ps[0]['end_line']}")
    return res


def check_search(root: Path, sample: list[dict]) -> dict:
    res = {"symbols": 0, "not_found": []}
    for s in sample:
        d = gj(["--tests", "search", s["name"]], root) or []
        if len(d) >= 20 and not any(x.get("qualified") == s["qualified"] for x in d):
            continue  # `toString`, `fmt`: more namesakes than one answer holds — not a miss
        res["symbols"] += 1
        if not any(x.get("qualified") == s["qualified"] for x in d):
            res["not_found"].append(f"{s['qualified']} by `{s['name']}` ({len(d)} hits)")
    return res


def check_edit(root: Path, sample: list[dict]) -> dict:
    res = {"replaced": 0, "deleted": 0, "wrong": []}
    clean = lambda: ev.restore(root)  # noqa: E731
    cache: dict = {}
    for s in sample:
        q = s["qualified"]
        d = gj(["read", q], root)
        m = d and re.match(r"(\S+):(\d+)-(\d+) [^\n]*\n```[^\n]*\n(.*)\n```\n$", d[0]["source"], re.S)
        if not m:
            continue
        file, a, b, code = m.group(1), int(m.group(2)), int(m.group(3)), m.group(4)
        before = (root / file).read_text(errors="replace")
        p = subprocess.run([ev.GITNEXT, "edit", "replace", q], input=code + "\n", cwd=root, capture_output=True, text=True)
        res["replaced"] += 1
        if (root / file).read_text(errors="replace") != before:
            diff = ev.sh(f"git diff --stat -- {file}", root)[1].strip().splitlines()
            res["wrong"].append(f"replace {q} with its own code changed {file}: {diff[-1].strip() if diff else p.stdout[:80]}")
        clean()
        ev.gx(["index"], root)
        p = subprocess.run([ev.GITNEXT, "edit", "delete", q], cwd=root, capture_output=True, text=True)
        res["deleted"] += 1
        after = (root / file).read_text(errors="replace").split("\n")
        old = lines_of(root, file, cache) if file in cache else before.split("\n")
        # what is left must be the old file without one block of lines: the symbol, and at most
        # three blank or comment lines around it (a diff by common prefix slides over repeated lines)
        if after == old:
            res["wrong"].append(f"delete {q}: nothing was removed ({(p.stdout + p.stderr).strip()[:100]})")
        else:
            blocks = [(lo, hi) for lo in range(max(1, a - 3), a + 1) for hi in range(b, min(len(old), b + 3) + 1) if old[:lo - 1] + old[hi:] == after]
            if not blocks:
                res["wrong"].append(f"delete {q}: what is left is not {file} without the lines around {a}-{b}")
            elif not any(all(not x.strip() or x.lstrip().startswith(("#", "//", "*", "/*", "@")) for x in old[lo - 1:a - 1] + old[b:hi]) for lo, hi in blocks):
                res["wrong"].append(f"delete {q}: removed code next to the symbol {a}-{b}")
        clean()
        ev.gx(["index"], root)
    return res


def no_generics(sig: str) -> str:
    while re.search(r"<[^<>]*>", sig):
        sig = re.sub(r"<[^<>]*>", "", sig)
    return sig


BASES = {
    ".py": lambda sig: [b.strip().split("[")[0].split(".")[-1] for b in (re.search(r"class \w+\((.*)\)", sig) or [None, ""])[1].split(",")
                        if b.strip() and "=" not in b],
    ".java": lambda sig: re.findall(r"[\w.]+", " ".join(re.findall(r"(?:extends|implements)\s+([\w.,\s]+?)(?=\s+(?:extends|implements|permits)\b|\s*\{|$)", no_generics(sig)))),
    ".ts": lambda sig: re.findall(r"[\w.]+", " ".join(re.findall(r"(?:extends|implements)\s+([\w.,\s]+?)(?=\s+(?:extends|implements)\b|\s*\{|$)", no_generics(sig)))),
}


def check_hierarchy(root: Path, syms: list[dict]) -> dict:
    res = {"types": 0, "bases": 0, "missing_base": [], "missing_subtype": []}
    types = [s for s in syms if s["kind"] in ("class", "interface", "struct", "trait", "enum")]
    by_name: dict[str, list[dict]] = {}
    for t in types:
        by_name.setdefault(t["name"], []).append(t)
    for t in types:
        ext = Path(t["file"]).suffix
        ext = ".ts" if ext in (".tsx", ".mts") else ext
        if ext not in BASES or len(by_name[t["name"]]) != 1:
            continue
        bases = [b.split(".")[-1] for b in BASES[ext](t.get("signature") or "")]
        bases = [b for b in bases if len(by_name.get(b, [])) == 1 and b != t["name"]]  # a base that is one type of this repo
        if not bases:
            continue
        res["types"] += 1
        h = (gj(["--tests", "hierarchy", t["qualified"]], root) or [{}])[0]
        up = {x.split("::")[-1].split(".")[-1] for x in h.get("extends", []) + h.get("implements", [])}
        for b in bases:
            res["bases"] += 1
            if b not in up:
                res["missing_base"].append(f"{t['qualified']}: `{b}` is in its signature, not in extends / implements")
            hb = (gj(["--tests", "hierarchy", by_name[b][0]["qualified"]], root) or [{}])[0]
            down = {x["qualified"] for x in hb.get("subtypes", []) + hb.get("implementors", [])}
            if t["qualified"] not in down:
                res["missing_subtype"].append(f"{by_name[b][0]['qualified']}: `{t['name']}` extends it, not among its subtypes")
    return res


def dead_entries(root: Path) -> dict[str, list[tuple[str, str, str, int]]]:
    d = gj(["dead"], root) or {}
    out = {}
    for group in ("likely", "public_unused"):
        out[group] = []
        for e in d.get(group, []):
            m = re.match(r"(\w+) (\S+)\s+(\S+):(\d+)$", e)
            if m:
                out[group].append((m.group(1), m.group(2), m.group(3), int(m.group(4))))
    return out


def check_dead(root: Path, repo: dict, syms: list[dict], run: bool) -> dict:
    ent = dead_entries(root)
    res = {"likely": len(ent["likely"]), "public_unused": len(ent["public_unused"]), "named_elsewhere": [], "deleted": 0, "broke": None}
    files = sorted({s["file"] for s in syms})
    text = {f: (root / f).read_text(errors="replace").split("\n") for f in files if (root / f).is_file()}
    span = {(s["file"], s["start_line"]): s["end_line"] for s in syms}
    for kind, name, file, line in ent["likely"]:
        short = name.split("::")[-1]
        end = span.get((file, line), line)
        pat = re.compile(rf"(?<![\w]){re.escape(short)}(?![\w])")
        hits = [f"{f}:{n}" for f, L in text.items() for n, l in enumerate(L, 1)
                if pat.search(l) and not (f == file and line <= n <= end) and not l.lstrip().startswith(("#", "//", "*", "/*"))]
        if hits:
            res["named_elsewhere"].append(f"{kind} {name} {file}:{line} — named at {', '.join(hits[:3])}{' …' if len(hits) > 3 else ''}")
    L = ev.LANG.get(repo["lang"])
    res["verdict"] = f"{len(ent['likely'])} named likely dead"
    if not ent["likely"]:
        res["verdict"] += ": nothing to delete"
        return res
    if not run or not L:
        res["verdict"] += ": deleting not tried" + ("" if run else " (--no-dead-run)")
        return res
    L = dict(L)
    if repo["lang"] == "rust":
        # every feature and every target: code behind a feature, examples, benches and tests all build.
        # Benches with `#![feature(test)]` build on nightly only: where stable fails before anything is deleted, nightly
        tc = ""
        build = "cargo {}test --all-features --all-targets --no-run --no-fail-fast --message-format human 2>&1"
        if ev.sh(build.format(""), root, timeout=1800)[0] and "nightly" in ev.sh("rustup toolchain list", root)[1]:
            tc = "+nightly "
        L["compile"], L["tests"] = build.format(tc), f"cargo {tc}test --all-features --no-fail-fast 2>&1"
        res["toolchain"] = tc.strip("+ ") or "stable"
    compiled = lambda: set(map(str, L["errors"](ev.sh(L["compile"], root, timeout=1800)[1], root))) if L.get("compile") and L.get("errors") else set()  # noqa: E731
    before_c = compiled()
    before_t = set(L["failed"](ev.sh(L["tests"], root, timeout=1800)[1], root))
    # bottom-up inside each file, so the lines of the ones still to delete do not move
    for kind, name, file, line in sorted(ent["likely"], key=lambda e: (e[2], -e[3])):
        p = subprocess.run([ev.GITNEXT, "edit", "delete", f"{name.split('::')[-1]}@{line}"], cwd=root, capture_output=True, text=True)
        res["deleted"] += p.returncode == 0
    after_c = compiled()
    out = ev.sh(L["tests"], root, timeout=1800)[1]
    after_t = set(L["failed"](out, root))
    # pytest: a test module that no longer imports is an ERROR line without a test name
    after_t |= {f"{m} (does not import)" for m in re.findall(r"^ERROR (\S+\.py)(?: - .*)?$", out, re.M)}
    res["broke"] = {"compile_errors": sorted(after_c - before_c)[:8], "tests": sorted(after_t - before_t)[:8], "tests_ran": bool(L["ran"](out, root))}
    new_c, new_t = len(after_c - before_c), len(after_t - before_t)
    res["verdict"] += (f", {res['deleted']} deleted: {new_c} new compile errors, {new_t} new failing tests"
                       + ("" if res["broke"]["tests_ran"] else " (the tests did not run)") + (f" ({res['toolchain']})" if "toolchain" in res else ""))
    ev.restore(root)
    ev.gx(["index"], root)
    return res


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repos", default="")
    ap.add_argument("--n", type=int, default=30, help="functions per project for symmetry / read / search")
    ap.add_argument("--n-edit", type=int, default=10)
    ap.add_argument("--seed", type=int, default=5)
    ap.add_argument("--no-dead-run", action="store_true", help="skip deleting the dead code and running the tests")
    ap.add_argument("--out", default=str(HERE / "results" / "consistency.json"))
    args = ap.parse_args()
    repos = [r for r in json.loads((HERE / "repos.json").read_text()) if not args.repos or r["name"] in args.repos.split(",")]
    all_res = {}
    for repo in repos:
        t0 = time.time()
        try:
            root = ev.prepare(repo)
        except RuntimeError as e:
            print(f"{repo['name']}: skipped ({str(e)[:80]})", flush=True)
            continue
        ev.gx(["init"], root, timeout=1800)
        syms = ev.symbols(root)
        count: dict[str, int] = {}
        for s in syms:
            count[s["qualified"]] = count.get(s["qualified"], 0) + 1
        funcs = [s for s in syms if s["kind"] in ("function", "method") and count[s["qualified"]] == 1 and not s.get("test")
                 and not s["file"].startswith(("examples/", "example/", "docs/", "_examples/"))]
        rng = random.Random(args.seed)
        rng.shuffle(funcs)
        sample = funcs[:args.n]
        r = {
            "symmetry": check_symmetry(root, sample),
            "read": check_read(root, sample),
            "symbols": check_symbols(root, syms),
            "search": check_search(root, sample),
            "edit": check_edit(root, sample[:args.n_edit]),
            "hierarchy": check_hierarchy(root, syms),
            "dead": check_dead(root, repo, syms, not args.no_dead_run),
        }
        ev.restore(root)
        r["seconds"] = round(time.time() - t0)
        all_res[repo["name"]] = r
        sy, d = r["symmetry"], r["dead"]
        print(f"{repo['name']:10} symmetry {sy['caller_pairs']}+{sy['callee_pairs']} pairs, off {len(sy['not_in_callees'])}/{len(sy['not_in_impact'])}/{len(sy['not_in_callers'])}/{len(sy['sure_here_maybe_there'])}"
              f" · read off {len(r['read']['wrong'])}/{r['read']['symbols']} · symbols off {len(r['symbols']['wrong'])}/{r['symbols']['symbols']}"
              f" · search off {len(r['search']['not_found'])} · edit off {len(r['edit']['wrong'])}/{r['edit']['replaced'] + r['edit']['deleted']}"
              f" · hierarchy off {len(r['hierarchy']['missing_base'])}+{len(r['hierarchy']['missing_subtype'])}/{r['hierarchy']['bases']}"
              f" · dead {d['likely']}, named elsewhere {len(d['named_elsewhere'])}, broke {d['broke']}  ({r['seconds']}s)", flush=True)
        Path(args.out).write_text(json.dumps(all_res, ensure_ascii=False, indent=1))
    write_md(all_res, Path(args.out).with_suffix(".md"), args)
    bad = sum(len(r["symmetry"]["not_in_callees"]) + len(r["symmetry"]["not_in_impact"]) + len(r["symmetry"]["not_in_callers"]) + len(r["symmetry"]["sure_here_maybe_there"]) + len(r["read"]["wrong"])
              + len(r["symbols"]["wrong"]) + len(r["search"]["not_found"]) + len(r["edit"]["wrong"]) + len(r["hierarchy"]["missing_base"])
              + len(r["hierarchy"]["missing_subtype"]) + len(r["dead"]["named_elsewhere"])
              + (len(r["dead"]["broke"]["compile_errors"]) + len(r["dead"]["broke"]["tests"]) if r["dead"]["broke"] else 0) for r in all_res.values())
    raise SystemExit(1 if bad else 0)


def write_md(all_res: dict, path: Path, args) -> None:
    md = ["# Do the commands agree with each other and with the source?", "",
          f"`eval/consistency.py`, {args.n} functions per project (seed {args.seed}). What each column checks is at the top of the script.",
          "A cell is `disagreements / checked`.", "",
          "| project | callers→callees | callers→impact | callees→callers | callees sure, callers maybe | read | symbols | search | edit | hierarchy | dead: named elsewhere | dead deleted: new compile errors / failing tests |",
          "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for name, r in all_res.items():
        sy, d, h = r["symmetry"], r["dead"], r["hierarchy"]
        broke = "—" if not d["broke"] else f"{len(d['broke']['compile_errors'])} / {len(d['broke']['tests'])} ({d['deleted']} deleted)"
        md.append(f"| {name} | {len(sy['not_in_callees'])} / {sy['caller_pairs']} | {len(sy['not_in_impact'])} / {sy['caller_pairs']} | {len(sy['not_in_callers'])} / {sy['callee_pairs']} | {len(sy['sure_here_maybe_there'])} / {sy['callee_pairs']}"
                  f" | {len(r['read']['wrong'])} / {r['read']['symbols']} | {len(r['symbols']['wrong'])} / {r['symbols']['symbols']} | {len(r['search']['not_found'])} / {r['search']['symbols']}"
                  f" | {len(r['edit']['wrong'])} / {r['edit']['replaced'] + r['edit']['deleted']} | {len(h['missing_base']) + len(h['missing_subtype'])} / {2 * h['bases']}"
                  f" | {len(d['named_elsewhere'])} / {d['likely']} | {broke} |")
    md += ["", "## The disagreements", ""]
    for name, r in all_res.items():
        items = ([("callers→callees", x) for x in r["symmetry"]["not_in_callees"]] + [("callers→impact", x) for x in r["symmetry"]["not_in_impact"]]
                 + [("callees→callers", x) for x in r["symmetry"]["not_in_callers"] + r["symmetry"]["sure_here_maybe_there"]] + [("read", x) for x in r["read"]["wrong"]]
                 + [("symbols", x) for x in r["symbols"]["wrong"]] + [("search", x) for x in r["search"]["not_found"]] + [("edit", x) for x in r["edit"]["wrong"]]
                 + [("hierarchy", x) for x in r["hierarchy"]["missing_base"] + r["hierarchy"]["missing_subtype"]]
                 + [("dead", x) for x in r["dead"]["named_elsewhere"]]
                 + ([("dead deleted", x) for x in r["dead"]["broke"]["compile_errors"] + r["dead"]["broke"]["tests"]] if r["dead"]["broke"] else []))
        if items:
            md.append(f"**{name}**")
            md += [f"- {k}: {x}" for k, x in items[:25]] + ([f"- … and {len(items) - 25} more"] if len(items) > 25 else []) + [""]
    path.write_text("\n".join(md) + "\n")
    print("\n".join(md[:8 + len(all_res)]))


if __name__ == "__main__":
    main()
