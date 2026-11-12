//! How far each kind of answer can be trusted — measured, not assumed.
//!
//! gitnext says *where* things are and *what is connected*; the agent decides what to
//! change; the compiler and the tests have the last word. Between those lies a line the
//! agent should not have to guess: a "sure" caller can be acted on, a test list from the
//! call graph is a starting point. The numbers are what the project's own eval measured
//! (`eval/results/latest.md`, run 5 of 12 Nov 2026) — update them when the eval moves.

/// (language family, share of "sure" callers that are real, share of failing tests the
/// call graph finds) — in percent; `None` = not measured for that language.
const MEASURED: &[(&str, Option<u8>, Option<u8>)] = &[
    ("go", Some(97), Some(82)),
    ("java", Some(100), Some(19)),
    ("js", Some(100), Some(33)),
    ("rust", Some(100), Some(61)),
    ("python", None, Some(19)),
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
    match row(file).and_then(|r| r.2) {
        Some(p) => format!("the call graph finds ≈{p}% of the tests that would fail — run the whole test file before you finish"),
        None => "how many failing tests the call graph finds is not measured for this language — run the whole test file before you finish".to_string(),
    }
}

/// The same, as short as `look` needs it.
pub fn tests_short(file: &str) -> String {
    row(file).and_then(|r| r.2).map(|p| format!("; ≈{p}% of the failing ones — run the file too")).unwrap_or_default()
}
