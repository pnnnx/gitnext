//! How far each kind of answer can be trusted — measured, not assumed.
//!
//! gitnext says *where* things are and *what is connected*; the agent decides what to
//! change; the compiler and the tests have the last word. Between those lies a line the
//! agent should not have to guess: a "sure" caller can be acted on, a test list from the
//! call graph is a starting point. The numbers are what the project's own eval measured
//! (`eval/results/latest.md`, run 6 of 12 Nov 2026: ten repos, two per language, 232 functions for
//! callers and 166 for tests) — update them when the eval moves by more than its threshold.

/// (language family, share of "sure" callers that are real, share of failing tests the
/// call graph finds) — in percent; `None` = not measured for that language.
const MEASURED: &[(&str, Option<u8>, Option<u8>)] = &[
    ("go", Some(98), Some(79)),
    ("java", Some(100), Some(49)),
    ("js", Some(100), Some(3)),
    ("rust", Some(100), Some(53)),
    ("python", None, Some(36)),
];

fn row(file: &str) -> Option<&'static (&'static str, Option<u8>, Option<u8>)> {
    let fam = gitnext_parse::family_of(file)?;
    MEASURED.iter().find(|(f, _, _)| *f == fam)
}

/// `≈97% real` for callers marked sure in a file of this language; empty when not measured.
pub fn callers_sure(file: &str) -> String {
    row(file).and_then(|r| r.1).map(|p| format!("≈{p}% real")).unwrap_or_default()
}

/// What the agent must know about a test list that came from the call graph.
pub fn tests_note(file: &str) -> String {
    // Python can do better than the call graph: one coverage run gives the real list
    let learn = if gitnext_parse::family_of(file) == Some("python") { "; `gitnext learn-tests` learns the real list from one test run" } else { "" };
    match row(file).and_then(|r| r.2) {
        Some(p) => format!("the call graph finds ≈{p}% of the tests that would fail — run the whole test file before you finish{learn}"),
        None => "how many failing tests the call graph finds is not measured for this language — run the whole test file before you finish".to_string(),
    }
}

/// For a list that came from a coverage run (`gitnext learn-tests`) instead of the call graph.
pub fn tests_note_learned(stale: bool) -> String {
    let base = "these tests ran this code in the coverage run (measured on click: 98% of such tests fail when the code breaks, and none is missing)";
    if stale {
        format!("{base}; the code changed since that run — `gitnext learn-tests` refreshes the map")
    } else {
        base.to_string()
    }
}

/// The same, as short as `look` needs it.
pub fn tests_short(file: &str) -> String {
    row(file).and_then(|r| r.2).map(|p| format!("; ≈{p}% of the failing ones — run the file too")).unwrap_or_default()
}
