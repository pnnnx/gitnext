//! Rust: not here yet. The prototype (`eval/learn_tests_go.py`, `eval/learn_tests_rust.py`) says it is worth it: `cargo llvm-cov` run one test at a time finds 100% of the failing tests (the call graph: 59%); the first instrumented build takes 12–21 s and about 350 MB.
//! What `run` has to return is in `super::Run`; `super::owners` gives the function around a line.

use std::path::Path;

use anyhow::{bail, Result};
use gitnext_index::Graph;

use super::Run;

pub(super) fn run(_root: &Path, _g: &Graph) -> Result<Run> {
    bail!("not supported yet")
}
