"""learn-tests prototype for Java (jsoup, json-java): does a per-test coverage map (JaCoCo) find the tests that fail
better than `gitnext tests-for`, what does it cost, and how to take it?

The eval's truth for Java is the bare name of a test method (`level="name"`, `run.java_failed`), so the map is per
test method too. Three ways to take it, all with the JaCoCo agent:

- `per-test` (one Maven run): a listener of the JUnit Platform (`TestExecutionListener`, `META-INF/services`;
  JUnit 4 runs through the platform too, vintage engine of surefire 3) takes the agent's data, and resets it, when a test
  starts and when it ends (`RT.getAgent().getExecutionData(true)`), one `.exec` per test. What runs in a test class
  outside its tests (`@BeforeAll`, `@BeforeClass`, `@AfterAll`, and its static blocks: the listener initializes the
  class when it starts, else they would run in its first test) is booked to every test of the class: when it
  throws, they all fail. The invocations of a parameterized test are one test method.
- `per-test-fork`: the same, with a JVM of its own for every test class (`-DreuseForks=false`): what a JVM does
  once (a lazily built table, a cache) shows up in the first test of every class, not only in the first one of all.
- `per-class` (`-Dtest=<Class>`, one Maven run per test class): one `.exec` per class, every method of the class is
  booked with all the class ran.

One JVM (`Analyze`, jacoco core) turns the `.exec` files into lines that ran; a line is booked to the innermost
function whose body holds it (from `run.body_start`): a class's field initializers and static blocks run when the
class loads, booking them to the function around them would book it to every test that loads the class.
The build is changed only in a copy of `pom.xml` (`.gnx-pom.xml` in the checkout, removed after the run): the
agent goes through surefire's `argLine` (jsoup sets one in its pom, `-DargLine` cannot add to it) and the listener
through `additionalClasspathElements` (a profile of jsoup sets that list, `-Dmaven.test.additionalClasspath` would
replace it). The listener, `Analyze` and `Inits` are compiled from the sources below into the work directory;
the JaCoCo jars come from the local Maven repository (`mvn dependency:get` the first time).
The driver is `learn_tests_go.measure`.

    GITNEXT=$PWD/target/release/gitnext python3 eval/learn_tests_java.py [--repos jsoup,json-java] [--n2 12] \\
        [--seed 7] [--pick-from <truth.json of an earlier run>] [--how per-test,per-test-fork,per-class] \\
        [--owner body|line] [--render]
"""
from __future__ import annotations

import argparse
import json
import re
import time
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import learn_tests_go as lg  # noqa: E402  the shared driver of 005
import run  # noqa: E402  the eval: same checkout, ground truth and cache

JACOCO = "0.8.14"
M2 = Path.home() / ".m2" / "repository"
HOWS = ("per-test", "per-test-fork", "per-class")
PLATFORM = "1.14.4"  # the JUnit Platform surefire 3.6 brings for JUnit 4 (vintage)

DUMP = r"""
package gnx;

import java.io.*;
import java.nio.file.*;

/** The JaCoCo agent's data of every test on its own: taken, and reset, when a test starts and when it ends. */
public final class Dump {
    static final Path DIR = Paths.get(System.getProperty("gnx.dir", "gnx-exec"));
    static final Object agent;
    static final java.lang.reflect.Method get;
    static int seq;
    static PrintWriter index;
    static {
        try {
            Object a = Class.forName("org.jacoco.agent.rt.RT").getMethod("getAgent").invoke(null);
            agent = a;
            get = Class.forName("org.jacoco.agent.rt.IAgent").getMethod("getExecutionData", boolean.class);
            Files.createDirectories(DIR);
            index = new PrintWriter(new FileWriter(DIR.resolve("index.tsv").toFile(), true), true);
        } catch (Exception e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    static byte[] take() {
        try {
            return (byte[]) get.invoke(agent, true);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    // a JVM per test class (`reuseForks=false`) writes to the same directory: the pid keeps the files apart
    static final String JVM = Long.toString(ProcessHandle.current().pid());

    static void write(String cls, String method, boolean failed, byte[] data) {
        String n = JVM + "-" + seq++;
        try {
            Files.write(DIR.resolve(n + ".exec"), data);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        index.println(n + "\t" + cls + "\t" + method + "\t" + (failed ? 1 : 0));
    }

    /** A test class starts: what ran before it is no test's. Its static blocks run now, not in its first test:
     *  they are all of its tests'. */
    public static synchronized void classStarted(String cls) {
        take();
        try {
            Class.forName(cls, true, Thread.currentThread().getContextClassLoader());
        } catch (Throwable e) {
            // its tests fail on their own
        }
    }

    /** What ran since the class started or since its last test (`@BeforeAll`, `@AfterAll`, a static block): all of its tests'. */
    public static synchronized void classPart(String cls) {
        write(cls, "", false, take());
    }

    public static synchronized void started(String cls) {
        classPart(cls);
    }

    public static synchronized void finished(String cls, String method, boolean failed) {
        write(cls, method, failed, take());
    }
}
"""

