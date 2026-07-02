//! Regression for #268: pending requests from long-running turns must be
//! resumed after a restart, no matter how long ago the turn STARTED.
//!
//! The old implementation deleted every row older than 10 minutes before
//! resuming, which purged exactly the turns that most need resuming (long
//! agentic runs): an interrupted 28-minute CLI coding turn was dropped while
//! a 5-minute-old turn on another provider resumed. The only age guard now
//! is a 24h crash-debris cap keyed on updated_at (last interaction), which
//! `touch()` refreshes as mid-turn progress persists.

use crate::db::Database;
use crate::db::repository::PendingRequestRepository;
use rusqlite::params;

async fn backdate(db: &Database, id: uuid::Uuid, created_secs_ago: i64, updated_secs_ago: i64) {
    let id_s = id.to_string();
    db.pool()
        .get()
        .await
        .unwrap()
        .interact(move |conn| {
            conn.execute(
                "UPDATE pending_requests SET created_at = unixepoch() - ?1, \
                 updated_at = unixepoch() - ?2 WHERE id = ?3",
                params![created_secs_ago, updated_secs_ago, id_s],
            )
        })
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn thirty_minute_old_turn_is_still_resumed() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    let session_id = uuid::Uuid::new_v4();
    repo.insert(id, session_id, "long agentic turn", "tui", None)
        .await
        .unwrap();
    // 30 minutes into the turn: well past the old 10-minute cutoff.
    backdate(&db, id, 1800, 1800).await;

    let interrupted = repo.get_interrupted().await.unwrap();
    assert_eq!(
        interrupted.len(),
        1,
        "a 30-minute-old in-flight turn must resume, not be purged"
    );
    assert_eq!(interrupted[0].session_id, session_id.to_string());
}

#[tokio::test]
async fn day_old_debris_is_purged() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    repo.insert(id, uuid::Uuid::new_v4(), "crash debris", "tui", None)
        .await
        .unwrap();
    // Last interaction over a day ago.
    backdate(&db, id, 100_000, 100_000).await;

    let interrupted = repo.get_interrupted().await.unwrap();
    assert!(
        interrupted.is_empty(),
        "rows idle for over 24h are crash debris and must be cleared"
    );
}

#[tokio::test]
async fn touch_keeps_a_very_long_turn_alive() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    repo.insert(id, uuid::Uuid::new_v4(), "multi-hour turn", "tui", None)
        .await
        .unwrap();
    // Turn STARTED two days ago but progress persisted recently.
    backdate(&db, id, 200_000, 200_000).await;
    repo.touch(id).await.unwrap();

    let interrupted = repo.get_interrupted().await.unwrap();
    assert_eq!(
        interrupted.len(),
        1,
        "a touched (recently active) turn must survive the debris cap"
    );
}

#[tokio::test]
async fn channel_scoped_query_has_same_age_semantics() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let old_turn = uuid::Uuid::new_v4();
    repo.insert(
        old_turn,
        uuid::Uuid::new_v4(),
        "old telegram turn",
        "telegram",
        Some("123"),
    )
    .await
    .unwrap();
    backdate(&db, old_turn, 1800, 1800).await;

    let debris = uuid::Uuid::new_v4();
    repo.insert(
        debris,
        uuid::Uuid::new_v4(),
        "telegram debris",
        "telegram",
        Some("123"),
    )
    .await
    .unwrap();
    backdate(&db, debris, 100_000, 100_000).await;

    let rows = repo.get_interrupted_for_channel("telegram").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, old_turn.to_string());
}
