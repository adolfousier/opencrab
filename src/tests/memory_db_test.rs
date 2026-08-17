//! SQLite store schema and queries.
//!
//! Moved out of `src/memory/db.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::memory::db::*;

fn temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("memory.db")).unwrap();
    (store, dir)
}

#[test]
fn hash_is_sha256_hex() {
    // Same input qmd hashed for every existing memory.db row.
    let h = Store::hash_content("hello");
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn roundtrip_document_and_fts() {
    let (store, _dir) = temp_store();
    let body = "# Title One\n\nthe quick brown fox jumps over the lazy dog";
    let hash = Store::hash_content(body);

    store.insert_content(&hash, body, "now").unwrap();
    store
        .insert_document("memory", "a.md", "Title One", &hash, "now", "now")
        .unwrap();

    let found = store.find_active_document("memory", "a.md").unwrap();
    assert_eq!(found.unwrap().1, hash);

    let hits = store.search_fts("\"fox\"", 10, None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].doc.path, "a.md");
    assert!(hits[0].score > 0.0);

    // Collection filter excludes other collections.
    let brain_hits = store.search_fts("\"fox\"", 10, Some("brain")).unwrap();
    assert!(brain_hits.is_empty());

    let doc = store.get_document("memory", "a.md").unwrap().unwrap();
    assert_eq!(doc.body.as_deref(), Some(body));
}

#[test]
fn deactivate_removes_from_fts() {
    let (store, _dir) = temp_store();
    let body = "uniqueword for deactivation test";
    let hash = Store::hash_content(body);
    store.insert_content(&hash, body, "now").unwrap();
    store
        .insert_document("memory", "b.md", "", &hash, "now", "now")
        .unwrap();
    assert_eq!(
        store.search_fts("\"uniqueword\"", 10, None).unwrap().len(),
        1
    );

    store.deactivate_document("memory", "b.md").unwrap();
    assert_eq!(
        store.search_fts("\"uniqueword\"", 10, None).unwrap().len(),
        0
    );
    assert!(
        store
            .find_active_document("memory", "b.md")
            .unwrap()
            .is_none()
    );
}

#[test]
fn embedding_roundtrip_and_backfill_list() {
    let (store, _dir) = temp_store();
    let body = "embed me";
    let hash = Store::hash_content(body);
    store.insert_content(&hash, body, "now").unwrap();
    store
        .insert_document("memory", "c.md", "", &hash, "now", "now")
        .unwrap();

    // Needs embedding: no seq-0 row yet.
    let needing = store.get_hashes_needing_embedding().unwrap();
    assert_eq!(needing.len(), 1);
    assert_eq!(needing[0].0, hash);

    store.ensure_vector_table(4).unwrap();
    store
        .insert_embedding(&hash, 0, 0, &[0.1, 0.2, 0.3, 0.4], "test-model", "now")
        .unwrap();

    assert!(store.get_hashes_needing_embedding().unwrap().is_empty());
}

#[test]
fn title_extraction() {
    assert_eq!(Store::extract_title("# Hello\nbody"), "Hello");
    assert_eq!(Store::extract_title("intro\n## Sub\nbody"), "Sub");
    assert_eq!(Store::extract_title("no heading here"), "");
}
