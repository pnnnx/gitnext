//! External libraries: which ones a symbol calls, and which the project declares.
//!
//! A call that leaves the repo is matched to the file's imports
//! (`fs.readFile` with `import * as fs from "node:fs"` → `node:fs`; `HashMap::new`
//! with `use std::collections::HashMap` → `std`). The library name is the
//! package as its ecosystem names it: an npm package, a Go module, a Python
//! top-level module, a crate, a Java package group. Standard libraries are
//! marked, so "uses requests" and "uses json" read differently.

use crate::graph::Graph;
use gitnext_parse::{family_of, EdgeKind};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LibUse {
    /// Package name (`react`, `github.com/gin-gonic/gin`, `requests`, `serde`, `org.springframework.web`).
    pub name: String,
    /// Part of the language's standard library (`node:fs`, `fmt`, `json`, `std`, `java.util`).
    pub stdlib: bool,
    /// Members used, as written after the package (`get`, `useState`, `Println`).
    pub members: Vec<String>,
}

/// A dependency declared in a manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Dep {
    pub name: String,
    pub version: String,
    /// `npm`, `go`, `pypi`, `crates`, `maven`.
    pub ecosystem: &'static str,
    pub manifest: String,
    pub dev: bool,
}

impl Graph {
    /// Libraries a symbol calls into, most-used first.
    pub fn libraries(&self, src: usize) -> Vec<LibUse> {
        let file = self.syms[src].file.clone();
        let imports = self.external_imports(&file);
        let mut by: BTreeMap<String, (bool, Vec<String>)> = BTreeMap::new();
        for x in self.callees(src).external {
            if let Some((lib, std, member)) = classify(&x, &imports, &file) {
                let e = by.entry(lib).or_insert((std, vec![]));
                if !e.1.contains(&member) {
                    e.1.push(member);
                }
            }
        }
        let mut out: Vec<LibUse> = by.into_iter().map(|(name, (stdlib, members))| LibUse { name, stdlib, members }).collect();
        out.sort_by_key(|l| (l.stdlib, std::cmp::Reverse(l.members.len())));
        out
    }

    /// Every library the code calls into, with the number of call sites.
    pub fn library_usage(&self) -> Vec<(LibUse, usize)> {
        let mut imports_by_file: HashMap<&str, Vec<String>> = HashMap::new();
        let mut by: BTreeMap<String, (bool, Vec<String>, usize)> = BTreeMap::new();
        let mut resolved_cache: HashMap<(&str, &str), bool> = HashMap::new();
        for (file, e) in &self.edges {
            if e.kind != EdgeKind::Calls || e.to.starts_with('.') {
                continue;
            }
            let imports = imports_by_file.entry(file.as_str()).or_insert_with(|| self.external_imports(file));
            if imports.is_empty() {
                continue;
            }
            let Some((lib, std, member)) = classify(&e.to, imports, file) else { continue };
            let external = *resolved_cache.entry((file.as_str(), e.to.as_str())).or_insert_with(|| self.resolve(file, e).is_empty());
            if !external {
                continue;
            }
            let x = by.entry(lib).or_insert((std, vec![], 0));
            x.2 += 1;
            if !x.1.contains(&member) && x.1.len() < 12 {
                x.1.push(member);
            }
        }
        let mut out: Vec<(LibUse, usize)> = by.into_iter().map(|(name, (stdlib, members, n))| (LibUse { name, stdlib, members }, n)).collect();
        out.sort_by_key(|(l, n)| (l.stdlib, std::cmp::Reverse(*n)));
        out
    }

