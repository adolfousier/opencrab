//! Concurrent saves never corrupt the context store (#906).
//!
//! `save()` used `fs::write`, which truncates at OPEN, not per write. Two
//! concurrent savers therefore each held a descriptor at offset 0: if the
//! longer writer finished first and the shorter one wrote over it, the longer
//! object's tail survived, leaving `{valid shorter object}{leftover}` and a
//! "trailing characters" parse failure on the next read.
//!
//! #853 fixed it with a uniquely-named temp file plus an atomic rename. The PR
//! proposed this test and did not write it, so nothing prevented a refactor
//! reintroducing the race.
//!
//! Against the old `fs::write` these fail; against rename they hold.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::tools::context::ContextStore;

fn tmp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("opencrabs-ctx-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&p);
    p.join("context_store.json")
}

/// Payload sizes must DIFFER, or a shorter write cannot leave a longer one's
/// tail behind and the race cannot reproduce.
fn store_with_bulk(session: &str, facts: usize) -> ContextStore {
    let mut s = ContextStore::new(session.to_string());
    s.facts = (0..facts)
        .map(|i| format!("fact number {i} recorded"))
        .collect();
    s
}

#[tokio::test]
async fn concurrent_saves_never_leave_a_partial_object() {
    let path = tmp_path("concurrent");
    let _ = std::fs::remove_file(&path);

    let mut handles = Vec::new();
    for i in 0..32 {
        let path = path.clone();
        handles.push(tokio::spawn(async move {
            // Alternating bulk: a short write landing over a long one is the
            // exact shape that used to corrupt.
            let store = store_with_bulk("s", if i % 2 == 0 { 400 } else { 2 });
            store.save(&path).await
        }));
    }
    for h in handles {
        let _ = h.await.expect("save task panicked");
    }

    ContextStore::load(&path, "s")
        .await
        .expect("store did not parse after concurrent saves");
}

#[tokio::test]
async fn every_intermediate_read_parses() {
    // Readers run WHILE writers do. Under the old code a read could catch the
    // mixed state; with rename it sees either the old object or the new one.
    let path = tmp_path("interleaved");
    let _ = std::fs::remove_file(&path);
    store_with_bulk("s", 10).save(&path).await.expect("seed");

    let writer = {
        let path = path.clone();
        tokio::spawn(async move {
            for i in 0..40 {
                let store = store_with_bulk("s", if i % 2 == 0 { 300 } else { 1 });
                let _ = store.save(&path).await;
            }
        })
    };
    let reader = {
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..40 {
                if path.exists() {
                    ContextStore::load(&path, "s")
                        .await
                        .expect("a concurrent read saw a partial object");
                }
                tokio::task::yield_now().await;
            }
        })
    };
    writer.await.expect("writer panicked");
    reader.await.expect("reader panicked");
}

#[tokio::test]
async fn no_temp_files_are_left_behind() {
    // The cleanup half of #906: a completed run must leave only the store.
    let path = tmp_path("no-temps");
    let _ = std::fs::remove_file(&path);
    for i in 0..8 {
        store_with_bulk("s", i * 20)
            .save(&path)
            .await
            .expect("save");
    }
    let dir = path.parent().expect("parent");
    let strays: Vec<_> = std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(strays.is_empty(), "temp files left behind: {strays:?}");
}
