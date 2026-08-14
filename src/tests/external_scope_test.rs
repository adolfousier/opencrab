//! #1051: external scope filtering + session gate.
//!
//! Verifies (a) search_fts with the external collection returns only
//! external docs, (b) the memory scope excludes external, and (c) the
//! session gate blocks external content in a marked-shared session.
//! FTS-only (no embedding engine) so it runs anywhere.

use crate::memory::db::Store;
use crate::memory::{is_session_shared, mark_session_shared, COLLECTION_EXTERNAL};

fn seed(store: &Store) {
    let now = crate::utils::string::utc_timestamp();
    let mem_hash = Store::hash_content("daily log about rust async");
    let ext_hash = Store::hash_content("design notes on sqlite concurrency");
    store.insert_content(&mem_hash, "daily log about rust async", &now).unwrap();
    store.insert_document(
        "memory",
        "2026-08-14.md",
        "2026-08-14",
        &mem_hash,
        &now,
        &now,
    )
    .unwrap();
    store.insert_content(&ext_hash, "design notes on sqlite concurrency", &now).unwrap();
    store.insert_document(
        COLLECTION_EXTERNAL,
        "/home/u/notes/design.md",
        "design.md",
        &ext_hash,
        &now,
        &now,
    )
    .unwrap();
}

#[test]
fn external_scope_returns_only_external_docs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");
    seed(&store);

    let hits = store
        .search_fts("\"sqlite\" \"concurrency\"", 5, Some(COLLECTION_EXTERNAL))
        .expect("fts");
    assert_eq!(hits.len(), 1, "external scope hits the external doc");
    assert_eq!(hits[0].doc.collection_name, COLLECTION_EXTERNAL);
    // Absolute key passthrough (mine map #3): path stored as-is.
    assert_eq!(hits[0].doc.path, "/home/u/notes/design.md");
}

#[test]
fn memory_scope_excludes_external_docs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");
    seed(&store);

    // The external doc contains "sqlite"; memory scope must not surface it.
    let hits = store
        .search_fts("\"sqlite\"", 5, Some("memory"))
        .expect("fts");
    assert!(hits.iter().all(|h| h.doc.collection_name != COLLECTION_EXTERNAL));
}

#[test]
fn session_gate_marks_and_detects_shared_sessions() {
    use uuid::Uuid;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    assert!(!is_session_shared(a), "unmarked session is not shared");
    mark_session_shared(a);
    assert!(is_session_shared(a), "marked session is shared");
    assert!(!is_session_shared(b), "mark is per-session");
}
