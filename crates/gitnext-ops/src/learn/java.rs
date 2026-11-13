//! Java: Maven's own test run with the JaCoCo agent, whose data is taken and reset around every
//! test by a listener of the JUnit Platform, so each test gets the lines it ran itself. Measured on
//! JSON-java and jsoup: the map finds the failing tests of 99% and 94% of broken functions (the
//! call graph: 71% and 29%) for 2–3.5× a plain test run.
//!
//! Three small Java classes do the part that has to run inside a JVM (`learn/java/*.java`); they
//! are compiled with the project's `javac` into `.gitnext/` and removed after the run. A class's
//! static blocks run once in a JVM, in whichever test touches the class first; what they run is
//! measured apart and booked to every test that runs a line of the class's file.
//!
//! Maven only, and tests that run on the JUnit Platform: JUnit 5, and JUnit 4 under a surefire
//! that runs it there (3.6 and newer). Gradle and TestNG are not learned. The JaCoCo jars are
//! taken from the local Maven repository; nothing is downloaded here.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use gitnext_index::Graph;

use super::{owners_at, Run};

const JACOCO: &str = "0.8.14";
/// The ASM that JaCoCo of that version is built with.
const ASM: &str = "9.9";
/// The JUnit Platform named when the local repository has none to compile the listener against.
const PLATFORM: &str = "1.14.4";
const TEST_LIMIT: Duration = Duration::from_secs(3 * 3600);
const STEP_LIMIT: Duration = Duration::from_secs(1800);
/// How many names a note lists before it says "and N more".
const NAMES: usize = 5;
/// The copy of `pom.xml` the tests are run from; it has to stand next to the original.
const POM: &str = ".gitnext-pom.xml";

const SOURCES: [(&str, &str); 4] = [
    ("Dump", include_str!("java/Dump.java")),
    ("Listener", include_str!("java/Listener.java")),
    ("Analyze", include_str!("java/Analyze.java")),
    ("Inits", include_str!("java/Inits.java")),
];

pub(super) fn run(root: &Path, g: &Graph) -> Result<Run> {
    run_with("mvn", root, g)
}

/// The compiled helpers, the coverage data and the copy of the pom; removed however `run` ends.
struct Scratch(PathBuf, PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file(&self.1);
    }
}

/// What a command printed (stdout and stderr together) and how it ended.
struct Ran {
    ok: bool,
    timed_out: bool,
    text: String,
}

fn timed(cmd: &mut Command, log: &Path, limit: Duration) -> Result<Ran> {
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    let mut child = cmd.stdin(Stdio::null()).stdout(out).stderr(err).spawn()?;
    let started = Instant::now();
    let (ok, timed_out) = loop {
        match child.try_wait()? {
            Some(status) => break (status.success(), false),
            None if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                break (false, true);
            }
        }
    };
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    Ok(Ran { ok, timed_out, text })
}

/// The last lines of a build log that say what went wrong: Maven's `[ERROR]` lines, else the tail.
fn why_failed(text: &str) -> String {
    let errors: Vec<&str> = text.lines().filter(|l| l.starts_with("[ERROR]") && l.len() > 8).take(4).collect();
    if errors.is_empty() {
        text.lines().rev().filter(|l| !l.trim().is_empty()).take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    } else {
        errors.join("\n")
    }
}

fn some(names: &[String]) -> String {
    let mut s = names.iter().take(NAMES).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > NAMES {
        s.push_str(&format!(" and {} more", names.len() - NAMES));
    }
    s
}

/// Learning can start: a single-module Maven project, and a `mvn` and a `javac` that run.
fn ready(mvn: &str, root: &Path) -> Result<String> {
    let Ok(pom) = std::fs::read_to_string(root.join("pom.xml")) else {
        if ["build.gradle", "build.gradle.kts"].iter().any(|f| root.join(f).exists()) {
            bail!("the project builds with Gradle; learn-tests knows Maven only");
        }
        bail!("no pom.xml at the root of the repository");
    };
    if pom.contains("<modules>") {
        bail!("a Maven project of several modules is not learned yet");
    }
    for (tool, flag, what) in [(mvn, "--version", "install Maven (https://maven.apache.org)"), ("javac", "-version", "install a JDK")] {
        match Command::new(tool).arg(flag).stdin(Stdio::null()).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => bail!("`{tool} {flag}` failed: {}", why_failed(&String::from_utf8_lossy(&o.stderr))),
            Err(_) => bail!("`{tool}` is not in PATH: {what} to learn which test runs which function"),
        }
    }
    Ok(pom)
}

