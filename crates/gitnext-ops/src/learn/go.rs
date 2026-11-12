//! Go: not here yet. The prototype (`eval/learn_tests_go.py`, `eval/learn_tests_rust.py`) says it is worth it: a test binary built with coverage and run one test at a time finds 100% of the failing tests (the call graph: 78%) for 1.2–5× a plain test run.
//! What `run` has to return is in `super::Run`; `super::owners` gives the function around a line.

use std::path::Path;

use anyhow::{bail, Result};
use gitnext_index::Graph;

use super::Run;

pub(super) fn run(_root: &Path, _g: &Graph) -> Result<Run> {
    bail!("not supported yet")
}