DUMP5 = r"""
package gnx;

import org.junit.platform.engine.TestExecutionResult;
import org.junit.platform.engine.support.descriptor.ClassSource;
import org.junit.platform.engine.support.descriptor.MethodSource;
import org.junit.platform.launcher.TestExecutionListener;
import org.junit.platform.launcher.TestIdentifier;

public final class Dump5 implements TestExecutionListener {
    @Override
    public void executionStarted(TestIdentifier id) {
        if (id.isTest()) Dump.started(cls(id));
        else if (id.getSource().orElse(null) instanceof ClassSource) Dump.classStarted(((ClassSource) id.getSource().get()).getClassName());
    }

    static String cls(TestIdentifier id) {
        Object s = id.getSource().orElse(null);
        return s instanceof MethodSource ? ((MethodSource) s).getClassName() : "?";
    }

    @Override
    public void executionFinished(TestIdentifier id, TestExecutionResult r) {
        if (!id.isTest()) {
            if (id.getSource().orElse(null) instanceof ClassSource) Dump.classPart(((ClassSource) id.getSource().get()).getClassName());
            return;
        }
        String cls = "?", m = id.getDisplayName();
        if (id.getSource().orElse(null) instanceof MethodSource) {
            MethodSource s = (MethodSource) id.getSource().get();
            cls = s.getClassName();
            m = s.getMethodName();
        }
        Dump.finished(cls, m, r.getStatus() != TestExecutionResult.Status.SUCCESSFUL);
    }
}
"""