    /// Imports of a file that do not point into the repo.
    fn external_imports(&self, file: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .edges
            .iter()
            .filter(|(f, e)| f == file && e.kind == EdgeKind::Imports)
            .flat_map(|(_, e)| crate::graph::flatten_use(&e.to))
            .filter(|p| {
                let first = p.split("::").next().unwrap_or("");
                !matches!(first, "crate" | "self" | "super") && !self.is_repo_root(first)
            })
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

/// (library, stdlib, member) for an external call target, through the file's imports.
fn classify(to: &str, imports: &[String], file: &str) -> Option<(String, bool, String)> {
    if to.starts_with('.') || to.starts_with("Self::") {
        return None;
    }
    let fam = family_of(file).unwrap_or("");
    // longest import the call starts with: `node:fs::readFile`, `requests::adapters::get`
    let hit = imports.iter().filter(|imp| to == imp.as_str() || to.starts_with(&format!("{imp}::"))).max_by_key(|imp| imp.len());
    let (imp, member) = match hit {
        Some(imp) => (imp.clone(), to[imp.len()..].trim_start_matches("::").to_string()),
        None => {
            // Rust/Java: `HashMap::new` where `std::collections::HashMap` is imported
            let first = to.split("::").next().unwrap_or(to);
            let imp = imports.iter().find(|imp| imp.rsplit("::").next() == Some(first))?;
            (imp.clone(), to.to_string())
        }
    };
    let (lib, std) = library_name(&imp, fam);
    let member = if member.is_empty() { imp.rsplit("::").next().unwrap_or(&imp).to_string() } else { member.replace("::", ".") };
    Some((lib, std, member))
}

/// Package name of an import path, and whether it is the standard library.
pub fn library_name(imp: &str, family: &str) -> (String, bool) {
    match family {
        "js" => {
            if let Some(m) = imp.strip_prefix("node:") {
                return (format!("node:{}", m.split('/').next().unwrap_or(m)), true);
            }
            let mut parts = imp.split("::").next().unwrap_or(imp).split('/');
            let first = parts.next().unwrap_or("");
            let name = if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or("")) } else { first.to_string() };
            let std = NODE_BUILTINS.contains(&name.as_str());
            (if std { format!("node:{name}") } else { name }, std)
        }
        "go" => {
            let path = imp.split("::").next().unwrap_or(imp);
            let segs: Vec<&str> = path.split('/').collect();
            if segs.first().is_some_and(|s| s.contains('.')) {
                // github.com/owner/repo — the module, not its sub-packages
                let n = if segs[0] == "gopkg.in" { 2 } else { 3 };
                (segs.iter().take(n).copied().collect::<Vec<_>>().join("/"), false)
            } else {
                (path.to_string(), true)
            }
        }
        "java" => {
            let segs: Vec<&str> = imp.split("::").collect();
            if matches!(segs.first().copied(), Some("java" | "javax" | "jdk" | "sun")) {
                (segs.iter().take(2).copied().collect::<Vec<_>>().join("."), true)
            } else {
                // the package (lowercase segments before the class), cut to its organisation:
                // org.junit, org.mockito, com.fasterxml.jackson, org.springframework.web
                let pkg: Vec<&str> = segs.iter().copied().take_while(|s| s.chars().next().is_some_and(|c| c.is_lowercase())).collect();
                let broad = pkg.get(1).is_some_and(|s| matches!(*s, "google" | "apache" | "springframework" | "fasterxml" | "jetbrains" | "eclipse" | "amazonaws" | "squareup"));
                let n = if broad { 3 } else { 2 };
                (pkg.iter().take(n.min(pkg.len()).max(1)).copied().collect::<Vec<_>>().join("."), false)
            }
        }
        "python" => {
            let first = imp.split("::").next().unwrap_or(imp);
            (first.to_string(), PY_STDLIB.contains(&first))
        }
        "rust" => {
            let first = imp.split("::").next().unwrap_or(imp);
            (first.to_string(), matches!(first, "std" | "core" | "alloc"))
        }
        _ => (imp.split("::").next().unwrap_or(imp).to_string(), false),
    }
}

const NODE_BUILTINS: &[&str] = &[
    "fs", "path", "os", "http", "https", "http2", "crypto", "url", "util", "events", "stream", "child_process", "buffer", "assert",
    "zlib", "readline", "net", "tls", "dns", "worker_threads", "process", "querystring", "timers", "cluster", "vm", "module", "perf_hooks",
];

