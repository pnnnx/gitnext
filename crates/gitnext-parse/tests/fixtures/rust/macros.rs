//! Functions declared inside macro calls: property tests and generated items.

use std::collections::HashMap;

/// Counts words.
pub struct Counter {
    words: HashMap<String, usize>,
}

impl Counter {
    pub fn new() -> Self {
        Counter { words: HashMap::new() }
    }

    pub fn add(&mut self, w: &str) {
        *self.words.entry(w.to_string()).or_default() += 1;
    }

    pub fn get(&self, w: &str) -> usize {
        self.words.get(w).copied().unwrap_or(0)
    }

    accessors! {
        /// How many different words.
        pub fn distinct(&self) -> usize { self.words.len() }
    }
}

fn normalize(w: &str) -> String {
    w.to_lowercase()
}

// A wrapper like indexmap's `quickcheck_limit!`: its rules write `#[test]`.
macro_rules! props {
    { $( fn $name:ident($($a:ident : $t:ty),*) -> bool { $($body:tt)* } )* } => {
        $( #[test] fn $name() { fn prop($($a: $t),*) -> bool { $($body)* } let _ = prop; } )*
    };
}

props! {
    /// Adding a word makes it count once.
    fn add_then_get(w: String) -> bool {
        let mut c = Counter::new();
        c.add(&w);
        c.get(&w) == 1
    }

    fn normalize_is_idempotent(w: String) -> bool {
        normalize(&normalize(&w)) == normalize(&w)
    }
}

quickcheck::quickcheck! {
    fn distinct_never_exceeds_adds(ws: Vec<String>) -> bool {
        let mut c = Counter::new();
        for w in &ws {
            c.add(w);
        }
        c.distinct() <= ws.len()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn get_of_unknown_is_zero(w in "[a-z]{1,8}") {
        prop_assert_eq!(Counter::new().get(&w), 0);
    }
}

thread_local! {
    static SEEN: std::cell::Cell<usize> = std::cell::Cell::new(0);
}

generate!(fn generated_helper() -> String { normalize("X") });

fn not_items() {
    inner! { fn hidden() {} }
}