ANALYZE = r"""
package gnx;

import java.nio.file.*;
import java.util.*;
import org.jacoco.core.analysis.*;
import org.jacoco.core.data.*;
import org.jacoco.core.internal.data.CRC64;
import org.jacoco.core.tools.ExecFileLoader;

/** args: classes dir, exec dir, out file. Every `<n>.exec` → one JSON line {"i": "n", "l": {"pkg/File.java": [lines that ran]}}. */
public final class Analyze {
    public static void main(String[] a) throws Exception {
        Path classes = Paths.get(a[0]), dir = Paths.get(a[1]);
        // a multi-release jar holds a class twice (`META-INF/versions/11/…`): the one whose id the data names ran
        Map<String, List<byte[]>> bytes = new HashMap<>();
        try (var w = Files.walk(classes)) {
            for (Path p : (Iterable<Path>) w.filter(p -> p.toString().endsWith(".class"))::iterator) {
                String rel = classes.relativize(p).toString().replaceFirst("^META-INF/versions/\\d+/", "");
                bytes.computeIfAbsent(rel.substring(0, rel.length() - 6), k -> new ArrayList<>()).add(Files.readAllBytes(p));
            }
        }
        List<Path> execs = new ArrayList<>();
        try (var s = Files.list(dir)) {
            s.filter(p -> p.toString().endsWith(".exec")).sorted().forEach(execs::add);
        }
        try (var out = new java.io.PrintWriter(Files.newBufferedWriter(Paths.get(a[2])))) {
            for (Path e : execs) {
                ExecFileLoader l = new ExecFileLoader();
                l.load(e.toFile());
                ExecutionDataStore store = l.getExecutionDataStore();
                CoverageBuilder b = new CoverageBuilder();
                Analyzer an = new Analyzer(store, b);
                for (ExecutionData d : store.getContents()) {
                    if (!d.hasHits()) continue;
                    for (byte[] c : bytes.getOrDefault(d.getName(), List.of())) {
                        if (CRC64.classId(c) == d.getId()) {
                            an.analyzeClass(c, d.getName());
                            break;
                        }
                    }
                }
                Map<String, TreeSet<Integer>> lines = new TreeMap<>();
                for (IClassCoverage cc : b.getClasses()) {
                    if (cc.getSourceFileName() == null) continue;
                    String f = (cc.getPackageName().isEmpty() ? "" : cc.getPackageName() + "/") + cc.getSourceFileName();
                    for (int i = cc.getFirstLine(); i > 0 && i <= cc.getLastLine(); i++) {
                        int st = cc.getLine(i).getStatus();
                        if (st == ICounter.FULLY_COVERED || st == ICounter.PARTLY_COVERED)
                            lines.computeIfAbsent(f, k -> new TreeSet<>()).add(i);
                    }
                }
                StringBuilder sb = new StringBuilder("{\"i\":\"").append(e.getFileName().toString().replace(".exec", "")).append("\",\"l\":{");
                String sep = "";
                for (var en : lines.entrySet()) {
                    sb.append(sep).append('"').append(en.getKey()).append("\":").append(en.getValue().toString().replace(" ", ""));
                    sep = ",";
                }
                out.println(sb.append("}}"));
            }
        }
    }
}
"""

INITS = r"""
package gnx;

import java.net.*;
import java.nio.file.*;
import java.util.*;

/** args: classes dir, out dir, classpath of the rest. Every class of the library is initialized on its own, in a
 *  class loader of its own (so all it needs is initialized again): `init-<n>.exec` is what its static blocks and
 *  field initializers ran, `inits.tsv` names the class. */
public final class Inits {
    public static void main(String[] a) throws Exception {
        Path classes = Paths.get(a[0]), out = Paths.get(a[1]);
        List<URL> urls = new ArrayList<>(List.of(classes.toUri().toURL()));
        for (String p : a[2].split(":")) if (!p.isEmpty()) urls.add(Paths.get(p).toUri().toURL());
        List<String> names = new ArrayList<>();
        try (var w = Files.walk(classes)) {
            w.filter(p -> p.toString().endsWith(".class") && !p.toString().contains("META-INF"))
             .forEach(p -> { String r = classes.relativize(p).toString(); names.add(r.substring(0, r.length() - 6).replace('/', '.')); });
        }
        Collections.sort(names);
        System.setProperty("gnx.dir", out.toString());
        Dump.take();
        int n = 0;
        try (var idx = new java.io.PrintWriter(Files.newBufferedWriter(out.resolve("inits.tsv")))) {
            for (String c : names) {
                try (URLClassLoader l = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getPlatformClassLoader())) {
                    Class.forName(c, true, l);
                } catch (Throwable e) {
                    // it needs what a test sets up: what ran until it threw still counts
                }
                Files.write(out.resolve("init-" + n + ".exec"), Dump.take());
                idx.println(n++ + "\t" + c);
            }
        }
    }
}
"""


def jar(group: str, art: str, ver: str, cls: str = "") -> Path:
    """A jar from the local Maven repository, fetched once (needs the network the first time)."""
    p = M2 / group.replace(".", "/") / art / ver / f"{art}-{ver}{'-' + cls if cls else ''}.jar"
    if not p.exists():
        code, out = run.sh(f"mvn -q -B dependency:get -Dartifact={group}:{art}:{ver}:jar{':' + cls if cls else ''}",
                           Path.cwd(), timeout=900)
        if code or not p.exists():
            raise RuntimeError(f"no {p.name}: {out[-400:]}")
    return p


