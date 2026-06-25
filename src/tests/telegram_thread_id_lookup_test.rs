//! Tests for `latest_thread_id_for_chat` — the proactive-path lookup that
//! resolves a Telegram chat's most recent thread_id from
//! `channel_messages` so `telegram_send` and startup-resume route into
//! the originating forum topic (issue #130 proactive path).
//!
//! The helper itself reads via `crate::db::global_pool()` which is set
//! only by file-backed `Database::connect`. Tests can't safely install
//! into that OnceLock (other tests share the process), so we cover:
//!   1. The `i32 -> ThreadId(MessageId)` parse path inline.
//!   2. The repo contract the helper depends on (round-trip, ordering).
//!   3. The graceful-None path when the global pool is absent.

use crate::channels::telegram::send::latest_thread_id_for_chat;
use crate::db::models::ChannelMessage;
use crate::db::{ChannelMessageRepository, Database};
use chrono::{Duration, Utc};
use teloxide::types::{MessageId, ThreadId};

#[test]
fn parses_numeric_thread_id_string_into_thread_id() {
    let tid_str = "42";
    let result: Option<ThreadId> = tid_str.parse::<i32>().ok().map(|n| ThreadId(MessageId(n)));
    assert_eq!(result, Some(ThreadId(MessageId(42))));
}

#[test]
fn non_numeric_thread_id_string_returns_none() {
    let tid_str = "not a number";
    let result: Option<ThreadId> = tid_str.parse::<i32>().ok().map(|n| ThreadId(MessageId(n)));
    assert_eq!(result, None);
}

#[test]
fn thread_id_overflowing_i32_returns_none() {
    // teloxide's ThreadId wraps MessageId(i32), so values outside i32
    // range can't be represented. The helper must return None rather
    // than panic on overflow.
    let tid_str = "9999999999999";
    let result: Option<ThreadId> = tid_str.parse::<i32>().ok().map(|n| ThreadId(MessageId(n)));
    assert_eq!(result, None);
}

#[tokio::test]
async fn channel_message_thread_id_round_trips_through_repo() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());

    let chat_id = "test-chat-thread-roundtrip";
    let msg = ChannelMessage::new(
        "telegram".into(),
        chat_id.into(),
        Some("Some Group".into()),
        "u1".into(),
        "alice".into(),
        "hello from topic 17".into(),
        "text".into(),
        Some("msg-1".into()),
    )
    .with_thread(Some("17".to_string()), Some("General".into()));

    repo.insert(&msg).await.expect("insert");
    let recent = repo
        .recent(Some("telegram"), chat_id, 1, None)
        .await
        .expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].thread_id.as_deref(), Some("17"));
}

/// #226: forum-topic isolation. `recent()` with a thread_id must return ONLY
/// that topic's messages — the handler was passing `None`, so every topic saw
/// every other topic's history. Two topics in the same chat must not bleed.
#[tokio::test]
async fn recent_scoped_to_thread_does_not_bleed_across_topics() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());

    let chat_id = "test-chat-topic-isolation";
    let mk = |body: &str, mid: &str, thread: &str| {
        ChannelMessage::new(
            "telegram".into(),
            chat_id.into(),
            None,
            "u1".into(),
            "alice".into(),
            body.into(),
            "text".into(),
            Some(mid.into()),
        )
        .with_thread(Some(thread.to_string()), None)
    };

    repo.insert(&mk("topic ten message", "m-10", "10"))
        .await
        .unwrap();
    repo.insert(&mk("topic twenty message", "m-20", "20"))
        .await
        .unwrap();

    // Scoped to topic 10 → only topic-10 content, never topic 20.
    let only_10 = repo
        .recent(Some("telegram"), chat_id, 30, Some("10"))
        .await
        .unwrap();
    assert_eq!(
        only_10.len(),
        1,
        "topic 10 must see exactly its own message"
    );
    assert_eq!(only_10[0].thread_id.as_deref(), Some("10"));
    assert!(only_10.iter().all(|m| !m.content.contains("twenty")));

    // Unscoped (None) returns both — kept for the non-forum group case where
    // there's a single shared conversation.
    let all = repo
        .recent(Some("telegram"), chat_id, 30, None)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn recent_returns_newest_first_so_helper_picks_latest_thread() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = ChannelMessageRepository::new(db.pool().clone());

    let chat_id = "test-chat-recent-order";
    let mut old = ChannelMessage::new(
        "telegram".into(),
        chat_id.into(),
        None,
        "u1".into(),
        "alice".into(),
        "older".into(),
        "text".into(),
        Some("msg-old".into()),
    )
    .with_thread(Some("100".to_string()), None);
    old.created_at = Utc::now() - Duration::hours(1);

    let new = ChannelMessage::new(
        "telegram".into(),
        chat_id.into(),
        None,
        "u1".into(),
        "alice".into(),
        "newer".into(),
        "text".into(),
        Some("msg-new".into()),
    )
    .with_thread(Some("200".to_string()), None);

    repo.insert(&old).await.expect("insert old");
    repo.insert(&new).await.expect("insert new");

    let recent = repo
        .recent(Some("telegram"), chat_id, 1, None)
        .await
        .expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].thread_id.as_deref(), Some("200"));
}

#[tokio::test]
async fn latest_thread_id_returns_none_for_missing_chat_or_uninit_pool() {
    // Helper hits global_pool(); when uninitialized in the test process
    // it returns None. When initialized, a chat with no stored messages
    // also returns None. Either way: never panic, never invent a thread.
    let result = latest_thread_id_for_chat(99_999_999_999).await;
    assert_eq!(result, None);
}
