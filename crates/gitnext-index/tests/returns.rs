//! A receiver whose type is only known as "what that function returns" (`x := getPool()`,
//! `getPool().release()`): the declared result type picks the right `release` among several.

use gitnext_index::{Graph, Index};

fn graph(name: &str, files: &[(&str, &str)]) -> Graph {
    let dir = std::env::temp_dir().join(format!("gitnext-returns-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, code) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, code).unwrap();
    }
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    let g = idx.graph().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    g
}

/// Names of the functions that surely call `target` (guesses fail the test).
fn callers(g: &Graph, target: &str) -> Vec<String> {
    let found = g.find(target);
    assert_eq!(found.len(), 1, "{target}: {found:?}");
    let mut v: Vec<String> = g
        .callers(found[0])
        .into_iter()
        .map(|(c, _)| {
            assert!(!g.is_guess(c, found[0]), "{} → {target} is only a guess", g.syms[c].sym.name);
            g.syms[c].sym.name.clone()
        })
        .collect();
    v.sort();
    v
}

#[test]
fn go_result_types() {
    let g = graph(
        "go",
        &[(
            "pool/pool.go",
            r#"package pool

type Pool struct{}

func (p *Pool) Release() {}
func (p *Pool) Child() *Conn { return &Conn{} }

type Conn struct{}

func (c *Conn) Release() {}

func Get() *Pool             { return &Pool{} }
func Open() (*Conn, error)   { return &Conn{}, nil }
func All() []*Pool           { return nil }

func useVar()   { p := Get(); p.Release() }
func useTuple() { c, err := Open(); _ = err; c.Release() }
func useChain() { Get().Child().Release() }
func useRange() {
	for _, p := range All() {
		p.Release()
	}
}
func useCopy() { p := Get(); q := p; q.Release() }
"#,
        )],
    );
    assert_eq!(callers(&g, "Pool::Release"), ["useCopy", "useRange", "useVar"]);
    assert_eq!(callers(&g, "Conn::Release"), ["useChain", "useTuple"]);
}

#[test]
fn java_result_types() {
    let g = graph(
        "java",
        &[(
            "src/app/Repo.java",
            r#"package app;

class Pool {
    void release() {}
    Conn child() { return new Conn(); }
}

class Conn {
    void release() {}
}

class Repo {
    Pool pool() { return new Pool(); }
    static Conn open() { return new Conn(); }
    void viaCall() { pool().release(); }
    void viaVar() { var c = Repo.open(); c.release(); }
    void viaChain() { pool().child().release(); }
    void viaNew() { new Pool().release(); }
    void viaCast(Object o) { ((Conn) o).release(); }
}
"#,
        )],
    );
    assert_eq!(callers(&g, "Pool::release"), ["viaCall", "viaNew"]);
    assert_eq!(callers(&g, "Conn::release"), ["viaCast", "viaChain", "viaVar"]);
}

#[test]
fn ts_result_types() {
    let g = graph(
        "ts",
        &[
            (
                "src/pool.ts",
                r#"export class Pool { release(): void {} }
export class Conn { release(): void {} }
export function getPool(): Pool { return new Pool() }
export async function open(): Promise<Conn> { return new Conn() }
"#,
            ),
            (
                "src/use.ts",
                r#"import { getPool, open } from "./pool"

export function viaVar() { const p = getPool(); p.release() }
export async function viaAwait() { const c = await open(); c.release() }
export function viaCall() { getPool().release() }
"#,
            ),
        ],
    );
    assert_eq!(callers(&g, "Pool::release"), ["viaCall", "viaVar"]);
    assert_eq!(callers(&g, "Conn::release"), ["viaAwait"]);
}

#[test]
fn unknown_result_stays_a_guess() {
    // `make()` declares no result type: both `release` methods remain candidates, as before
    let g = graph(
        "unknown",
        &[(
            "src/x.ts",
            r#"export class Pool { release(): void {} }
export class Conn { release(): void {} }
export function make() { return new Pool() }
export function use() { make().release() }
"#,
        )],
    );
    let pool = g.find("Pool::release")[0];
    let user = g.find("use")[0];
    assert!(g.callers(pool).iter().any(|(c, _)| *c == user));
    assert!(g.is_guess(user, pool));
}