def javac(work: Path, name: str, sources: dict[str, str], cp: list[Path]) -> Path:
    src, out = work / name / "src", work / name / "classes"
    for f, text in sources.items():
        (src / "gnx").mkdir(parents=True, exist_ok=True)
        (src / "gnx" / f"{f}.java").write_text(text)
    out.mkdir(parents=True, exist_ok=True)
    code, o = run.sh(f"javac -nowarn --release 11 {'-cp ' + ':'.join(map(str, cp)) if cp else ''} -d {out} {src}/gnx/*.java", work)
    if code:
        raise RuntimeError(f"javac {name}: {o[-800:]}")
    return out


def test_classpath(root: Path, work: Path) -> list[Path]:
    f = work / "test-cp.txt"
    code, o = run.sh(f"mvn -q -B -o dependency:build-classpath -Dmdep.includeScope=test -Dmdep.outputFile={f}", root,
                     timeout=900)
    if code:
        raise RuntimeError(f"test classpath: {o[-400:]}")
    return [Path(x) for x in f.read_text().strip().split(":") if x]


def helpers(root: Path, work: Path) -> dict:
    """The agent, the listener (on the test classpath through `${gnx.cp}`) and `Analyze`.
    Surefire 3 runs JUnit 4 through the JUnit Platform too (vintage), so one listener of the platform serves both."""
    cp = test_classpath(root, work)
    m = next((re.match(r"junit-platform-engine-([\d.]+)\.jar$", p.name) for p in cp
              if p.name.startswith("junit-platform-engine-")), None)
    ver = m.group(1) if m else PLATFORM
    plat = [jar("org.junit.platform", a, ver) for a in ("junit-platform-launcher", "junit-platform-engine", "junit-platform-commons")]
    listener = javac(work, "dump", {"Dump": DUMP, "Dump5": DUMP5}, cp + plat + [jar("org.opentest4j", "opentest4j", "1.3.0")])
    svc = listener / "META-INF" / "services"
    svc.mkdir(parents=True, exist_ok=True)
    (svc / "org.junit.platform.launcher.TestExecutionListener").write_text("gnx.Dump5\n")
    core = jar("org.jacoco", "org.jacoco.core", JACOCO)
    asm = [jar("org.ow2.asm", a, "9.9") for a in ("asm", "asm-commons", "asm-tree")]
    analyze = javac(work, "analyze", {"Analyze": ANALYZE}, [core, *asm])
    inits = javac(work, "inits", {"Dump": DUMP, "Inits": INITS}, [])
    return {"agent": jar("org.jacoco", "org.jacoco.agent", JACOCO, "runtime"), "listener": listener,
            "junit": 5 if m else 4, "analyze_cp": [analyze, core, *asm], "inits": inits, "cp": cp}


def gnx_pom(root: Path) -> str:
    """A copy of pom.xml whose surefire takes `${gnx.argLine}` (the agent) and `${gnx.cp}` (the listener)."""
    p = (root / "pom.xml").read_text()
    el = "<additionalClasspathElement>${gnx.cp}</additionalClasspathElement>"
    m = re.search(r"<artifactId>maven-surefire-plugin</artifactId>.*?</plugin>", p, re.S)
    if m and "<configuration>" in m.group(0):
        blk = m.group(0)
        if "<argLine>" in blk:
            blk = re.sub(r"<argLine>(.*?)</argLine>", lambda x: f"<argLine>{x.group(1)} ${{gnx.argLine}}</argLine>",
                         blk, count=1, flags=re.S)
        else:
            blk = blk.replace("<configuration>", "<configuration><argLine>${gnx.argLine}</argLine>", 1)
        if "<additionalClasspathElements>" not in blk:
            blk = blk.replace("<configuration>", "<configuration><additionalClasspathElements></additionalClasspathElements>", 1)
        p = p[:m.start()] + blk + p[m.end():]
    else:
        plug = ("<plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-surefire-plugin</artifactId>"
                "<configuration><argLine>${gnx.argLine}</argLine>"
                "<additionalClasspathElements></additionalClasspathElements></configuration></plugin>")
        i = p.index("<plugins>", p.index("<build>")) + len("<plugins>")
        p = p[:i] + plug + p[i:]
    # every list of extra classpath elements (a profile's too: it replaces the main one) gets the listener
    p = p.replace("<additionalClasspathElements>", "<additionalClasspathElements>" + el)
    (root / ".gnx-pom.xml").write_text(p)
    return ".gnx-pom.xml"


