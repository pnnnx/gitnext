//! `gitnext bench`: token cost of typical agent tasks on *this* repository,
//! naive (grep + read whole files + rewrite files) vs gitnext. Deterministic.

use crate::render::{self, Opts};
use gitnext_index::{dir_of, Graph};
use gitnext_parse::Kind;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Default, Serialize, Clone)]
pub struct Row {
    pub task: String,
    pub cases: usize,
    pub naive: usize,
    pub gitnext: usize,
}

impl Row {
    fn saved(&self) -> f64 {
        if self.naive == 0 {
            0.0
        } else {
            100.0 * (1.0 - self.gitnext as f64 / self.naive as f64)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Bench {
    pub symbols: usize,
    pub dirs: usize,
    pub rows: Vec<Row>,
    pub goals: Vec<(String, bool)>,
}

fn tok(s: &str) -> usize {
    s.len().div_ceil(4)
}

struct Corpus {
    files: HashMap<String, String>,
}

impl Corpus {
    fn load(root: &Path, g: &Graph) -> Corpus {
        let files = g
            .files
            .iter()
            .filter(|f| f.parsed)
            .filter_map(|f| std::fs::read_to_string(root.join(&f.path)).ok().map(|t| (f.path.clone(), t)))
            .collect();
        Corpus { files }
    }
    fn file_tok(&self, f: &str) -> usize {
        self.files.get(f).map(|t| tok(t)).unwrap_or(0)
    }
    /// `rg -n -w name` output size and the files it hits.
    fn grep(&self, word: &str) -> (usize, Vec<String>) {
        let mut cost = 0;
        let mut hits = BTreeSet::new();
        for (f, t) in &self.files {
            for l in t.lines() {
                let b = l.as_bytes();
                let whole = l.match_indices(word).any(|(i, _)| {
                    (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
                        && (i + word.len() >= b.len() || !(b[i + word.len()].is_ascii_alphanumeric() || b[i + word.len()] == b'_'))
                });
                if whole {
                    cost += tok(f) + tok(l) + 2;
                    hits.insert(f.clone());
                }
            }
        }
        (cost, hits.into_iter().collect())
    }
}

/// Evenly spread, deterministic sample of non-trivial, non-test callables.
fn sample_symbols(g: &Graph, n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..g.syms.len())
        .filter(|&i| {
            let s = &g.syms[i].sym;
            s.kind.is_callable() && !s.test && !s.decl && s.end_line >= s.start_line + 2 && s.name.len() >= 3
        })
        .collect();
    v.sort_by(|&a, &b| g.syms[a].sym.qualified.cmp(&g.syms[b].sym.qualified));
    if v.len() <= n {
        return v;
    }
    (0..n).map(|k| v[k * v.len() / n]).collect()
}

/// Directories worth orienting in: ≥ 2 parsed files and ≥ 2k tokens of source
/// (smaller ones are cheap to just read). Falls back to all multi-file dirs.
fn sample_dirs(g: &Graph, c: &Corpus, n: usize) -> Vec<String> {
    let multi: Vec<String> = g
        .dirs()
        .into_iter()
        .filter(|d| g.files.iter().filter(|f| f.parsed && dir_of(&f.path) == d).count() >= 2)
        .collect();
    let big: Vec<String> = multi
        .iter()
        .filter(|d| g.files.iter().filter(|f| f.parsed && dir_of(&f.path) == d.as_str()).map(|f| c.file_tok(&f.path)).sum::<usize>() >= 2000)
        .cloned()
        .collect();
    let mut d = if big.is_empty() { multi } else { big };
    d.sort();
    if d.len() <= n {
        return d;
    }
    (0..n).map(|k| d[k * d.len() / n].clone()).collect()
}

pub fn run(root: &Path, g: &Graph, n_syms: usize, n_dirs: usize) -> Bench {
    let c = Corpus::load(root, g);
    let o = Opts::default();
    let syms = sample_symbols(g, n_syms);
    let dirs = sample_dirs(g, &c, n_dirs);
    let ctx = gitnext_docs::Ctx::new(root, g);

    let mut orient = Row { task: "orient in a directory".into(), ..Default::default() };
    for d in &dirs {
        orient.cases += 1;
        orient.naive += g.files.iter().filter(|f| f.parsed && dir_of(&f.path) == d).map(|f| c.file_tok(&f.path)).sum::<usize>();
        orient.gitnext += tok(&gitnext_docs::render_dir(&ctx, d));
    }

    let mut find = Row { task: "find + understand a symbol".into(), ..Default::default() };
    let mut callers = Row { task: "who calls it".into(), ..Default::default() };
    let mut edit = Row { task: "edit one function".into(), ..Default::default() };
    let mut impact = Row { task: "impact of a signature change".into(), ..Default::default() };
    for &i in &syms {
        let r = &g.syms[i];
        let name = r.sym.name.as_str();
        let (grep_cost, hit_files) = c.grep(name);
        let own = c.file_tok(&r.file);
        let body = tok(&render::read(g, root, &[i]));
        // find + understand: grep, open the defining file / show + read
        find.cases += 1;
        find.naive += grep_cost + own;
        find.gitnext += tok(&render::show(g, i, o)) + body;
        // callers: grep, then open up to 5 hit files to see the call sites in context
        callers.cases += 1;
        callers.naive += grep_cost + hit_files.iter().filter(|f| **f != r.file).take(5).map(|f| c.file_tok(f)).sum::<usize>();
        callers.gitnext += tok(&render::callers(g, i, o));
        // edit: read the file + write it back whole / read symbol + send new code
        edit.cases += 1;
        edit.naive += own * 2;
        edit.gitnext += body * 2 + 12;
        // impact: grep + open every hit file / impact tree
        impact.cases += 1;
        impact.naive += grep_cost + hit_files.iter().map(|f| c.file_tok(f)).sum::<usize>();
        impact.gitnext += tok(&render::impact(g, i, 3, o));
    }
    let rows = vec![orient, find, callers, edit, impact];
    let pct = |r: &Row| if r.naive == 0 { 1.0 } else { r.gitnext as f64 / r.naive as f64 };
    let goals = vec![
        ("orient ≤ 10% of reading the directory".to_string(), pct(&rows[0]) <= 0.10),
        ("understand a symbol ≤ 25% of grep + read file".to_string(), pct(&rows[1]) <= 0.25),
        ("edit ≤ 5% of read + rewrite file".to_string(), pct(&rows[3]) <= 0.05),
    ];
    let _ = Kind::Function;
    Bench { symbols: syms.len(), dirs: dirs.len(), rows, goals }
}

pub fn render(b: &Bench) -> String {
    let k = gitnext_docs::fmt_k;
    let mut s = format!(
        "bench on this repo: {} symbols, {} directories (≈ tokens in + out, lower is better)\n{:<30} {:>9} {:>9} {:>7}\n",
        b.symbols, b.dirs, "task", "naive", "gitnext", "saved"
    );
    let mut tn = 0;
    let mut tg = 0;
    for r in &b.rows {
        tn += r.naive;
        tg += r.gitnext;
        let _ = writeln!(s, "{:<30} {:>9} {:>9} {:>6.0}%", r.task, k(r.naive), k(r.gitnext), r.saved());
    }
    let total = Row { task: "total".into(), cases: 0, naive: tn, gitnext: tg };
    let _ = writeln!(s, "{:<30} {:>9} {:>9} {:>6.0}%", "total", k(tn), k(tg), total.saved());
    let _ = writeln!(s, "goals:");
    for (g, ok) in &b.goals {
        let _ = writeln!(s, "  {} {g}", if *ok { "✓" } else { "✗" });
    }
    s.push_str("naive = grep -w + read whole files (+ rewrite the file to edit); gitnext = map / show+read / callers / read+replace_symbol / impact\n");
    s
}
