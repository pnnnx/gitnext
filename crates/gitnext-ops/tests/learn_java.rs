//! `gitnext learn-tests` in a Java project on Maven: the map names the tests that really ran each
//! method. What cannot be learned is said without starting a JVM; the run itself needs Maven with
//! JUnit and JaCoCo in its local repository and is skipped unless `GITNEXT_TEST_MVN=1` (the pom's
//! copy and the reading of the coverage lines are tested without Maven in `learn/java.rs`).

use gitnext_ops::gitnext_index::{Graph, Index, TestMap};
use gitnext_ops::{learn, locate};
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-learn-java-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn graph(dir: &Path) -> Graph {
    let mut idx = Index::open(dir).unwrap();
    idx.refresh().unwrap();
    idx.graph().unwrap()
}

const POM: &str = "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>demo</groupId>\n  <artifactId>demo</artifactId>\n  <version>1</version>\n  <properties>\n    <maven.compiler.release>11</maven.compiler.release>\n    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n  </properties>\n  <dependencies>\n    <dependency>\n      <groupId>org.junit.jupiter</groupId>\n      <artifactId>junit-jupiter</artifactId>\n      <version>5.14.4</version>\n      <scope>test</scope>\n    </dependency>\n  </dependencies>\n</project>\n";
const CALC: &str = "package demo;\n\npublic class Calc {\n    static final int BASE = base();\n\n    static int base() {\n        return 10;\n    }\n\n    public static int add(int a, int b) {\n        return a + b;\n    }\n\n    public static int twice(int x) {\n        return add(x, x);\n    }\n\n    public static int unused() {\n        return 0;\n    }\n}\n";
const CALC_TEST: &str = "package demo;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nimport org.junit.jupiter.api.Test;\n\nclass CalcTest {\n    @Test\n    void doubles() {\n        assertEquals(4, Calc.twice(2));\n    }\n}\n";
const ADD_TEST: &str = "package demo;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nimport org.junit.jupiter.api.Test;\n\nclass AddTest {\n    @Test\n    void adds() {\n        assertEquals(3, Calc.add(1, 2));\n    }\n}\n";

fn learn_error(dir: &Path) -> String {
    let g = graph(dir);
    let e = learn::learn(dir, &g).err().map(|e| format!("{e:#}")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(dir);
    e
}

#[test]
fn what_is_not_learned_is_said_in_one_line() {
    let e = learn_error(&project("gradle", &[("build.gradle", "plugins { id 'java' }\n"), ("src/main/java/demo/Calc.java", CALC)]));
    assert_eq!(e, "Java: the project builds with Gradle; learn-tests knows Maven only");
    let e = learn_error(&project("no-pom", &[("src/main/java/demo/Calc.java", CALC)]));
    assert_eq!(e, "Java: no pom.xml at the root of the repository");
    let modules = POM.replace("<dependencies>", "<modules><module>core</module></modules>\n  <dependencies>");
    let e = learn_error(&project("modules", &[("pom.xml", &modules), ("core/src/main/java/demo/Calc.java", CALC)]));
    assert_eq!(e, "Java: a Maven project of several modules is not learned yet");
}

#[test]
fn a_maven_project_learns_which_test_ran_which_method() {
    if std::env::var("GITNEXT_TEST_MVN").as_deref() != Ok("1") {
        eprintln!("skipped: set GITNEXT_TEST_MVN=1 to run Maven with the JaCoCo agent");
        return;
    }
    let dir = project(
        "maven",
        &[("pom.xml", POM), ("src/main/java/demo/Calc.java", CALC), ("src/test/java/demo/CalcTest.java", CALC_TEST), ("src/test/java/demo/AddTest.java", ADD_TEST)],
    );
    let g = graph(&dir);
    let l = learn::learn(&dir, &g).unwrap();
    assert!(!l.tests_failed && l.tests == 2 && l.notes.is_empty(), "{}", learn::render(&l));
    assert!(l.command.contains("includes=demo.*") && l.command.contains("each of 2 tests"), "{}", l.command);
    let tests_of = |name: &str| {
        let q = g.unique_name(g.find(name)[0]);
        let mut t: Vec<String> = TestMap::open_existing(&dir).unwrap().tests_of(&q).into_iter().map(|c| format!("{} {}", c.file, c.name)).collect();
        t.sort();
        t
    };
    let (add, calc) = ("src/test/java/demo/AddTest.java AddTest::adds", "src/test/java/demo/CalcTest.java CalcTest::doubles");
    assert_eq!(tests_of("twice"), [calc]);
    assert_eq!(tests_of("add"), [add, calc]);
    // a static initializer runs once in the JVM, in the first test that touches the class: it is both tests'
    assert_eq!(tests_of("base"), [add, calc]);
    assert!(tests_of("unused").is_empty());
    let (t, source) = locate::tests_for_at(&g, &dir, g.find("twice")[0], 4);
    assert_eq!(source, locate::Source::Learned { stale: false });
    assert_eq!(t.iter().map(|x| g.syms[x.symbol].sym.name.as_str()).collect::<Vec<_>>(), ["doubles"]);
    // the helper classes, the coverage data and the copy of the pom are gone
    let left: Vec<_> = std::fs::read_dir(dir.join(".gitnext")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("learn-java")).collect();
    assert!(left.is_empty() && !dir.join(".gitnext-pom.xml").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
