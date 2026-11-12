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