def package_of(root: Path) -> str:
    """The project's top package (`org.jsoup`): what the agent instruments."""
    pk = set()
    for f in (root / "src" / "main" / "java").rglob("*.java"):
        m = re.search(r"^package\s+([\w.]+)\s*;", f.read_text(errors="replace"), re.M)
        if m:
            pk.add(m.group(1))
    parts = [x.split(".") for x in pk]
    pre = []
    for col in zip(*parts):
        if len(set(col)) > 1:
            break
        pre.append(col[0])
    return ".".join(pre) or "*"


def analyze(h: dict, root: Path, d: Path) -> tuple[dict[str, dict[str, set[int]]], float]:
    """`<n>.exec` in d → n -> file (relative to the repository) -> lines that ran."""
    t = time.time()
    out = d / "lines.jsonl"
    code, o = run.sh(f"java -cp {':'.join(map(str, h['analyze_cp']))} gnx.Analyze {root}/target/classes {d} {out}", root,
                     timeout=3600)
    if code:
        raise RuntimeError(f"Analyze: {o[-800:]}")
    rel: dict[str, str] = {}
    for f in (root / "src" / "main").rglob("*.java"):
        rel.setdefault(str(f.relative_to(root)).split("/java/", 1)[-1].split("/java11/", 1)[-1], str(f.relative_to(root)))
    res = {}
    for line in out.read_text().splitlines():
        j = json.loads(line)
        res[j["i"]] = {rel.get(f, f): set(ls) for f, ls in j["l"].items()}
    return res, time.time() - t


def bytes_of(d: Path) -> int:
    return sum(p.stat().st_size for p in d.glob("*.exec"))


def per_test(root: Path, h: dict, work: Path, pkg: str, fork: bool = False) -> tuple[dict, dict]:
    d = work / "exec-test"
    d.mkdir(parents=True, exist_ok=True)
    pom = gnx_pom(root)
    t = time.time()
    code, out = run.sh(f"mvn -B -o -f {pom} test -Dsurefire.printSummary=true "
                       f"'-Dgnx.argLine=-javaagent:{h['agent']}=output=none,includes={pkg}.*,jmx=false -Dgnx.dir={d}' "
                       f"-Dgnx.cp={h['listener']} {'-DreuseForks=false' if fork else ''} > {work}/per-test.log 2>&1", root, timeout=7200)
    mvn_s = time.time() - t
    (root / pom).unlink(missing_ok=True)
    lines, an_s = analyze(h, root, d)
    tests: dict = {}
    shared: dict[str, dict[str, set[int]]] = {}  # class -> what its setup and teardown ran
    for row in (d / "index.tsv").read_text().splitlines():
        n, cls, m, failed = row.split("\t")
        if not m:
            for f, ls in lines.get(n, {}).items():
                shared.setdefault(cls, {}).setdefault(f, set()).update(ls)
            continue
        t = tests.setdefault(f"{cls}#{m}", {"name": m, "failed": False, "lines": {}})
        t["failed"] |= failed == "1"
        for f, ls in lines.get(n, {}).items():
            t["lines"].setdefault(f, set()).update(ls)
    for tid, t in tests.items():
        for f, ls in shared.get(tid.split("#")[0], {}).items():
            t["lines"].setdefault(f, set()).update(ls)
    init_s, by_file = class_inits(root, h, work, pkg)
    if INIT:
        for t in tests.values():
            for f in list(t["lines"]):
                for g, ls in by_file.get(f, {}).items():
                    t["lines"].setdefault(g, set()).update(ls)
    return tests, {"how": "per-test-fork" if fork else "per-test", "class_inits_s": round(init_s, 1), "class_inits_booked": INIT,
                   "files_with_init_lines": len(by_file), "mvn_s": round(mvn_s, 1), "analyze_s": round(an_s, 1), "map_s": round(mvn_s + an_s + init_s, 1),
                   "exec_bytes": bytes_of(d), "dumps": len(lines), "test_methods": len(tests),
                   "classes": len({k.split("#")[0] for k in tests}), "classes_with_setup_lines": len([c for c, v in shared.items() if v]),
                   "run_failed": bool(code)}


