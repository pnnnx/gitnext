//! Which tests really run which functions — learned from one test run with coverage
//! (`gitnext learn-tests`) and kept in `.gitnext/tests.db`.
//!
//! The call graph cannot see calls made through a CLI runner, a decorator, a template or
//! reflection; on click it finds 19% of the tests that fail when a function breaks. A
//! coverage run sees all of them. The map is separate from the index: the index is a cache
//! that is rebuilt freely, the map costs a test run.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

/// A test that ran a function during the learning run.
#[derive(Debug, Clone)]
pub struct Covering {
    /// repo-relative file of the test
    pub file: String,
    /// `test_name` or `TestClass::test_name` (parameters dropped)
    pub name: String,
    /// how many functions this test ran: a narrow test is the likeliest to notice a break
    pub breadth: u32,
    /// the function's code hash when the map was learned (differs now → the map is stale for it)
    pub body_hash: String,
}

pub struct TestMap {
    conn: Connection,
}

impl TestMap {
    fn path(root: &Path) -> std::path::PathBuf {
        root.join(".gitnext").join("tests.db")
    }

    /// The map of this repository, if one was learned.
    pub fn open_existing(root: &Path) -> Option<TestMap> {
        let p = Self::path(root);
        p.exists().then(|| Connection::open(p).ok()).flatten().map(|conn| TestMap { conn })
    }

    /// Replace the map: `pairs` = (function's qualified name, its code hash, test file, test name).
    pub fn write(root: &Path, source: &str, pairs: &[(String, String, String, String)]) -> Result<TestMap> {
        std::fs::create_dir_all(root.join(".gitnext"))?;
        let mut conn = Connection::open(Self::path(root))?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS covers; DROP TABLE IF EXISTS tests; DROP TABLE IF EXISTS meta;
             CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE tests(id INTEGER PRIMARY KEY, file TEXT NOT NULL, name TEXT NOT NULL, breadth INTEGER NOT NULL);
             CREATE TABLE covers(symbol TEXT NOT NULL, body_hash TEXT NOT NULL, test INTEGER NOT NULL, PRIMARY KEY(symbol, test)) WITHOUT ROWID;",
        )?;
        // breadth: how many different functions a test ran (a pair given twice counts once)
        let mut ids: HashMap<(&str, &str), (i64, u32)> = HashMap::new();
        let mut seen: std::collections::HashSet<(&str, &str, &str)> = std::collections::HashSet::new();
        for (symbol, _, file, name) in pairs {
            let n = ids.len() as i64;
            let e = ids.entry((file, name)).or_insert((n, 0));
            if seen.insert((symbol, file, name)) {
                e.1 += 1;
            }
        }
        {
            let mut t = tx.prepare("INSERT INTO tests(id, file, name, breadth) VALUES (?1, ?2, ?3, ?4)")?;
            for ((file, name), (id, breadth)) in &ids {
                t.execute(params![id, file, name, breadth])?;
            }
            let mut c = tx.prepare("INSERT OR IGNORE INTO covers(symbol, body_hash, test) VALUES (?1, ?2, ?3)")?;
            for (symbol, hash, file, name) in pairs {
                c.execute(params![symbol, hash, ids[&(file.as_str(), name.as_str())].0])?;
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            tx.execute("INSERT INTO meta(key, value) VALUES ('learned_at', ?1), ('source', ?2)", params![now.to_string(), source])?;
        }
        tx.commit()?;
        Ok(TestMap { conn })
    }

    /// The tests that ran this function, narrowest first.
    pub fn tests_of(&self, qualified: &str) -> Vec<Covering> {
        let run = || -> rusqlite::Result<Vec<Covering>> {
            let mut st = self.conn.prepare("SELECT t.file, t.name, t.breadth, c.body_hash FROM covers c JOIN tests t ON t.id = c.test WHERE c.symbol = ?1 ORDER BY t.breadth, t.file, t.name")?;
            let rows = st.query_map([qualified], |r| Ok(Covering { file: r.get(0)?, name: r.get(1)?, breadth: r.get(2)?, body_hash: r.get(3)? }))?;
            rows.collect()
        };
        run().unwrap_or_default()
    }