const PY_STDLIB: &[&str] = &[
    "abc", "argparse", "ast", "asyncio", "base64", "bisect", "builtins", "calendar", "codecs", "collections", "concurrent", "configparser",
    "contextlib", "copy", "csv", "ctypes", "dataclasses", "datetime", "decimal", "difflib", "email", "enum", "errno", "fnmatch",
    "fractions", "functools", "gc", "getpass", "glob", "gzip", "hashlib", "heapq", "hmac", "html", "http", "importlib", "inspect", "io",
    "ipaddress", "itertools", "json", "locale", "logging", "math", "mimetypes", "multiprocessing", "operator", "os", "pathlib",
    "pickle", "platform", "pprint", "queue", "random", "re", "sched", "secrets", "select", "shlex", "shutil", "signal", "socket",
    "sqlite3", "ssl", "statistics", "string", "struct", "subprocess", "sys", "tarfile", "tempfile", "textwrap", "threading", "time",
    "timeit", "tomllib", "traceback", "types", "typing", "unicodedata", "unittest", "urllib", "uuid", "warnings", "weakref", "xml",
    "zipfile", "zlib", "zoneinfo",
];

/// Python import name → distribution name where they differ.
const PY_DIST: &[(&str, &str)] = &[
    ("yaml", "pyyaml"), ("PIL", "pillow"), ("sklearn", "scikit-learn"), ("cv2", "opencv-python"), ("bs4", "beautifulsoup4"),
    ("dateutil", "python-dateutil"), ("dotenv", "python-dotenv"), ("jwt", "pyjwt"), ("google", "google-api-python-client"),
    ("attr", "attrs"), ("serial", "pyserial"), ("magic", "python-magic"), ("Crypto", "pycryptodome"), ("docx", "python-docx"),
];

fn norm(s: &str) -> String {
    s.to_lowercase().replace(['-', '.'], "_")
}

/// The declared dependency a library name stands for (version, dev or not).
pub fn match_dep<'a>(lib: &str, deps: &'a [Dep]) -> Option<&'a Dep> {
    let n = norm(lib);
    let dist = PY_DIST.iter().find(|(i, _)| *i == lib).map(|(_, d)| norm(d));
    deps.iter().find(|d| {
        let dn = norm(&d.name);
        dn == n || dist.as_deref() == Some(dn.as_str())
            // maven `org.springframework:spring-web` ↔ package `org.springframework.web`; go module prefix
            || (d.ecosystem == "maven" && lib.starts_with(d.name.split(':').next().unwrap_or("~")))
            || (d.ecosystem == "go" && (lib == d.name || lib.starts_with(&format!("{}/", d.name)) || d.name.starts_with(&format!("{lib}/"))))
    })
}

/// Dependencies declared in the manifests among `files` (path, content).
pub fn manifest_deps(files: &[(String, String)]) -> Vec<Dep> {
    let mut out = Vec::new();
    for (path, text) in files {
        let file = path.rsplit('/').next().unwrap_or(path);
        match file {
            "package.json" => npm(path, text, &mut out),
            "go.mod" => gomod(path, text, &mut out),
            "Cargo.toml" => cargo(path, text, &mut out),
            "pyproject.toml" => pyproject(path, text, &mut out),
            "pom.xml" => pom(path, text, &mut out),
            "build.gradle" | "build.gradle.kts" => gradle(path, text, &mut out),
            f if f.starts_with("requirements") && f.ends_with(".txt") => {
                for l in text.lines() {
                    if let Some(d) = py_req(l, path, f.contains("dev") || f.contains("test")) {
                        out.push(d);
                    }
                }
            }
            _ => {}
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert((d.ecosystem, d.name.clone())));
    out
}

/// Manifest files worth reading for [`manifest_deps`].
pub fn is_manifest(path: &str) -> bool {
    let f = path.rsplit('/').next().unwrap_or(path);
    matches!(f, "package.json" | "go.mod" | "Cargo.toml" | "pyproject.toml" | "pom.xml" | "build.gradle" | "build.gradle.kts")
        || (f.starts_with("requirements") && f.ends_with(".txt"))
}

fn npm(path: &str, text: &str, out: &mut Vec<Dep>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
    for (key, dev) in [("dependencies", false), ("peerDependencies", false), ("devDependencies", true)] {
        if let Some(m) = v.get(key).and_then(|x| x.as_object()) {
            for (name, ver) in m {
                out.push(Dep { name: name.clone(), version: ver.as_str().unwrap_or("").into(), ecosystem: "npm", manifest: path.into(), dev });
            }
        }
    }
}

fn gomod(path: &str, text: &str, out: &mut Vec<Dep>) {
    let mut in_block = false;
    for l in text.lines() {
        let t = l.split("//").next().unwrap_or("").trim();
        if t.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && t == ")" {
            in_block = false;
            continue;
        }
        let spec = if in_block { Some(t) } else { t.strip_prefix("require ") };
        if let Some(s) = spec {
            let mut it = s.split_whitespace();
            if let (Some(n), Some(v)) = (it.next(), it.next()) {
                let indirect = l.contains("// indirect");
                if !indirect {
                    out.push(Dep { name: n.into(), version: v.into(), ecosystem: "go", manifest: path.into(), dev: false });
                }
            }
        }
    }
}

fn cargo(path: &str, text: &str, out: &mut Vec<Dep>) {
    let mut section = String::new();
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            section = t.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let dev = section.ends_with("dev-dependencies");
        if !(section.ends_with("dependencies")) || t.is_empty() || t.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = t.split_once('=') else { continue };
        let name = name.trim().trim_matches('"');
        if rest.contains("path =") || rest.contains("path=") || rest.contains("workspace = true") || rest.contains("workspace=true") {
            continue;
        }
        let version = rest.trim().trim_matches('"').to_string();
        let version = if version.starts_with('{') { version.split("version").nth(1).and_then(|v| v.split('"').nth(1)).unwrap_or("").to_string() } else { version };
        out.push(Dep { name: name.into(), version, ecosystem: "crates", manifest: path.into(), dev });
    }
}