INIT = True  # book what a library class's initialization runs to every test that runs a line of its file


def class_inits(root: Path, h: dict, work: Path, pkg: str) -> tuple[float, dict[str, dict[str, set[int]]]]:
    """A library class's static blocks run once a JVM, in whichever test touches it first: in the others a function
    they call never shows up, though it throwing fails them all. So: what initializing each class on its own runs,
    by its file (relative) → file → lines."""
    d = work / "exec-init"
    d.mkdir(parents=True, exist_ok=True)
    t = time.time()
    cp = ":".join(map(str, [h["inits"], *h["cp"]]))
    rest = ":".join(str(p) for p in h["cp"] if p.suffix == ".jar")
    code, o = run.sh(f"java -javaagent:{h['agent']}=output=none,includes={pkg}.*,jmx=false -cp {cp} gnx.Inits "
                     f"{root}/target/classes {d} {rest} > {work}/inits.log 2>&1", root, timeout=1800)
    lines, an_s = analyze(h, root, d)
    names = dict(r.split("\t") for r in (d / "inits.tsv").read_text().splitlines()) if (d / "inits.tsv").exists() else {}
    rel = {}
    for f in (root / "src" / "main").rglob("*.java"):
        rel.setdefault(str(f.relative_to(root)).split("/java/", 1)[-1].split("/java11/", 1)[-1], str(f.relative_to(root)))
    by_file: dict[str, dict[str, set[int]]] = {}
    for n, c in names.items():
        src = rel.get(c.split("$")[0].replace(".", "/") + ".java")
        if not src:
            continue
        for g, ls in lines.get(f"init-{n}", {}).items():
            by_file.setdefault(src, {}).setdefault(g, set()).update(ls)
    return time.time() - t, by_file


def surefire_cases(root: Path) -> dict[str, dict[str, bool]]:
    """class -> method -> failed, from the XML reports of the last surefire run."""
    out: dict[str, dict[str, bool]] = {}
    for f in (root / "target" / "surefire-reports").glob("TEST-*.xml"):
        try:
            x = ET.parse(f).getroot()
        except ET.ParseError:
            continue
        for c in x.iter("testcase"):
            m = re.sub(r"[\[(].*$", "", c.get("name", ""))
            bad = c.find("failure") is not None or c.find("error") is not None
            cl = out.setdefault(c.get("classname", ""), {})
            cl[m] = cl.get(m, False) or bad
    return out


def per_class(root: Path, h: dict, work: Path, pkg: str) -> tuple[dict, dict]:
    d = work / "exec-class"
    d.mkdir(parents=True, exist_ok=True)
    classes = sorted(surefire_cases(root))  # the plain run of `measure` left its reports
    empty = work / "empty"
    empty.mkdir(exist_ok=True)
    pom = gnx_pom(root)
    t = time.time()
    cases: dict[str, dict[str, bool]] = {}
    for i, c in enumerate(classes):
        run.sh(f"rm -rf target/surefire-reports; mvn -q -B -o -f {pom} test -Dtest={c} -Dsurefire.failIfNoSpecifiedTests=false "
               f"'-Dgnx.argLine=-javaagent:{h['agent']}=destfile={d}/{i:04d}.exec,includes={pkg}.*,jmx=false' "
               f"-Dgnx.cp={empty} > {work}/per-class.log 2>&1", root, timeout=1800)
        cases[f"{i:04d}"] = surefire_cases(root).get(c, {})
    mvn_s = time.time() - t
    (root / pom).unlink(missing_ok=True)
    lines, an_s = analyze(h, root, d)
    tests: dict = {}
    for i, c in enumerate(classes):
        ls = lines.get(f"{i:04d}", {})
        for m, failed in cases[f"{i:04d}"].items():
            tests[f"{c}#{m}"] = {"name": m, "failed": failed, "lines": ls}
    return tests, {"how": "per-class", "mvn_s": round(mvn_s, 1), "analyze_s": round(an_s, 1), "map_s": round(mvn_s + an_s, 1),
                   "exec_bytes": bytes_of(d), "dumps": len(lines), "test_methods": len(tests), "classes": len(classes)}


