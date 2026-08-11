//! The memory store follows the active profile (#999).
//!
//! It used to be a single `OnceCell` that captured `opencrabs_home()` on the
//! first call and cached the Store forever. Profiles genuinely switch inside
//! one process: `cron/scheduler.rs` runs each job inside
//! `with_profile_home_async`, so a job under profile B executed with B's
//! config, keys and brain files while reading and writing profile A's
//! `memory.db`, whichever initialized first. The turn path indexes MEMORY.md,
//! so one profile's memory was written into another's index.
//!
//! Profiles are the isolation boundary here. These tests assert that the
//! component holding indexed content respects it.

use crate::config::profile::with_profile_home_async;
use crate::memory::store::get_store;

/// Two profiles must not share a store.
#[tokio::test]
async fn each_profile_gets_its_own_store() {
    let a = format!("store-a-{}", uuid::Uuid::new_v4());
    let b = format!("store-b-{}", uuid::Uuid::new_v4());

    let store_a = with_profile_home_async(Some(&a), async { get_store().map(|s| s as *const _) })
        .await
        .expect("store A opens");
    let store_b = with_profile_home_async(Some(&b), async { get_store().map(|s| s as *const _) })
        .await
        .expect("store B opens");

    assert_ne!(
        store_a, store_b,
        "two profiles resolved to the same store handle, so one is writing into the other"
    );
}

/// The same profile keeps one store, so this is not opening a fresh handle per
/// call and quietly multiplying connections.
#[tokio::test]
async fn the_same_profile_reuses_one_store() {
    let p = format!("store-same-{}", uuid::Uuid::new_v4());

    let first = with_profile_home_async(Some(&p), async { get_store().map(|s| s as *const _) })
        .await
        .expect("first open");
    let second = with_profile_home_async(Some(&p), async { get_store().map(|s| s as *const _) })
        .await
        .expect("second open");

    assert_eq!(
        first, second,
        "the same profile must reuse its store rather than opening another"
    );
}

/// Content indexed under one profile must not be visible from another.
///
/// This is the leak in its observable form: the pointer check above proves the
/// handles differ, this proves the DATA does too.
#[tokio::test]
async fn content_written_under_one_profile_is_invisible_from_another() {
    let a = format!("store-data-a-{}", uuid::Uuid::new_v4());
    let b = format!("store-data-b-{}", uuid::Uuid::new_v4());
    // Alphanumeric only: FTS5 treats a hyphen as an operator, so a UUID-shaped
    // marker would be parsed as a query rather than matched as a word.
    let marker = format!("zmarker{}", uuid::Uuid::new_v4().simple());

    // Write a document into profile A's store.
    with_profile_home_async(Some(&a), async {
        let store = get_store().expect("store A");
        let guard = store.lock().expect("lock A");
        let body = format!("# Note\n\nThis body contains {marker} exactly once.\n");
        let hash = qmd::Store::hash_content(&body);
        let now = crate::utils::string::utc_timestamp();
        guard.insert_content(&hash, &body, &now).expect("content");
        guard
            .insert_document("memory", "note.md", "Note", &hash, &now, &now)
            .expect("document");
    })
    .await;

    // Profile A can find it.
    let found_in_a = with_profile_home_async(Some(&a), async {
        let store = get_store().expect("store A");
        let guard = store.lock().expect("lock A");
        guard
            .search_fts(&marker, 10, None)
            .map(|r| r.len())
            .unwrap_or(0)
    })
    .await;
    assert_eq!(found_in_a, 1, "profile A must see its own document");

    // Profile B must not.
    let found_in_b = with_profile_home_async(Some(&b), async {
        let store = get_store().expect("store B");
        let guard = store.lock().expect("lock B");
        guard
            .search_fts(&marker, 10, None)
            .map(|r| r.len())
            .unwrap_or(0)
    })
    .await;
    assert_eq!(
        found_in_b, 0,
        "profile B saw profile A's content, the isolation boundary is broken"
    );
}
