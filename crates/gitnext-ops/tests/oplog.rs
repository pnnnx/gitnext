//! Operation log: every gitnext edit can be undone on its own; undo never loses changes made since.

use gitnext_ops::edit::{self, EditKind};
use gitnext_ops::gitnext_index::{Graph, Index};
use gitnext_ops::oplog;
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitnext-oplog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

fn graph(dir: &Path) -> Graph {
    let mut idx = Index::open(dir).unwrap();
    idx.refresh().unwrap();
    idx.graph().unwrap()
}

fn read(dir: &Path, f: &str) -> String {
    std::fs::read_to_string(dir.join(f)).unwrap()
}

fn edit(dir: &Path, sym: &str, kind: EditKind, code: &str) -> u64 {
    let g = graph(dir);
    let i = g.find(sym)[0];
    edit::edit_symbol(dir, &g, i, kind, code, false).unwrap();
    oplog::load(dir).last().unwrap().id
}

const TWO: &str = "def f(x):\n    return x\n\n\ndef g(y):\n    return y\n";

#[test]
fn undo_replace_keeps_a_later_edit_of_another_function() {
    let dir = project("replace", &[("a.py", TWO)]);
    let op_f = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 1\n");
    let op_g = edit(&dir, "g", EditKind::Replace, "def g(y):\n    return y * 2\n");
    assert_eq!(op_g, op_f + 1);
    let r = oplog::undo(&dir, op_f).unwrap();
    assert_eq!(r.files, vec!["a.py"]);
    assert_eq!(read(&dir, "a.py"), "def f(x):\n    return x\n\n\ndef g(y):\n    return y * 2\n");
    let ops = oplog::load(&dir);
    assert_eq!(ops.last().unwrap().undoes, Some(op_f));
    assert!(oplog::render(&ops, 10).contains(&format!("[undone by #{}]", r.op.unwrap())));
}

#[test]
fn undo_insert_gives_the_file_back_byte_for_byte() {
    let dir = project("insert", &[("a.py", TWO)]);
    let op = edit(&dir, "f", EditKind::InsertAfter, "def h():\n    return 0\n");
    assert!(read(&dir, "a.py").contains("def h()"));
    oplog::undo(&dir, op).unwrap();
    assert_eq!(read(&dir, "a.py"), TWO);
}

#[test]
fn undo_rename_across_files_in_one_step() {
    let files = [
        ("pkg/__init__.py", ""),
        ("pkg/runner.py", "# call `detect` first\ndef detect(root):\n    return root\n"),
        ("pkg/cli.py", "from .runner import detect\n\n\ndef main():\n    print(detect('.'))\n"),
        ("tests/test_runner.py", "from pkg.runner import detect\n\n\ndef test_detect():\n    assert detect(1) == 1\n"),
        ("notes.txt", "free text\n"),
    ];
    let dir = project("rename", &files);
    let g = graph(&dir);
    let i = g.find("detect")[0];
    let before = oplog::load(&dir).len();
    {
        // as `gitnext rename --text` does it: the rename and the text pass are one operation
        let _op = oplog::Group::start(&dir, "rename", "detect → find");
        let mut r = edit::rename(&dir, &g, i, "find", false).unwrap();
        edit::rename_leftover_text(&dir, &mut r).unwrap();
    }
    let ops = oplog::load(&dir);
    assert_eq!(ops.len(), before + 1, "one operation for the whole rename");
    let op = ops.last().unwrap();
    assert_eq!(op.files.len(), 3);
    assert!(read(&dir, "pkg/runner.py").contains("# call `find` first"));
    std::fs::write(dir.join("notes.txt"), "free text, edited by hand\n").unwrap();
    oplog::undo(&dir, op.id).unwrap();
    for (p, c) in &files[1..4] {
        assert_eq!(&read(&dir, p), c, "{p}");
    }
    assert_eq!(read(&dir, "notes.txt"), "free text, edited by hand\n");
}

#[test]
fn undo_refuses_when_the_block_was_rewritten_and_writes_nothing() {
    let dir = project("conflict", &[("a.py", TWO)]);
    let op = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 1\n");
    let by_hand = read(&dir, "a.py").replace("x + 1", "x + 2");
    std::fs::write(dir.join("a.py"), &by_hand).unwrap();
    let e = oplog::undo(&dir, op).unwrap_err().to_string();
    assert!(e.contains("nothing undone") && e.contains("a.py") && e.contains("by hand"), "{e}");
    assert_eq!(read(&dir, "a.py"), by_hand);
}

#[test]
fn two_edits_of_one_function_undo_newest_first() {
    let dir = project("order", &[("a.py", TWO)]);
    let first = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 1\n");
    let second = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 2\n");
    let now = read(&dir, "a.py");
    let e = oplog::undo(&dir, first).unwrap_err().to_string();
    assert!(e.contains(&format!("by #{second}")) && e.contains("undo those first"), "{e}");
    assert_eq!(read(&dir, "a.py"), now);
    oplog::undo(&dir, second).unwrap();
    oplog::undo(&dir, first).unwrap();
    assert_eq!(read(&dir, "a.py"), TWO);
}

