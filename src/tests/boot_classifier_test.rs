//! Boot-classifier recovery tests (#33, owner-approved design 2026-08-29).
//!
//! The classifier promotes the #1227 wake pass from log-only to recovery:
//! a recently-active session whose topic's LAST persisted message is from a
//! user had its turn interrupted by the restart → resume; bot-last → the
//! turn completed → log only; nothing persisted → unclassifiable → log only.
//!
//! These tests pin the classification contract on an in-memory DB, mirroring
//! the row shapes the classifier depends on: bindings via
//! `SessionBindingRepository::upsert` (INNER-JOINs `sessions`, so a session
//! row is required), bot rows with sender `bot:opencrabs`, user rows with an
//! arbitrary sender id.

use crate::channels::telegram::resume::classify_recently_active;
use crate::db::models::{BOT_SENDER_ID, ChannelMessage, Session};
use crate::db::{ChannelMessageRepository, Database, SessionBindingRepository, SessionRepository};
use std::collections::HashSet;
use uuid::Uuid;

async fn test_db() -> Database {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    db
}

async fn bind_session(db: &Database, session: Uuid, chat: &str, thread: Option<i32>) {
    SessionRepository::new(db.pool().clone())
        .create(&Session {
            id: session,
            title: None,
            model: None,
            provider_name: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            token_count: 0,
            total_cost: 0.0,
            working_directory: None,
            auto_title_attempted: false,
            project_id: None,
        })
        .await
        .unwrap();
    SessionBindingRepository::new(db.pool().clone())
        .upsert(session.to_string(), "telegram", chat, thread)
        .await
        .unwrap();
}

async fn store_msg(db: &Database, chat: &str, thread: Option<&str>, sender: &str, text: &str) {
    let mut cm = ChannelMessage::new(
        "telegram".to_string(),
        chat.to_string(),
        None,
        sender.to_string(),
        "sender".to_string(),
        text.to_string(),
        "text".to_string(),
        None,
    );
    cm.thread_id = thread.map(|t| t.to_string());
    ChannelMessageRepository::new(db.pool().clone())
        .insert(&cm)
        .await
        .unwrap();
}

#[tokio::test]
async fn user_last_topic_classifies_interrupted() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100123", Some(249)).await;
    // User spoke, bot never replied — the turn was killed mid-flight.
    store_msg(&db, "-100123", Some("249"), "user:alexey", "check the logs").await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert_eq!(
        r.interrupted.len(),
        1,
        "user-last must classify interrupted"
    );
    assert_eq!(r.interrupted[0].0, sid);
    assert_eq!(r.interrupted[0].1, -100123);
    assert_eq!(r.interrupted[0].2, Some(249));
    assert!(r.completed.is_empty() && r.unclassified.is_empty());
}

#[tokio::test]
async fn bot_last_topic_classifies_completed() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100456", Some(77)).await;
    store_msg(&db, "-100456", Some("77"), "user:alexey", "status?").await;
    // Bot's own outgoing row (record_outgoing shape) after the user — turn
    // completed before the kill.
    store_msg(&db, "-100456", Some("77"), BOT_SENDER_ID, "All green.").await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert!(
        r.interrupted.is_empty(),
        "bot-last must NOT be resumed — the turn already finished"
    );
    assert_eq!(r.completed.len(), 1);
    assert_eq!(r.completed[0], sid.simple().to_string()[..8].to_owned());
}

#[tokio::test]
async fn later_user_message_flips_completed_back_to_interrupted() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100789", Some(11)).await;
    store_msg(&db, "-100789", Some("11"), "user:alexey", "q1").await;
    store_msg(&db, "-100789", Some("11"), BOT_SENDER_ID, "a1").await;
    // A NEW user message after the bot reply = a fresh turn the restart
    // killed — this is the double-kill coma case (#729 resumed turns leave
    // no pending row), and it MUST land in interrupted, not completed.
    store_msg(&db, "-100789", Some("11"), "user:alexey", "q2").await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert_eq!(r.interrupted.len(), 1, "last word = user → interrupted");
    assert!(r.completed.is_empty());
}

#[tokio::test]
async fn no_topic_messages_classifies_unclassified() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100222", Some(5)).await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert!(
        r.interrupted.is_empty(),
        "resuming blind would replay noise"
    );
    assert_eq!(r.unclassified.len(), 1);
}

#[tokio::test]
async fn general_binding_null_thread_classifies() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100333", None).await;
    // General/DM rows are stored with a NULL thread — the classifier's
    // None arm must address exactly those, not fall into some topic.
    store_msg(&db, "-100333", None, "user:alexey", "general ping").await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert_eq!(r.interrupted.len(), 1);
    assert_eq!(r.interrupted[0].2, None);
}

#[tokio::test]
async fn already_resumed_sessions_are_skipped() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100444", Some(3)).await;
    store_msg(&db, "-100444", Some("3"), "user:alexey", "mid-turn text").await;

    let mut resumed = HashSet::new();
    resumed.insert(sid);
    let r = classify_recently_active(db.pool().clone(), &resumed).await;
    assert!(r.interrupted.is_empty(), "journal already roused this one");
    assert!(r.completed.is_empty() && r.unclassified.is_empty());
}

#[tokio::test]
async fn same_second_rows_resolve_to_last_inserted() {
    let db = test_db().await;
    let sid = Uuid::new_v4();
    bind_session(&db, sid, "-100555", Some(9)).await;
    // Both rows land in the same second (test speed) — the rowid tiebreak
    // must pick the LAST inserted, i.e. the bot's completion, not the user's.
    store_msg(&db, "-100555", Some("9"), "user:alexey", "go").await;
    store_msg(&db, "-100555", Some("9"), BOT_SENDER_ID, "done").await;

    let r = classify_recently_active(db.pool().clone(), &HashSet::new()).await;
    assert!(r.interrupted.is_empty(), "rowid tiebreak → bot row wins");
    assert_eq!(r.completed.len(), 1);
}
