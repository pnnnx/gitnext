"""learn-tests prototype for Rust (semver, indexmap): per-test coverage map against `gitnext tests-for`.

The test binaries are built once with `-C instrument-coverage` (the eval's `cargo test`: default features, its own
target dir), then every test runs alone (`<bin> --exact <path>`) with its own `LLVM_PROFILE_FILE`;
`llvm-profdata merge` + `llvm-cov export -format=lcov` (rustup's llvm-tools-preview) give the lines it ran, and
each line is booked to the innermost function holding it. Doc-tests are not run: what they would find is counted
in the report. Driver, samples and metrics: `learn_tests_go.measure`.

    GITNEXT=$PWD/target/release/gitnext python3 eval/learn_tests_rust.py [--repos semver,indexmap] [--n2 12] \\
        [--pick-from <truth.json of an earlier run>]
"""
from __future__ import annotations

import glob
import json
import os
import shlex
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import learn_tests_go as lg  # noqa: E402
import run  # noqa: E402


def llvm_tool(name: str) -> str:
    sysroot = run.sh("rustc --print sysroot", Path("."))[1].strip()
    hits = glob.glob(f"{sysroot}/lib/rustlib/*/bin/{name}")
    if not hits:
        raise RuntimeError(f"{name}: rustup component add llvm-tools-preview")
    return hits[0]


def lcov_lines(text: str, root: Path) -> dict[str, set[int]]:
    out: dict[str, set[int]] = {}
    cur = None
    r = str(root.resolve()) + "/"
    for line in text.splitlines():
        if line.startswith("SF:"):
            p = line[3:]
            cur = out.setdefault(p[len(r):], set()) if p.startswith(r) else None
        elif line.startswith("DA:") and cur is not None:
            n, c = line[3:].split(",")[:2]
            if c != "0":
                cur.add(int(n))
    return out


def rust_collect(root: Path, syms: list[dict], work: Path) -> tuple[dict, dict]:
    profdata, cov = llvm_tool("llvm-profdata"), llvm_tool("llvm-cov")
    target = work / "target"  # its own target dir: never the one the eval's plain runs use
    env = {**os.environ, "RUSTFLAGS": "-C instrument-coverage", "CARGO_TARGET_DIR": str(target),
           "LLVM_PROFILE_FILE": str(work / "build-%p.profraw")}  # build scripts are instrumented too
    t = time.time()
    code, o = run.sh("cargo test --no-run --message-format=json 2>/dev/null", root, timeout=3600, env=env)
    build_s = time.time() - t
    bins = []
    for line in o.splitlines():
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        if ev.get("reason") == "compiler-artifact" and ev.get("profile", {}).get("test") and ev.get("executable"):
            bins.append((ev["executable"], Path(ev["manifest_path"]).parent))
    if not bins:
        raise RuntimeError(f"no test binaries: {o[-400:]}")
    tests: dict = {}
    run_s = export_s = 0.0
    prof_bytes = 0
    prof = work / "prof"
    prof.mkdir(exist_ok=True)
    for k, (b, pkg) in enumerate(bins):
        _, listed = run.sh(f"{b} --list --format terse 2>/dev/null", pkg)
        names = [x[:-len(": test")] for x in listed.splitlines() if x.endswith(": test")]
        for j, n in enumerate(names):
            raw, data = prof / f"{k}-{j}.profraw", prof / f"{k}-{j}.profdata"
            renv = {**os.environ, "LLVM_PROFILE_FILE": str(raw), "CARGO_MANIFEST_DIR": str(pkg)}
            t = time.time()
            code, _ = run.sh(f"{b} --exact {shlex.quote(n)} --test-threads=1 -q", pkg, timeout=300, env=renv)
            run_s += time.time() - t
            prof_bytes += raw.stat().st_size if raw.exists() else 0
            t = time.time()
            lines: dict = {}
            if raw.exists():
                run.sh(f"{profdata} merge -sparse {raw} -o {data}", pkg)
                _, lc = run.sh(f"{cov} export -format=lcov -instr-profile={data} {b} "
                               "--ignore-filename-regex='/.cargo/|/rustc/'", pkg, timeout=600)
                lines = lcov_lines(lc, root)
            export_s += time.time() - t
            tests[f"{Path(b).name}:{n}"] = {"name": n.rsplit("::", 1)[-1], "failed": bool(code), "lines": lines}
            raw.unlink(missing_ok=True)
            data.unlink(missing_ok=True)
    _, doc = run.sh("grep -rlE '^\\s*//[/!] *```' src | wc -l", root)
    return tests, {"build_s": round(build_s, 1), "run_one_by_one_s": round(run_s, 1), "export_s": round(export_s, 1),
                   "map_s": round(build_s + run_s + export_s, 1), "map_without_build_s": round(run_s + export_s, 1),
                   "profiles_bytes": prof_bytes, "test_binaries": len(bins), "files_with_doctests": doc.strip()}


def rust_library(file: str) -> bool:
    return file.startswith("src/") and file.endswith(".rs")


if __name__ == "__main__":
    lg.main_for("rust", "semver,indexmap", rust_collect, "cargo test --no-fail-fast 2>&1", rust_library, __doc__)