/// The local Maven repository: the folder above a jar of the test classpath that holds Maven's
/// own plugins (`org/apache/maven`), else `~/.m2/repository`.
fn repository(classpath: &[PathBuf]) -> PathBuf {
    let above = classpath.iter().filter(|p| p.extension().is_some_and(|e| e == "jar")).find_map(|jar| jar.ancestors().skip(3).find(|a| a.join("org/apache/maven").is_dir()).map(Path::to_path_buf));
    above.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default()).join(".m2").join("repository"))
}

/// The versions of an artifact the repository holds, newest last (by their numbers).
fn versions(repo: &Path, group: &str, artifact: &str) -> Vec<String> {
    let dir = repo.join(group.replace('.', "/")).join(artifact);
    let mut found: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    found.retain(|v| jar(repo, group, artifact, v, "").exists());
    found.sort_by_key(|v| v.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok()).collect::<Vec<_>>());
    found
}

fn jar(repo: &Path, group: &str, artifact: &str, version: &str, classifier: &str) -> PathBuf {
    let dash = if classifier.is_empty() { String::new() } else { format!("-{classifier}") };
    repo.join(group.replace('.', "/")).join(artifact).join(version).join(format!("{artifact}-{version}{dash}.jar"))
}

/// A copy of `pom.xml` whose surefire takes `${gitnext.argLine}` (the agent) and `${gitnext.cp}`
/// (the listener) on top of what the project sets itself.
fn learning_pom(pom: &str) -> Option<String> {
    const ARG: &str = "${gitnext.argLine}";
    const ELEMENTS: &str = "<additionalClasspathElements>";
    let mut p = pom.to_string();
    let block = p.find("<artifactId>maven-surefire-plugin</artifactId>").and_then(|a| p[a..].find("</plugin>").map(|e| (a, a + e)));
    match block {
        Some((a, e)) if p[a..e].contains("<configuration>") => {
            let mut b = p[a..e].to_string();
            match b.find("</argLine>") {
                Some(i) => b.insert_str(i, &format!(" {ARG}")),
                None => b = b.replacen("<configuration>", &format!("<configuration><argLine>{ARG}</argLine>"), 1),
            }
            if !b.contains(ELEMENTS) {
                b = b.replacen("<configuration>", &format!("<configuration>{ELEMENTS}</additionalClasspathElements>"), 1);
            }
            p.replace_range(a..e, &b);
        }
        _ => {
            let plugin = format!(
                "<plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-surefire-plugin</artifactId><configuration><argLine>{ARG}</argLine>{ELEMENTS}</additionalClasspathElements></configuration></plugin>"
            );
            match p.find("<build>").and_then(|b| p[b..].find("<plugins>").map(|i| b + i + "<plugins>".len())) {
                Some(i) => p.insert_str(i, &plugin),
                None => {
                    let end = p.rfind("</project>")?;
                    p.insert_str(end, &format!("<build><plugins>{plugin}</plugins></build>"));
                }
            }
        }
    }
    // every list of extra classpath elements (a profile's too: it replaces the main one) gets the listener
    Some(p.replace(ELEMENTS, &format!("{ELEMENTS}<additionalClasspathElement>${{gitnext.cp}}</additionalClasspathElement>")))
}

/// `package a.b.c;` of a Java file.
fn package_of(text: &str) -> &str {
    text.lines().map(str::trim).find_map(|l| l.strip_prefix("package ")?.split(';').next()).map(str::trim).unwrap_or("")
}

