//! Tests for `record_outgoing` (#1085 P1b R2) — the shared persistence
//! that replaced the byte-identical `ChannelMessage` builders in cron's
//! `deliver_telegram` and the telegram_send tool's `persist_outgoing`.
//!
//! Pins the row shape the reply-recovery lookup depends on (#234):
//! channel, chat id, message id key, thread stamping, and the
//! empty-content skip. The Q4 plain-text fallback itself is inherited
//! by construction (the outbox ladder IS `send_html_or_plain`), so it
//! has no separate unit surface without a live bot — this file pins the
//! persistence half that IS unit-testable.

use crate::channels::telegram::send::record_outgoing;
use crate::db::{ChannelMessageRepository, Database};
use teloxide::types::{MessageId, ThreadId};

#[tokio::test]
async fn empty_sent_list_writes_nothing() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());
    record_outgoing(Some(db.pool().clone()), -100123, None, &[]).await;
    // No rows, no panic — the early return held.
    let rows = repo
        .recent(Some("telegram"), "-100123", 10, None, None)
        .await;
    assert!(rows.unwrap().is_empty());
}

#[tokio::test]
async fn stamps_thread_and_message_id_for_reply_recovery() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());
    let chat = -1004428873948i64;
    let thread = ThreadId(MessageId(249));

    record_outgoing(
        Some(db.pool().clone()),
        chat,
        Some(thread),
        &[(4242, "cron body".to_string())],
    )
    .await;

    let rows = repo
        .recent(Some("telegram"), &chat.to_string(), 10, None, None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    // Reply recovery keys on exactly these two fields.
    assert_eq!(row.platform_message_id.as_deref(), Some("4242"));
    assert_eq!(row.thread_id.as_deref(), Some("249"));
    assert_eq!(row.channel, "telegram");
    assert_eq!(row.channel_chat_id, chat.to_string());
    assert_eq!(row.content, "cron body");
}

#[tokio::test]
async fn skips_blank_chunks_and_persists_the_rest() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());
    let chat = 100777i64;

    record_outgoing(
        Some(db.pool().clone()),
        chat,
        None,
        &[
            (1, "part one".to_string()),
            (2, "   \n  ".to_string()), // whitespace-only chunk: skipped
            (3, "part two".to_string()),
        ],
    )
    .await;

    let rows = repo
        .recent(Some("telegram"), &chat.to_string(), 10, None, None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "blank chunk must not produce a row");
    assert!(
        rows.iter()
            .any(|r| r.platform_message_id.as_deref() == Some("1"))
    );
    assert!(
        rows.iter()
            .any(|r| r.platform_message_id.as_deref() == Some("3"))
    );
}

#[tokio::test]
async fn falls_back_to_global_pool_when_none_passed() {
    // The tool path passes None and relies on the global pool; when the
    // global pool is ALSO absent (early startup, unit tests), the
    // function must no-op with a warning instead of panicking. We can't
    // install a global pool here (process-wide OnceLock), so the absent
    // case is what this exercises.
    record_outgoing(None, 111, None, &[(9, "x".to_string())]).await;
    // Reaching here without panic is the contract.
}