    /// A fact about the learning run (`commit`: what was checked out then; `source`, `learned_at`).
    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// What a test ran when the map was learned: (function, its code hash then, how many tests ran it).
    /// `name` is `test_b` or `TestA::test_b`; a bare name matches the test of any class in the file.
    pub fn ran_by(&self, file: &str, name: &str) -> Vec<(String, String, u32)> {
        let run = || -> rusqlite::Result<Vec<(String, String, u32)>> {
            let mut st = self.conn.prepare(
                "SELECT c.symbol, c.body_hash, (SELECT count(*) FROM covers x WHERE x.symbol = c.symbol)
                 FROM covers c JOIN tests t ON t.id = c.test
                 WHERE t.file = ?1 AND (t.name = ?2 OR substr(t.name, -length(?3)) = ?3) GROUP BY c.symbol ORDER BY c.symbol",
            )?;
            let rows = st.query_map(params![file, name, format!("::{name}")], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect()
        };
        run().unwrap_or_default()
    }

    /// (functions with at least one test, tests, pairs)
    pub fn size(&self) -> (usize, usize, usize) {
        let n = |sql: &str| self.conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (n("SELECT count(DISTINCT symbol) FROM covers"), n("SELECT count(*) FROM tests"), n("SELECT count(*) FROM covers"))
    }
}

/// Lines each test ran, read from a coverage.py data file (`.coverage` is SQLite):
/// (source file as recorded, context — pytest-cov writes `tests/test_x.py::test_a[p]|run` —, lines).
/// Code run outside any test (imports) has an empty context and is left out. A project that
/// measures branches (`branch = true`) has arcs instead of lines: both ends of an arc ran.
pub fn read_coverage(data: &Path) -> Result<Vec<(String, String, Vec<u32>)>> {
    let conn = Connection::open_with_flags(data, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut out = Vec::new();
    {
        let mut st = conn.prepare("SELECT f.path, c.context, b.numbits FROM line_bits b JOIN file f ON f.id = b.file_id JOIN context c ON c.id = b.context_id WHERE c.context != ''")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
        for row in rows {
            let (file, context, bits) = row?;
            // coverage.py "numbits": bit i of byte b set = line b * 8 + i ran
            let lines: Vec<u32> = bits.iter().enumerate().flat_map(|(b, byte)| (0..8u32).filter(move |i| byte & (1 << i) != 0).map(move |i| b as u32 * 8 + i)).collect();
            out.push((file, context, lines));
        }
    }
    let mut arcs: HashMap<(i64, i64), std::collections::BTreeSet<u32>> = HashMap::new();
    {
        let mut st = conn.prepare("SELECT a.file_id, a.context_id, a.fromno, a.tono FROM arc a JOIN context c ON c.id = a.context_id WHERE c.context != ''")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?;
        for row in rows {
            let (file, context, from, to) = row?;
            let set = arcs.entry((file, context)).or_default();
            // a negative end is "entered / left the code object", not a line
            set.extend([from, to].into_iter().filter(|l| *l > 0).map(|l| l as u32));
        }
    }
    if !arcs.is_empty() {
        let names = |table: &str, col: &str| -> Result<HashMap<i64, String>> {
            let mut st = conn.prepare(&format!("SELECT id, {col} FROM {table}"))?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        let (files, contexts) = (names("file", "path")?, names("context", "context")?);
        for ((f, c), lines) in arcs {
            if let (Some(file), Some(context)) = (files.get(&f), contexts.get(&c)) {
                out.push((file.clone(), context.clone(), lines.into_iter().collect()));
            }
        }
    }
    Ok(out)
}