/// What the agent instruments: the classes under the package all of the project's code shares.
fn includes<'a>(packages: impl Iterator<Item = &'a str>) -> String {
    let mut common: Option<Vec<&str>> = None;
    for p in packages {
        let parts: Vec<&str> = p.split('.').filter(|x| !x.is_empty()).collect();
        common = Some(match common {
            None => parts,
            Some(c) => c.iter().zip(&parts).take_while(|(a, b)| a == b).map(|(a, _)| *a).collect(),
        });
    }
    match common {
        Some(c) if !c.is_empty() => format!("{}.*", c.join(".")),
        _ => "*".to_string(),
    }
}

/// What `Analyze` wrote: the dump's name → source file (`org/x/A.java`) → lines that ran.
fn ran_lines(tsv: &str) -> HashMap<&str, Vec<(&str, Vec<u32>)>> {
    let mut by_dump: HashMap<&str, Vec<(&str, Vec<u32>)>> = HashMap::new();
    for row in tsv.lines() {
        let mut col = row.split('\t');
        if let (Some(n), Some(file), Some(lines)) = (col.next(), col.next(), col.next()) {
            by_dump.entry(n).or_default().push((file, lines.split(',').filter_map(|l| l.parse().ok()).collect()));
        }
    }
    by_dump
}

/// The JVM tools the run needs, found in the local Maven repository.
struct Jars {
    agent: PathBuf,
    analyze: Vec<PathBuf>,
    /// what the listener is compiled against, on top of the test classpath
    platform: Vec<PathBuf>,
}

/// `platform`: the version of the JUnit Platform on the test classpath, when it is there.
fn jars(repo: &Path, platform: Option<&str>) -> Result<Jars> {
    const GROUP: &str = "org.junit.platform";
    let agent = jar(repo, "org.jacoco", "org.jacoco.agent", JACOCO, "runtime");
    let analyze: Vec<PathBuf> =
        std::iter::once(jar(repo, "org.jacoco", "org.jacoco.core", JACOCO, "")).chain(["asm", "asm-commons", "asm-tree"].map(|a| jar(repo, "org.ow2.asm", a, ASM, ""))).collect();
    // the listener is only compiled against these; surefire brings its own, often of another version
    let all = |v: &str| ["junit-platform-launcher", "junit-platform-engine", "junit-platform-commons"].map(|a| jar(repo, GROUP, a, v, ""));
    let have = |v: &str| all(v).iter().all(|j| j.exists());
    let version = platform.filter(|v| have(v)).map(String::from).or_else(|| versions(repo, GROUP, "junit-platform-launcher").into_iter().rev().find(|v| have(v)));
    let mut get = Vec::new();
    if !analyze.iter().all(|j| j.exists()) {
        get.push(format!("org.jacoco:org.jacoco.core:{JACOCO}"));
    }
    if !agent.exists() {
        get.push(format!("org.jacoco:org.jacoco.agent:{JACOCO}:jar:runtime"));
    }
    if version.is_none() {
        get.push(format!("{GROUP}:junit-platform-launcher:{}", platform.unwrap_or(PLATFORM)));
    }
    if !get.is_empty() {
        let commands: Vec<String> = get.iter().map(|a| format!("mvn dependency:get -Dartifact={a}")).collect();
        bail!("what measures the lines a test runs (JaCoCo {JACOCO}, the JUnit Platform launcher) is not in the local Maven repository; get it once: {}", commands.join(" && "));
    }
    Ok(Jars { agent, analyze, platform: version.map(|v| all(&v).to_vec()).unwrap_or_default() })
}

fn javac(dir: &Path, names: &[&str], classpath: &[PathBuf]) -> Result<PathBuf> {
    let (src, out) = (dir.join("src").join("gitnextlearn"), dir.join("classes"));
    std::fs::create_dir_all(&src)?;
    std::fs::create_dir_all(&out)?;
    let mut cmd = Command::new("javac");
    cmd.args(["-nowarn", "--release", "11", "-d"]).arg(&out);
    if !classpath.is_empty() {
        cmd.arg("-cp").arg(std::env::join_paths(classpath)?);
    }
    for name in names {
        let file = src.join(format!("{name}.java"));
        std::fs::write(&file, SOURCES.iter().find(|(n, _)| n == name).map(|(_, text)| *text).unwrap_or_default())?;
        cmd.arg(file);
    }
    let ran = timed(&mut cmd, &dir.join("javac.log"), STEP_LIMIT)?;
    if !ran.ok {
        bail!("javac could not compile the helper that splits coverage by test (a JDK 11 or newer is needed):\n{}", why_failed(&ran.text));
    }
    Ok(out)
}