def body_owner(root: Path, syms: list[dict]) -> dict[str, list]:
    """file -> list indexed by line: the innermost function whose body holds that line (or None)."""
    by_file: dict[str, list[dict]] = {}
    for s in lg.fn_syms(syms):
        by_file.setdefault(s["file"], []).append(s)
    out = {}
    for f, ss in by_file.items():
        arr = [None] * (max(s["end_line"] for s in ss) + 2)
        for s in sorted(ss, key=lambda s: s["start_line"] - s["end_line"]):  # widest first, inner ones overwrite
            # a one-line method (`int f() { return x; }`) has no line ending in `{`: all of it is its body
            b = run.body_start(root, s, "java") or s["start_line"]
            for i in range(b, s["end_line"] + 1):
                arr[i] = s["qualified"]
        out[f] = arr
    return out


CLASS_OF: dict[str, list[str]] = {}  # bare test name -> the test classes with a method of that name
SCORE = lg.lt.score


def to_classes(names) -> list[str]:
    return list(dict.fromkeys(c for n in names for c in CLASS_OF.get(n, ())))


def score_both(order: list[str], truth: set[str]) -> dict:
    """The eval's score by method name, and the same by test class: did the class with a failing test come up."""
    d = SCORE(order, truth)
    d["by_class"] = SCORE(to_classes(order), set(to_classes(truth)))
    return d


def collector(how: str, owner: str):
    def collect(root: Path, syms: list[dict], work: Path) -> tuple[dict, dict]:
        h = helpers(root, work)
        pkg = package_of(root)
        if how == "per-class":
            tests, cost = per_class(root, h, work, pkg)
        else:
            tests, cost = per_test(root, h, work, pkg, fork=how == "per-test-fork")
        CLASS_OF.clear()
        for tid in sorted(tests):
            CLASS_OF.setdefault(tests[tid]["name"], []).append(tid.split("#")[0].rsplit(".", 1)[-1])
        cost["names_in_several_classes"] = sum(1 for v in CLASS_OF.values() if len(v) > 1)
        lg.lt.score = score_both
        cost.update(junit=h["junit"], package=pkg, owner=owner)
        if owner == "body":
            ob = body_owner(root, syms)
            lg.line_owner = lambda _syms: ob  # measure books lines through line_owner: use the body-only one
        else:
            lg.line_owner = LINE_OWNER
        return tests, cost
    return collect


LINE_OWNER = lg.line_owner


def java_library(file: str) -> bool:
    return file.endswith(".java") and "/src/test/" not in f"/{file}" and not file.startswith("src/test/")


def mean_score(res: dict, src: str, level: str, group: str = "all") -> dict:
    rs = [r for r in res["functions"] if r.get("score") and (group == "all" or r["group"] == group)]
    sc = [r["score"][src] if level == "method" else r["score"][src]["by_class"] for r in rs]
    return {k: lg.lt.mean([x[k] for x in sc]) for k in ("recall", "precision", "top_precision", "caught")} | {"functions": len(rs)}


