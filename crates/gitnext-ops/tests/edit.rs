//! `gitnext edit`: a symbol replaced with its own code is the same file; a deleted symbol takes its
//! whole doc comment with it.

use gitnext_ops::edit::{edit_symbol, EditKind};
use gitnext_ops::gitnext_index::Index;
use gitnext_ops::render;

const JAVA: &str = "\
package demo;

public class Range {
    /**
     Maps a slot to the start of its name.
     */
    private static int nameStart(int index) {
        return index * 4;
    }

    /**
     * Reads by index.
     * @return the element
     */
    private static Object readByIndex(Object[] items, String token) {
        try {
            int index = Integer.parseInt(token);
            return items[index];
       } catch (NumberFormatException e) {
\t\t\treturn null;
       }
    }

    int last; /* not a doc
    of anything */
    static int width() {
        return 4;
    }
}
";

fn project() -> (std::path::PathBuf, Index) {
    let dir = std::env::temp_dir().join(format!("gitnext-edit-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/Range.java"), JAVA).unwrap();
    let mut idx = Index::open(&dir).unwrap();
    idx.refresh().unwrap();
    (dir, idx)
}

#[test]
fn replace_with_its_own_code_changes_nothing() {
    let (dir, idx) = project();
    let g = idx.graph().unwrap();
    let i = g.find("readByIndex")[0];
    // the code as `read` gives it: the doc comment, and two lines indented less than the first
    let shown = render::read(&g, &dir, &[i]);
    let code: Vec<&str> = shown.lines().skip(2).collect();
    let code = code[..code.len() - 1].join("\n");
    assert!(code.starts_with("    /**\n     * Reads by index.") && code.contains("\n       } catch"), "{shown}");
    edit_symbol(&dir, &g, i, EditKind::Replace, &code, false).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("src/Range.java")).unwrap(), JAVA);
    // sent flush-left, as an agent sends it: every line that began with the four spaces is back in place
    let flush: String = code.lines().map(|l| l.strip_prefix("    ").unwrap_or(l)).collect::<Vec<_>>().join("\n");
    edit_symbol(&dir, &g, i, EditKind::Replace, &flush, false).unwrap();
    let again = std::fs::read_to_string(dir.join("src/Range.java")).unwrap();
    assert!(again.contains("    private static Object readByIndex(") && again.contains("\n            int index = Integer.parseInt(token);\n"), "{again}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_takes_the_whole_doc_comment() {
    let (dir, idx) = project();
    let g = idx.graph().unwrap();
    let i = g.find("nameStart")[0];
    // a Javadoc whose lines do not start with `*` is still the doc of the method
    assert!(render::read(&g, &dir, &[i]).starts_with("src/Range.java:4-9 "), "{}", render::read(&g, &dir, &[i]));
    edit_symbol(&dir, &g, i, EditKind::Delete, "", false).unwrap();
    let left = std::fs::read_to_string(dir.join("src/Range.java")).unwrap();
    assert_eq!(left, JAVA.replace("    /**\n     Maps a slot to the start of its name.\n     */\n    private static int nameStart(int index) {\n        return index * 4;\n    }\n\n", ""));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_comment_that_ends_a_line_of_code_is_not_a_doc() {
    let (dir, idx) = project();
    let g = idx.graph().unwrap();
    let i = g.find("width")[0];
    assert!(render::read(&g, &dir, &[i]).starts_with("src/Range.java:26-28 "), "{}", render::read(&g, &dir, &[i]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `replace` given a line of the code where the name belongs: the edit that was meant is made.
#[test]
fn a_line_of_the_code_given_as_the_name() {
    use gitnext_ops::edit::{is_code, replace_text};
    const EVAL: &str = "pub fn matches(a: u32, b: u32) -> bool {\n    if a == b {\n        return true;\n    }\n    a > b\n}\n\npub fn other(a: u32, b: u32) -> bool {\n    let same = a == b;\n    same || a > b\n}\n";
    let dir = std::env::temp_dir().join(format!("gitnext-edit-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::write(dir.join("src/lib.rs"), EVAL).unwrap();
    let graph = || {
        let mut idx = Index::open(&dir).unwrap();
        idx.refresh().unwrap();
        idx.graph().unwrap()
    };
    let read = || std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    assert!(is_code("if a == b {") && is_code("fn matches(a: u32, b: u32)") && !is_code("eval::matches") && !is_code("Version::fmt@33"));
    // one line for another: the text is replaced where it stands, at its indent
    let r = replace_text(&dir, &graph(), "if a == b {", "if a != b {", false).unwrap();
    assert_eq!((r.file.as_str(), r.lines), ("src/lib.rs", (2, 2)));
    assert_eq!(read(), EVAL.replace("if a == b {", "if a != b {"));
    // inside a line
    replace_text(&dir, &graph(), "same || a > b", "same || a < b", false).unwrap();
    assert!(read().contains("    same || a < b\n"), "{}", read());
    std::fs::write(dir.join("src/lib.rs"), EVAL).unwrap();
    // the first line of a function as the name, the whole function as the code: the function is replaced
    let whole = "pub fn matches(a: u32, b: u32) -> bool {\n    a >= b\n}\n";
    replace_text(&dir, &graph(), "pub fn matches(a: u32, b: u32) -> bool {", whole, false).unwrap();
    assert_eq!(read(), EVAL.replace("pub fn matches(a: u32, b: u32) -> bool {\n    if a == b {\n        return true;\n    }\n    a > b\n}\n", whole));
    std::fs::write(dir.join("src/lib.rs"), EVAL).unwrap();
    // a line inside the function as the name, the whole function as the code: the same
    replace_text(&dir, &graph(), "return true;", whole, false).unwrap();
    assert!(read().starts_with(whole) && read().contains("pub fn other"), "{}", read());
    std::fs::write(dir.join("src/lib.rs"), EVAL).unwrap();
    // in two places: both are named, nothing is written
    let e = replace_text(&dir, &graph(), "a == b", "a != b", false).unwrap_err().to_string();
    assert!(e.starts_with("`a == b` is in 2 places: src/lib.rs:2 (in matches@1), src/lib.rs:9 (in other@8); name the function to change"), "{e}");
    // a whole function for a line of another function is not put there
    let e = replace_text(&dir, &graph(), "let same = a == b;", whole, false).unwrap_err().to_string();
    assert!(e.starts_with("the code is a whole function or class, but `let same = a == b;` stands in another one (in other@8)"), "{e}");
    let e = replace_text(&dir, &graph(), "if a <= b {", "if a < b {", false).unwrap_err().to_string();
    assert!(e.starts_with("`if a <= b {` is not the name of a function or class, and no line of the code has this text"), "{e}");
    assert_eq!(read(), EVAL);
    // what would not parse is refused as with a symbol
    let e = replace_text(&dir, &graph(), "if a == b {", "if a == b {{{", false).unwrap_err().to_string();
    assert!(e.contains("would not parse"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
