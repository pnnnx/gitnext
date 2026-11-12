//! Several processes / threads open the same fresh repository at once (two tests of one
//! binary, an agent's MCP server next to the CLI): each must wait for the others, not fail
//! with "database is locked".

use gitnext_index::Index;

#[test]
fn many_openers_of_a_fresh_repo() {
    for round in 0..12 {
        let dir = std::env::temp_dir().join(format!("gitnext-concurrent-{}-{round}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        for i in 0..20 {
            std::fs::write(dir.join(format!("src/m{i}.py")), format!("def f{i}():\n    return g{i}()\n\ndef g{i}():\n    pass\n")).unwrap();
        }
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (dir, start) = (dir.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    let mut idx = Index::open(&dir).map_err(|e| format!("open: {e:#}"))?;
                    idx.refresh().map_err(|e| format!("refresh: {e:#}"))?;
                    let g = idx.graph().map_err(|e| format!("graph: {e:#}"))?;
                    Ok::<usize, String>(g.find("f7").len())
                })
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), Ok(1), "round {round}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