fn py_req(line: &str, path: &str, dev: bool) -> Option<Dep> {
    let t = line.split('#').next()?.trim().trim_matches(|c| c == '"' || c == '\'' || c == ',');
    if t.is_empty() || t.starts_with('-') {
        return None;
    }
    let cut = t.find(|c: char| "<>=!~;[ ".contains(c)).unwrap_or(t.len());
    let name = &t[..cut];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)) {
        return None;
    }
    let version = t[cut..].split(';').next().unwrap_or("").trim().to_string();
    Some(Dep { name: name.into(), version, ecosystem: "pypi", manifest: path.into(), dev })
}

fn pyproject(path: &str, text: &str, out: &mut Vec<Dep>) {
    let mut section = String::new();
    let mut in_list = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') && !in_list {
            section = t.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        if section == "project" && t.starts_with("dependencies") {
            in_list = !t.contains(']');
            if let Some(inline) = t.split_once('[').map(|(_, r)| r) {
                for item in inline.trim_end_matches(']').split(',') {
                    out.extend(py_req(item, path, false));
                }
            }
            continue;
        }
        if in_list {
            if t.starts_with(']') {
                in_list = false;
                continue;
            }
            out.extend(py_req(t, path, false));
            continue;
        }
        if section.starts_with("tool.poetry") && section.ends_with("dependencies") {
            if let Some((n, v)) = t.split_once('=') {
                let n = n.trim();
                if n != "python" && !n.is_empty() {
                    out.push(Dep { name: n.into(), version: v.trim().trim_matches('"').into(), ecosystem: "pypi", manifest: path.into(), dev: section.contains("dev") || section.contains("group") });
                }
            }
        }
        if section == "project.optional-dependencies" {
            if let Some((_, r)) = t.split_once('[') {
                for item in r.trim_end_matches(']').split(',') {
                    out.extend(py_req(item, path, true));
                }
            }
        }
    }
}

fn pom(path: &str, text: &str, out: &mut Vec<Dep>) {
    for block in text.split("<dependency>").skip(1) {
        let block = block.split("</dependency>").next().unwrap_or("");
        let tag = |t: &str| block.split(&format!("<{t}>")).nth(1).and_then(|r| r.split(&format!("</{t}>")).next()).map(str::trim).unwrap_or("");
        let (g, a) = (tag("groupId"), tag("artifactId"));
        if !g.is_empty() && !a.is_empty() {
            out.push(Dep { name: format!("{g}:{a}"), version: tag("version").into(), ecosystem: "maven", manifest: path.into(), dev: tag("scope") == "test" });
        }
    }
}

