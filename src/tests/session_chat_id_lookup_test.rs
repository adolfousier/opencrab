//! Tests for `SessionRepository::find_by_title_suffix` and the
//! corresponding service-level `SessionService::find_session_by_title_suffix`.
//!
//! Pins the 2026-04-25 fix where a Telegram group renamed from
//! "🦀 KRAB-INCEPTION 🦀" to "🦀 HEY IOLO BUILD 🦀" produced two
//! distinct session rows because the old title-only lookup didn't see
//! the rename. Embedding the stable Telegram chat_id as a `[chat:N]`
//! suffix on the title and looking up by suffix keeps a single
//! session per underlying chat across renames.
//!
//! These tests stay below the channel handler layer — exercising the
//! repo and service helpers directly — so they're hermetic and don't
//! need a Telegram bot to run.

use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::SessionRepository;
use crate::services::{ServiceContext, SessionService};

async fn fresh_repo() -> (Database, SessionRepository) {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB connect");
    db.run_migrations().await.expect("migrations");
    let repo = SessionRepository::new(db.pool().clone());
    (db, repo)
}

async fn fresh_service() -> (Database, SessionService) {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB connect");
    db.run_migrations().await.expect("migrations");
    let ctx = ServiceContext::new(db.pool().clone());
    let svc = SessionService::new(ctx);
    // Return both so the in-memory DB stays alive for the duration of
    // the test (Database holds the only strong handle to the pool).
    (db, svc)
}

fn make_session(title: &str) -> Session {
    Session::new(Some(title.to_string()), None, None)
}

// ─── repo.find_by_title_suffix ───────────────────────────────────────

#[tokio::test]
async fn finds_session_by_suffix_after_label_rename() {
    let (_db, repo) = fresh_repo().await;
    // Simulate: chat created with one label, then renamed.
    let s = make_session("Telegram: 🦀 KRAB-INCEPTION 🦀 [chat:-5246593256]");
    repo.create(&s).await.expect("create");

    let hit = repo
        .find_by_title_suffix("[chat:-5246593256]")
        .await
        .expect("query")
        .expect("session present");
    assert_eq!(hit.id, s.id);
}

#[tokio::test]
async fn finds_session_when_label_changed_but_suffix_unchanged() {
    let (_db, repo) = fresh_repo().await;
    // The bug case in the wild: row is stored with the OLD label but
    // the chat_id suffix is stable. A new request comes in with the
    // NEW label — the suffix lookup must still resolve to the right
    // row regardless of label drift.
    let s = make_session("Telegram: 🦀 KRAB-INCEPTION 🦀 [chat:-5246593256]");
    repo.create(&s).await.expect("create");

    // Caller doesn't know the stored label, only the chat_id.
    let hit = repo
        .find_by_title_suffix("[chat:-5246593256]")
        .await
        .expect("query")
        .expect("session present despite label drift");
    assert_eq!(hit.id, s.id);
    assert!(hit.title.unwrap().contains("KRAB-INCEPTION"));
}

#[tokio::test]
async fn suffix_lookup_returns_most_recent_when_multiple_match() {
    // Defensive: if for any reason two rows ever end up with the same
    // chat_id suffix (shouldn't happen post-fix, but guards against
    // legacy data), we return the most recently updated one — that's
    // the live session, not an archived twin.
    let (_db, repo) = fresh_repo().await;
    let older = make_session("Telegram: old label [chat:-1]");
    repo.create(&older).await.expect("create older");
    // Force a strictly later updated_at on `newer` so ORDER BY
    // updated_at DESC deterministically picks it. `Session::new`
    // stamps Utc::now() on both rows, which can collide at second
    // resolution in fast tests.
    let mut newer = make_session("Telegram: new label [chat:-1]");
    newer.updated_at = older.updated_at + chrono::Duration::seconds(1);
    repo.create(&newer).await.expect("create newer");

    let hit = repo
        .find_by_title_suffix("[chat:-1]")
        .await
        .expect("query")
        .expect("hit");
    assert_eq!(hit.id, newer.id);
    assert!(hit.title.unwrap().contains("new label"));
}

#[tokio::test]
async fn suffix_lookup_excludes_archived() {
    // Archived sessions must NOT be returned — otherwise a stale row
    // a user explicitly archived would silently get reused.
    let (_db, repo) = fresh_repo().await;
    let s = make_session("Telegram: archived chat [chat:-2]");
    repo.create(&s).await.expect("create");
    repo.archive(s.id).await.expect("archive");

    let hit = repo.find_by_title_suffix("[chat:-2]").await.expect("query");
    assert!(
        hit.is_none(),
        "archived session must not match suffix lookup, got {:?}",
        hit
    );
}

#[tokio::test]
async fn suffix_lookup_misses_when_no_match() {
    let (_db, repo) = fresh_repo().await;
    let s = make_session("Telegram: only chat [chat:-3]");
    repo.create(&s).await.expect("create");

    let hit = repo
        .find_by_title_suffix("[chat:-99999]")
        .await
        .expect("query");
    assert!(hit.is_none());
}

#[tokio::test]
async fn suffix_lookup_does_not_match_when_suffix_only_appears_as_substring() {
    // The LIKE pattern is `%{suffix}` — anchored to the END of the
    // title. A session whose label happens to *contain* the suffix
    // mid-string must not match (no realistic scenario, but guards
    // against false positives if label content ever overlaps with
    // chat_id syntax).
    let (_db, repo) = fresh_repo().await;
    let s = make_session("Telegram: middle [chat:-7] tail");
    repo.create(&s).await.expect("create");

    let hit = repo.find_by_title_suffix("[chat:-7]").await.expect("query");
    assert!(
        hit.is_none(),
        "suffix not at end of title must not match, got {:?}",
        hit
    );
}

// ─── service.find_session_by_title_suffix ───────────────────────────

#[tokio::test]
async fn service_layer_delegates_to_repo() {
    let (_db, svc) = fresh_service().await;
    // Insert directly via repo so we know what to look for. The
    // service helper should find the same row.
    let pool = _db.pool().clone();
    let repo = SessionRepository::new(pool);
    let s = make_session("Telegram: 🦀 group [chat:-42]");
    repo.create(&s).await.expect("create");

    let hit = svc
        .find_session_by_title_suffix("[chat:-42]")
        .await
        .expect("svc query")
        .expect("session present via service");
    assert_eq!(hit.id, s.id);
}

#[tokio::test]
async fn service_layer_returns_none_for_unknown_suffix() {
    let (_db, svc) = fresh_service().await;
    let hit = svc
        .find_session_by_title_suffix("[chat:-nonexistent]")
        .await
        .expect("svc query");
    assert!(hit.is_none());
}