#[test]
fn crlf_and_missing_final_newline_survive_undo() {
    for (name, text) in [("crlf", TWO.replace('\n', "\r\n")), ("nofinal", TWO.trim_end().to_string())] {
        let dir = project(name, &[("a.py", &text)]);
        let op = edit(&dir, "g", EditKind::Replace, "def g(y):\n    return y * 2\n");
        assert_ne!(read(&dir, "a.py"), text);
        oplog::undo(&dir, op).unwrap();
        assert_eq!(read(&dir, "a.py").as_bytes(), text.as_bytes(), "{name}");
    }
}

#[test]
fn undo_of_an_undo_redoes_and_double_undo_is_refused() {
    let dir = project("redo", &[("a.py", TWO)]);
    let op = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return -x\n");
    let done = read(&dir, "a.py");
    let u = oplog::undo(&dir, op).unwrap().op.unwrap();
    assert!(oplog::undo(&dir, op).unwrap_err().to_string().contains("already undone"));
    oplog::undo(&dir, u).unwrap();
    assert_eq!(read(&dir, "a.py"), done);
}

#[test]
fn log_is_capped_and_ids_keep_counting() {
    let dir = project("cap", &[("a.txt", "0\n")]);
    std::fs::create_dir_all(dir.join(".gitnext")).unwrap();
    let mut prev = "0\n".to_string();
    for k in 1..=oplog::MAX_OPS + 100 {
        let next = format!("{k}\n");
        oplog::record(&dir, "replace", "a", &[("a.txt".into(), prev.clone(), next.clone())]);
        prev = next;
    }
    let ops = oplog::load(&dir);
    assert!(ops.len() <= oplog::MAX_OPS, "{}", ops.len());
    assert_eq!(ops.last().unwrap().id, (oplog::MAX_OPS + 100) as u64);
    assert!(ops.windows(2).all(|w| w[1].id == w[0].id + 1));
}

#[test]
fn parallel_writers_get_distinct_ids() {
    let dir = project("lock", &[]);
    std::fs::create_dir_all(dir.join(".gitnext")).unwrap();
    let hs: Vec<_> = (0..8)
        .map(|t| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                for k in 0..20 {
                    oplog::record(&dir, "replace", "x", &[(format!("f{t}.txt"), format!("{k}\n"), format!("{}\n", k + 1))]);
                }
            })
        })
        .collect();
    hs.into_iter().for_each(|h| h.join().unwrap());
    let mut ids: Vec<u64> = oplog::load(&dir).iter().map(|o| o.id).collect();
    ids.sort();
    assert_eq!(ids, (1..=160).collect::<Vec<u64>>());
}

#[test]
fn undo_survives_lines_added_above_and_below_by_hand() {
    // the last function of a file, then code appended after it: the usual order of an agent's edits
    let dir = project("ends", &[("a.py", TWO)]);
    let op_f = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 1\n");
    let op_g = edit(&dir, "g", EditKind::Replace, "def g(y):\n    return y * 2\n");
    let text = format!("import os\n{}\n\ndef z():\n    return 0\n", read(&dir, "a.py"));
    std::fs::write(dir.join("a.py"), &text).unwrap();
    oplog::undo(&dir, op_g).unwrap();
    oplog::undo(&dir, op_f).unwrap();
    assert_eq!(read(&dir, "a.py"), format!("import os\n{TWO}\n\ndef z():\n    return 0\n"));
}

#[test]
fn undo_refuses_a_file_outside_the_repository() {
    let dir = project("outside", &[("a.py", TWO)]);
    let op = edit(&dir, "f", EditKind::Replace, "def f(x):\n    return x + 1\n");
    let outside = dir.parent().unwrap().join(format!("gitnext-oplog-outside-{}.txt", std::process::id()));
    std::fs::write(&outside, read(&dir, "a.py")).unwrap();
    let log = oplog::path(&dir);
    let name = outside.file_name().unwrap().to_str().unwrap();
    for forged in [format!("../{name}"), outside.to_str().unwrap().to_string()] {
        std::fs::write(&log, std::fs::read_to_string(&log).unwrap().replace("\"path\":\"a.py\"", &format!("\"path\":\"{forged}\""))).unwrap();
        let err = oplog::undo(&dir, op).unwrap_err().to_string();
        assert!(err.contains("outside the repository"), "{err}");
        assert!(read(&dir, "a.py").contains("x + 1") && std::fs::read_to_string(&outside).unwrap().contains("x + 1"));
        std::fs::write(&log, std::fs::read_to_string(&log).unwrap().replace(&format!("\"path\":\"{forged}\""), "\"path\":\"a.py\"")).unwrap();
    }
    let _ = std::fs::remove_file(outside);
}
