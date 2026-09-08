//! Regression for #1457: pending requests must carry the origin forum topic.
//!
//! A `/rebuild` fired from a Telegram forum topic reported completion into
//! the chat's default topic (General) because the pending-request row had
//! nowhere to store the thread id — the channel layer parsed it and threw it
//! away at write time. The row now persists `channel_thread_id`, and the
//! rebuild tool formats it into the `telegram:chat:thread` deliver_to target
//! (grammar owned by PR #1451's `parse_telegram_target`).

use crate::db::Database;
use crate::db::repository::PendingRequestRepository;

#[tokio::test]
async fn pending_request_round_trips_origin_thread() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    let session_id = uuid::Uuid::new_v4();
    repo.insert(
        id,
        session_id,
        "rebuild from OC Dev topic",
        "telegram",
        Some("-1004428873948"),
        Some("249"),
        "user",
    )
    .await
    .unwrap();

    let row = repo
        .find_latest_for_session(session_id)
        .await
        .unwrap()
        .expect("row must exist");
    assert_eq!(row.channel_chat_id.as_deref(), Some("-1004428873948"));
    assert_eq!(row.channel_thread_id.as_deref(), Some("249"));
}

#[tokio::test]
async fn legacy_row_without_thread_reads_back_none() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    let session_id = uuid::Uuid::new_v4();
    repo.insert(
        id,
        session_id,
        "DM turn",
        "telegram",
        Some("7711740248"),
        None,
        "user",
    )
    .await
    .unwrap();

    let row = repo
        .find_latest_for_session(session_id)
        .await
        .unwrap()
        .expect("row must exist");
    assert_eq!(row.channel_thread_id, None, "non-topic turns stay NULL");
}

#[tokio::test]
async fn thread_survives_in_interrupted_scan() {
    // Boot recovery reads via get_interrupted — the thread must survive that
    // path too, or a resumed session loses its origin topic for later turns.
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let id = uuid::Uuid::new_v4();
    let session_id = uuid::Uuid::new_v4();
    repo.insert(
        id,
        session_id,
        "crashed mid-turn",
        "telegram",
        Some("-100"),
        Some("7198"),
        "user",
    )
    .await
    .unwrap();

    let rows = repo.get_interrupted().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].channel_thread_id.as_deref(), Some("7198"));
}