fn gradle(path: &str, text: &str, out: &mut Vec<Dep>) {
    for l in text.lines() {
        let t = l.trim();
        let Some((conf, rest)) = t.split_once(['(', ' ']) else { continue };
        if !matches!(conf, "implementation" | "api" | "compileOnly" | "runtimeOnly" | "testImplementation" | "annotationProcessor" | "kapt") {
            continue;
        }
        let s = rest.trim().trim_start_matches('(').trim_end_matches(')').trim().trim_matches(|c| c == '"' || c == '\'');
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() >= 2 {
            out.push(Dep { name: format!("{}:{}", parts[0], parts[1]), version: parts.get(2).unwrap_or(&"").to_string(), ecosystem: "maven", manifest: path.into(), dev: conf.starts_with("test") });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_names() {
        assert_eq!(library_name("node:fs", "js"), ("node:fs".into(), true));
        assert_eq!(library_name("@tanstack/react-query", "js"), ("@tanstack/react-query".into(), false));
        assert_eq!(library_name("react-dom/client", "js"), ("react-dom".into(), false));
        assert_eq!(library_name("fs", "js"), ("node:fs".into(), true));
        assert_eq!(library_name("github.com/gin-gonic/gin/binding", "go"), ("github.com/gin-gonic/gin".into(), false));
        assert_eq!(library_name("net/http", "go"), ("net/http".into(), true));
        assert_eq!(library_name("java::util::List", "java"), ("java.util".into(), true));
        assert_eq!(library_name("org::springframework::web::bind::GetMapping", "java"), ("org.springframework.web".into(), false));
        assert_eq!(library_name("org::junit::Assert::assertTrue", "java"), ("org.junit".into(), false));
        assert_eq!(library_name("requests::adapters", "python"), ("requests".into(), false));
        assert_eq!(library_name("json", "python"), ("json".into(), true));
        assert_eq!(library_name("std::collections::HashMap", "rust"), ("std".into(), true));
    }

    #[test]
    fn calls_through_imports() {
        let imps = vec!["node:fs".to_string(), "react".to_string()];
        assert_eq!(classify("node:fs::readFile", &imps, "a.ts"), Some(("node:fs".into(), true, "readFile".into())));
        assert_eq!(classify("react::useState", &imps, "a.ts"), Some(("react".into(), false, "useState".into())));
        assert_eq!(classify(".then", &imps, "a.ts"), None);
        let rs = vec!["std::collections::HashMap".to_string()];
        assert_eq!(classify("HashMap::new", &rs, "a.rs"), Some(("std".into(), true, "HashMap.new".into())));
    }

    #[test]
    fn manifests() {
        let files = vec![
            ("package.json".to_string(), r#"{"dependencies":{"react":"^18.2.0"},"devDependencies":{"vitest":"1.0"}}"#.to_string()),
            ("go.mod".to_string(), "module x\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgolang.org/x/sys v0.1.0 // indirect\n)\n".to_string()),
            ("requirements.txt".to_string(), "requests>=2.31 # http\nPyYAML==6.0\n-e .\n".to_string()),
            ("Cargo.toml".to_string(), "[package]\nname=\"a\"\n[dependencies]\nserde = { version = \"1.0\", features = [\"derive\"] }\nlocal = { path = \"../x\" }\n".to_string()),
            ("pom.xml".to_string(), "<dependency><groupId>org.springframework</groupId><artifactId>spring-web</artifactId><version>6.1</version></dependency>".to_string()),
        ];
        let d = manifest_deps(&files);
        let names: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["react", "vitest", "github.com/gin-gonic/gin", "requests", "PyYAML", "serde", "org.springframework:spring-web"]);
        assert_eq!(d.iter().find(|x| x.name == "serde").unwrap().version, "1.0");
        assert!(match_dep("yaml", &d).is_some_and(|x| x.name == "PyYAML"));
        assert!(match_dep("github.com/gin-gonic/gin", &d).is_some());
        assert!(match_dep("org.springframework.web", &d).is_some());
    }
}