/// The dumps of a directory as lines by source file: runs `Analyze` over it.
fn analyze(root: &Path, classes: &Path, jars: &Jars, dumps: &Path) -> Result<String> {
    let out = dumps.join("lines.tsv");
    let classpath = std::env::join_paths(std::iter::once(classes.to_path_buf()).chain(jars.analyze.iter().cloned()))?;
    let mut cmd = Command::new("java");
    cmd.arg("-cp").arg(classpath).arg("gitnextlearn.Analyze").arg(root.join("target").join("classes")).arg(dumps).arg(&out).current_dir(root);
    let ran = timed(&mut cmd, &dumps.join("analyze.log"), STEP_LIMIT)?;
    if !ran.ok {
        bail!("reading the coverage data failed:\n{}", why_failed(&ran.text));
    }
    Ok(std::fs::read_to_string(&out).unwrap_or_default())
}

fn run_with(mvn: &str, root: &Path, g: &Graph) -> Result<Run> {
    let pom = ready(mvn, root)?;
    let started = Instant::now();
    let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if root_abs.to_string_lossy().contains(char::is_whitespace) {
        bail!("the path of the repository has a space in it; the JVM options of a Maven test run cannot carry it");
    }
    // the Java files of the index by what the JVM calls them: `org/x/A.java` (package + file name)
    let owner_of = owners_at(g, true);
    let (mut by_jvm_name, mut packages): (HashMap<String, &str>, Vec<String>) = (HashMap::new(), Vec::new());
    let mut files: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).filter(|f| f.ends_with(".java")).collect();
    files.sort_by_key(|f| (!f.contains("src/main/"), *f)); // the code first: a test helper of the same name does not shadow it
    for file in files {
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
        let package = package_of(&text);
        let name = file.rsplit('/').next().unwrap_or(file);
        by_jvm_name.entry(if package.is_empty() { name.to_string() } else { format!("{}/{name}", package.replace('.', "/")) }).or_insert(file);
        if owner_of.contains_key(file) && file.contains("src/main/") {
            packages.push(package.to_string());
        }
    }
    if packages.is_empty() {
        bail!("the index has no Java function under src/main/");
    }
    let includes = includes(packages.iter().map(String::as_str));
    // the tests of the index: (file, method)
    let tests: HashSet<(&str, &str)> = g.syms.iter().filter(|s| s.sym.test && s.file.ends_with(".java")).map(|s| (s.file.as_str(), s.sym.name.as_str())).collect();

    let dir = root_abs.join(".gitnext").join(format!("learn-java-{}", std::process::id()));
    let scratch = Scratch(dir.clone(), root.join(POM));
    std::fs::create_dir_all(&dir)?;
    let cp_file = dir.join("test-cp.txt");
    let mut cmd = Command::new(mvn);
    cmd.args(["-q", "-B", "dependency:build-classpath", "-Dmdep.includeScope=test"]).arg(format!("-Dmdep.outputFile={}", cp_file.display())).current_dir(root);
    let ran = timed(&mut cmd, &dir.join("cp.log"), STEP_LIMIT).with_context(|| format!("running `{mvn} dependency:build-classpath`"))?;
    let Ok(listed) = std::fs::read_to_string(&cp_file) else {
        bail!("`{mvn} dependency:build-classpath` failed:\n{}", why_failed(&ran.text));
    };
    let classpath: Vec<PathBuf> = std::env::split_paths(listed.trim()).collect();
    // JUnit 5 has the platform on the test classpath; JUnit 4 gets it from surefire 3.6 and newer
    let platform = classpath.iter().find_map(|p| p.file_name()?.to_str()?.strip_prefix("junit-platform-engine-")?.strip_suffix(".jar").map(String::from));
    let jars = jars(&repository(&classpath), platform.as_deref())?;

    let listener = javac(&dir.join("listener"), &["Dump", "Listener"], &classpath.iter().chain(&jars.platform).cloned().collect::<Vec<_>>())?;
    std::fs::create_dir_all(listener.join("META-INF/services"))?;
    std::fs::write(listener.join("META-INF/services/org.junit.platform.launcher.TestExecutionListener"), "gitnextlearn.Listener\n")?;
    let analyzer = javac(&dir.join("analyze"), &["Analyze"], &jars.analyze)?;
    let inits = javac(&dir.join("inits"), &["Dump", "Inits"], &[])?;

    let Some(copy) = learning_pom(&pom) else { bail!("pom.xml has no </project>") };
    std::fs::write(&scratch.1, copy)?;
    let dumps = dir.join("tests");
    let agent = format!("-javaagent:{}=output=none,includes={includes},jmx=false", jars.agent.display());
    let mut cmd = Command::new(mvn);
    cmd.args(["-B", "-f", POM, "test", "-Djacoco.skip=true"])
        .arg(format!("-Dgitnext.argLine={agent} -Dgitnext.dir={}", dumps.display()))
        .arg(format!("-Dgitnext.cp={}", listener.display()))
        .current_dir(root);
    let test_run = timed(&mut cmd, &dir.join("test.log"), TEST_LIMIT).with_context(|| format!("running `{mvn} test`"))?;
    let Ok(index) = std::fs::read_to_string(dumps.join("index.tsv")) else {
        if test_run.timed_out {
            bail!("`{mvn} test` with the coverage agent took longer than {} hours", TEST_LIMIT.as_secs() / 3600);
        }
        if platform.is_none() && test_run.text.contains("Tests run:") {
            bail!("the tests ran, but not on the JUnit Platform: JUnit 4 under a surefire older than 3.6, and TestNG, are not learned");
        }
        bail!("`{mvn} test` with the coverage agent recorded no test:\n{}", why_failed(&test_run.text));
    };
    let lines = analyze(root, &analyzer, &jars, &dumps)?;
    let by_dump = ran_lines(&lines);

    // what initializing each class of the project runs, by the class's file
    let init_dir = dir.join("inits-data");
    std::fs::create_dir_all(&init_dir)?;
    let rest: Vec<PathBuf> = classpath.iter().filter(|p| p.extension().is_some_and(|e| e == "jar")).cloned().collect();
    let mut cmd = Command::new("java");
    cmd.arg(&agent)
        .arg(format!("-Dgitnext.dir={}", init_dir.display()))
        .arg("-cp")
        .arg(std::env::join_paths(std::iter::once(inits).chain(classpath.iter().cloned()))?)
        .arg("gitnextlearn.Inits")
        .arg(root_abs.join("target").join("classes"))
        .arg(&init_dir)
        .arg(std::env::join_paths(&rest)?)
        .current_dir(root);
    let mut notes = Vec::new();
    let mut init_of: HashMap<&str, BTreeSet<(&str, u32)>> = HashMap::new(); // a class's file → (file, line) its initialization runs
    let init_run = timed(&mut cmd, &dir.join("inits.log"), STEP_LIMIT)?;
    let init_names = std::fs::read_to_string(init_dir.join("inits.tsv")).unwrap_or_default();
    let init_lines = if init_names.is_empty() { String::new() } else { analyze(root, &analyzer, &jars, &init_dir)? };
    if init_names.is_empty() {
        notes.push(format!("Java: what the classes' static blocks run was not measured ({})", why_failed(&init_run.text).lines().next().unwrap_or("no output")));
    }
    let init_dumps = ran_lines(&init_lines);
    for row in init_names.lines() {
        let Some((n, class)) = row.split_once('\t') else { continue };
        let outer = class.split('$').next().unwrap_or(class).replace('.', "/") + ".java";
        let (Some(file), Some(ran)) = (by_jvm_name.get(&outer), init_dumps.get(format!("init-{n}").as_str())) else { continue };
        for (f, ls) in ran {
            if let Some(f) = by_jvm_name.get(*f) {
                init_of.entry(file).or_default().extend(ls.iter().map(|l| (*f, *l)));
            }
        }
    }

    // a test's lines: its own dumps, and what its class ran around the tests (`@BeforeAll`, static blocks)
    let mut of_test: BTreeMap<(&str, &str), BTreeSet<(&str, u32)>> = BTreeMap::new();
    let mut of_class: HashMap<&str, BTreeSet<(&str, u32)>> = HashMap::new();
    let mut failing = BTreeSet::new();
    for row in index.lines() {
        let col: Vec<&str> = row.split('\t').collect();
        let [n, class, method, failed] = col[..] else { continue };
        let ran = by_dump.get(n).into_iter().flatten().filter_map(|(f, ls)| by_jvm_name.get(*f).map(|f| (*f, ls))).flat_map(|(f, ls)| ls.iter().map(move |l| (f, *l)));
        if method.is_empty() {
            of_class.entry(class).or_default().extend(ran);
        } else {
            of_test.entry((class, method)).or_default().extend(ran);
            if failed == "1" {
                failing.insert(format!("{}.{method}", class.rsplit('.').next().unwrap_or(class)));
            }
        }
    }
    let mut pairs = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for ((class, method), mut ran) in of_test.clone() {
        let outer = class.split('$').next().unwrap_or(class);
        let test_file = by_jvm_name.get(&(outer.replace('.', "/") + ".java")).copied().filter(|f| tests.contains(&(*f, method)));
        let Some(test_file) = test_file else {
            // inherited from a class in another file, or made at run time
            unknown.insert(format!("{}.{method}", class.rsplit('.').next().unwrap_or(class)));
            continue;
        };
        ran.extend(of_class.get(class).into_iter().flatten().copied());
        let touched: BTreeSet<&str> = ran.iter().map(|(f, _)| *f).collect();
        for f in touched {
            ran.extend(init_of.get(f).into_iter().flatten().copied());
        }
        let name = format!("{}::{method}", class.rsplit('.').next().unwrap_or(class).replace('$', "::"));
        for (f, l) in ran {
            if let Some(Some(i)) = owner_of.get(f).and_then(|o| o.get(l as usize)) {
                pairs.insert((*i, test_file.to_string(), name.clone()));
            }
        }
    }
    if test_run.timed_out {
        notes.push(format!("Java: the test run took longer than {} hours and was stopped; the map holds the tests that ran until then", TEST_LIMIT.as_secs() / 3600));
    }
    if !failing.is_empty() {
        notes.push(format!("Java: {} tests fail (what they ran is kept): {}", failing.len(), some(&failing.into_iter().collect::<Vec<_>>())));
    }
    if !unknown.is_empty() {
        notes.push(format!(
            "Java: {} tests have no method of that name in their class's file (inherited or made at run time) and are not in the map: {}",
            unknown.len(),
            some(&unknown.iter().cloned().collect::<Vec<_>>())
        ));
    }
    Ok(Run {
        source: "jacoco",
        command: format!("{mvn} test with the JaCoCo {JACOCO} agent (includes={includes}), its data taken around each of {} tests", of_test.len()),
        seconds: started.elapsed().as_secs_f32(),
        tests_failed: !test_run.ok,
        pairs,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_of_the_pom_carries_the_agent_and_the_listener() {
        const EL: &str = "<additionalClasspathElements><additionalClasspathElement>${gitnext.cp}</additionalClasspathElement>";
        // no surefire in the pom: the plugin is added
        let p = learning_pom("<project><build><plugins><plugin><artifactId>maven-jar-plugin</artifactId></plugin></plugins></build></project>").unwrap();
        assert!(p.contains("<plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-surefire-plugin</artifactId><configuration><argLine>${gitnext.argLine}</argLine>"), "{p}");
        assert!(p.contains(EL) && p.contains("maven-jar-plugin"), "{p}");
        // no build at all
        let p = learning_pom("<project><artifactId>a</artifactId></project>").unwrap();
        assert!(p.ends_with("</plugin></plugins></build></project>") && p.contains(EL), "{p}");
        // surefire with its own options: they stay, ours are added
        let own = "<project><build><plugins><plugin><artifactId>maven-surefire-plugin</artifactId><configuration><argLine>-Xmx1g</argLine><forkCount>1</forkCount></configuration></plugin></plugins></build>\
                   <profiles><profile><build><plugins><plugin><artifactId>maven-surefire-plugin</artifactId><configuration><additionalClasspathElements><additionalClasspathElement>x</additionalClasspathElement></additionalClasspathElements></configuration></plugin></plugins></build></profile></profiles></project>";
        let p = learning_pom(own).unwrap();
        assert!(p.contains("<argLine>-Xmx1g ${gitnext.argLine}</argLine><forkCount>1</forkCount>"), "{p}");
        assert_eq!(p.matches(EL).count(), 2, "{p}"); // the main list and the profile's
        assert!(learning_pom("not a pom").is_none());
    }

    #[test]
    fn the_agent_instruments_the_package_all_code_shares() {
        assert_eq!(includes(["org.jsoup.nodes", "org.jsoup", "org.jsoup.parser"].into_iter()), "org.jsoup.*");
        assert_eq!(includes(["org.json"].into_iter()), "org.json.*");
        assert_eq!(includes(["a.b", "c.d"].into_iter()), "*");
        assert_eq!(includes(["", "a"].into_iter()), "*");
        assert_eq!(package_of("// c\npackage org.json ;\nimport x;"), "org.json");
        assert_eq!(package_of("class A {}"), "");
    }

    #[test]
    fn the_lines_that_ran_are_read_by_dump_and_file() {
        let got = ran_lines("12-0\torg/x/A.java\t3,4,9\n12-0\torg/x/B.java\t1\n12-1\torg/x/A.java\t7\nbroken\n");
        assert_eq!(got["12-0"], vec![("org/x/A.java", vec![3, 4, 9]), ("org/x/B.java", vec![1])]);
        assert_eq!(got["12-1"], vec![("org/x/A.java", vec![7])]);
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn the_tools_come_from_the_repository_the_classpath_lies_in() {
        let repo = std::env::temp_dir().join(format!("gitnext-learn-java-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        let put = |group: &str, artifact: &str, version: &str, classifier: &str| {
            let j = jar(&repo, group, artifact, version, classifier);
            std::fs::create_dir_all(j.parent().unwrap()).unwrap();
            std::fs::write(&j, "").unwrap();
            j
        };
        std::fs::create_dir_all(repo.join("org/apache/maven/plugins")).unwrap();
        let junit = put("junit", "junit", "4.13.2", "");
        assert_eq!(repository(std::slice::from_ref(&junit)), repo);
        assert!(junit.ends_with("junit/junit/4.13.2/junit-4.13.2.jar"));
        // nothing there yet: one line with the commands, the platform's version a known one
        let e = jars(&repo, None).err().unwrap().to_string();
        assert!(e.ends_with(&format!("get it once: mvn dependency:get -Dartifact=org.jacoco:org.jacoco.core:{JACOCO} && mvn dependency:get -Dartifact=org.jacoco:org.jacoco.agent:{JACOCO}:jar:runtime && mvn dependency:get -Dartifact=org.junit.platform:junit-platform-launcher:{PLATFORM}")), "{e}");
        put("org.jacoco", "org.jacoco.agent", JACOCO, "runtime");
        put("org.jacoco", "org.jacoco.core", JACOCO, "");
        for a in ["asm", "asm-commons", "asm-tree"] {
            put("org.ow2.asm", a, ASM, "");
        }
        // the project's own platform is asked for when the repository has no launcher of it
        let e = jars(&repo, Some("1.11.0")).err().unwrap().to_string();
        assert!(e.ends_with("get it once: mvn dependency:get -Dartifact=org.junit.platform:junit-platform-launcher:1.11.0"), "{e}");
        for v in ["1.9.3", "1.14.4"] {
            for a in ["junit-platform-launcher", "junit-platform-engine", "junit-platform-commons"] {
                put("org.junit.platform", a, v, "");
            }
        }
        // any launcher serves to compile against: the newest by number, not by letter
        let got = jars(&repo, Some("1.11.0")).unwrap();
        assert!(got.platform[0].ends_with("junit-platform-launcher/1.14.4/junit-platform-launcher-1.14.4.jar"), "{:?}", got.platform);
        assert!(got.agent.ends_with(format!("org/jacoco/org.jacoco.agent/{JACOCO}/org.jacoco.agent-{JACOCO}-runtime.jar")) && got.analyze.len() == 4);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