def render(by_how: dict[str, dict]) -> str:
    first = next(iter(by_how.values()))
    L = [f"# learn-tests на {first['repo']} (java): сводка", "",
         "Все функции (pick + случайные). Полнота / точность / из 10 падают / поймано первыми 10. По методу — как истина "
         "eval (имя метода теста); по классу — класс, где есть падающий метод.", "",
         "| способ | единица | функций | граф (`tests-for`) | покрытие | объединение | карта, с | × обычного |",
         "|---|---|---:|---|---|---|---:|---:|"]
    for how, res in by_how.items():
        for level in ("method", "class"):
            cells = []
            for src in lg.SOURCES:
                m = mean_score(res, src, level)
                cells.append(" / ".join(lg.pct(m[k]) for k in ("recall", "precision", "top_precision", "caught")))
            k = res["cost"]
            L.append(f"| {how} | {'метод' if level == 'method' else 'класс'} | {mean_score(res, 'gitnext', level)['functions']} | "
                     + " | ".join(cells) + f" | {k['map_s']} | {k['slowdown_runs']} |")
    L.append("")
    for how, res in by_how.items():
        md = lg.render(res).replace("при panic в начале функции", "при throw в начале функции")
        md = md.replace("# learn-tests на", f"# learn-tests ({how}) на", 1)
        L.append(md)
    L += ["# По классу теста", "",
          "Класс засчитан, если в нём есть метод с падающим именем (имя в нескольких классах — все они).", "",
          "| способ | выборка | функций | источник | полнота | точность | из 10 падают | поймано |",
          "|---|---|---:|---|---:|---:|---:|---:|"]
    for how, res in by_how.items():
        for g in ("pick", "random", "all"):
            rs = [r for r in res["functions"] if r.get("score") and (g == "all" or r["group"] == g)]
            for src in lg.SOURCES:
                m = {k: lg.lt.mean([r["score"][src]["by_class"][k] for r in rs]) for k in
                     ("recall", "precision", "top_precision", "caught")}
                L.append(f"| {how} | {g} | {len(rs)} | {src} | {lg.pct(m['recall'])} | {lg.pct(m['precision'])} | "
                         f"{lg.pct(m['top_precision'])} | {lg.pct(m['caught'])} |")
    L += ["", "# Цена", "", "| | " + " | ".join(by_how) + " |", "|---|" + "---:|" * len(by_how)]
    keys = ["plain_s", "mvn_s", "analyze_s", "map_s", "slowdown_runs", "exec_bytes", "map_sqlite_bytes", "dumps",
            "test_methods", "classes", "names_in_several_classes", "map_pairs", "map_functions", "functions_with_body", "functions_never_run"]
    for k in keys:
        L.append(f"| {k} | " + " | ".join(str(r["cost"].get(k, "")) for r in by_how.values()) + " |")
    L.append(f"\n{first['repo']} {first['commit']}\n")
    return "\n".join(L)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repos", default="jsoup,json-java")
    ap.add_argument("--n2", type=int, default=12, help="random functions per repo, besides the cached ones")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--repeat", type=int, default=3, help="timed plain test runs")
    ap.add_argument("--pick-from", help="truth.json as the eval left it: its functions are the `pick` group")
    ap.add_argument("--how", default=",".join(HOWS), help="comma-separated: per-test, per-test-fork, per-class")
    ap.add_argument("--render", action="store_true", help="only rewrite the .md files from the .json ones")
    ap.add_argument("--owner", default="body", choices=("body", "line"),
                    help="a line belongs to the function whose body holds it, or to the one holding it at all")
    a = ap.parse_args()
    repos = {r["name"]: r for r in json.loads((run.HERE / "repos.json").read_text())}
    plain = run.LANG["java"]["tests"]
    for name in a.repos.split(","):
        out = run.HERE / "results" / f"learn-tests-{name}"
        suffix = "" if a.owner == "body" else f".{a.owner}"
        # a way measured again replaces its old result, the other one stays
        old = Path(f"{out}{suffix}.json")
        by_how = json.loads(old.read_text()) if old.exists() else {}
        for how in [] if a.render else a.how.split(","):
            by_how[how] = lg.measure(repos[name], collector(how, a.owner), plain, java_library, a)
        by_how = {h: by_how[h] for h in HOWS if h in by_how}
        Path(f"{out}{suffix}.json").write_text(json.dumps(by_how, ensure_ascii=False, indent=1))
        md = render(by_how)
        Path(f"{out}{suffix}.md").write_text(md)
        print("\n" + md, flush=True)


if __name__ == "__main__":
    main()
