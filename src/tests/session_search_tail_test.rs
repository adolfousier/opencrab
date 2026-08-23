//! Regression tests for #1166 — the `tail` operation on session_search.
//!
//! Covers: newest-session default (chronological last-N), explicit session
//! number selection, limit clamping, 'all' rejection, and empty sessions.

use crate::brain::tools::session_search::SessionSearchTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::db::Database;
use crate::db::models::{Message, Session};
use crate::db::repository::{MessageRepository, SessionRepository};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

async fn setup() -> (Database, SessionSearchTool) {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let tool = SessionSearchTool::new(db.pool().clone());
    (db, tool)
}

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

/// Seed `count` messages into a session, then force its updated_at.
///
/// The re-force matters: every message insert bumps the parent session's
/// updated_at to wall-clock now (message.rs:126), which would otherwise
/// destroy deterministic most-recent-first ordering across sessions.
async fn seed_session(
    repo: &SessionRepository,
    msg_repo: &MessageRepository,
    title: &str,
    updated_at: chrono::DateTime<Utc>,
    count: i32,
) -> Session {
    let mut session = Session::new(Some(title.to_string()), Some("m".to_string()), None);
    repo.create(&session).await.unwrap();
    for i in 1..=count {
        msg_repo
            .create(&Message::new(
                session.id,
                "user".into(),
                format!("{}-msg{}", title, i),
                i,
            ))
            .await
            .unwrap();
    }
    // After the bump storm: pin the final ordering timestamp.
    let mut final_session = session.clone();
    final_session.updated_at = updated_at;
    repo.update(&final_session).await.unwrap();
    session
}

#[tokio::test]
async fn tail_newest_session_returns_chronological_last_n() {
    let (db, tool) = setup().await;
    let srepo = SessionRepository::new(db.pool().clone());
    let mrepo = MessageRepository::new(db.pool().clone());

    // Older session first, then a newer one: "no filter" must pick NEWER.
    seed_session(
        &srepo,
        &mrepo,
        "old",
        Utc::now() - chrono::Duration::hours(2),
        3,
    )
    .await;
    seed_session(&srepo, &mrepo, "newer", Utc::now(), 3).await;

    let result = tool
        .execute(json!({"operation": "tail", "n": 2}), &ctx())
        .await
        .unwrap();
    assert!(result.success, "error: {:?}", result.error);
    assert!(
        result.output.contains("newer"),
        "should tail the newest session, got: {}",
        result.output
    );
    // Chronological: msg2 appears before msg3; msg1 (outside the tail) absent.
    let m2 = result.output.find("newer-msg2").expect("msg2 in output");
    let m3 = result.output.find("newer-msg3").expect("msg3 in output");
    assert!(m2 < m3, "tail must be oldest-first within the window");
    assert!(!result.output.contains("newer-msg1"));
}

#[tokio::test]
async fn tail_explicit_session_number_selects_that_session() {
    let (db, tool) = setup().await;
    let srepo = SessionRepository::new(db.pool().clone());
    let mrepo = MessageRepository::new(db.pool().clone());

    seed_session(
        &srepo,
        &mrepo,
        "old",
        Utc::now() - chrono::Duration::hours(2),
        1,
    )
    .await;
    seed_session(&srepo, &mrepo, "newer", Utc::now(), 1).await;

    // "2" = second-most-recent = "old".
    let result = tool
        .execute(json!({"operation": "tail", "session": "2"}), &ctx())
        .await
        .unwrap();
    assert!(result.success, "error: {:?}", result.error);
    assert!(
        result.output.contains("old-msg1"),
        "should tail session #2 (old), got: {}",
        result.output
    );
    assert!(!result.output.contains("newer-msg1"));
}

#[tokio::test]
async fn tail_limit_clamped_to_tool_maximum() {
    let (db, tool) = setup().await;
    let srepo = SessionRepository::new(db.pool().clone());
    let mrepo = MessageRepository::new(db.pool().clone());

    let session = seed_session(&srepo, &mrepo, "big", Utc::now(), 5).await;

    // Absurd n is clamped to 100 — here it just means all 5 come back.
    let big = tool
        .execute(json!({"operation": "tail", "n": 500}), &ctx())
        .await
        .unwrap();
    assert!(big.success);
    for i in 1..=5 {
        assert!(big.output.contains(&format!("big-msg{}", i)));
    }

    // Small n returns exactly n messages.
    let small = tool
        .execute(
            json!({"operation": "tail", "session": session.id.to_string(), "n": 2}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(small.success);
    assert!(!small.output.contains("big-msg3"), "only last 2 expected");
    assert_eq!(small.output.matches("big-msg").count(), 2);

    // n below the floor is raised to 1 rather than zero/underflowing.
    let floor = tool
        .execute(json!({"operation": "tail", "n": 0}), &ctx())
        .await
        .unwrap();
    assert!(floor.success);
    assert_eq!(floor.output.matches("big-msg").count(), 1);
}

#[tokio::test]
async fn tail_rejects_all_filter_with_guidance() {
    let (_db, tool) = setup().await;

    let result = tool
        .execute(json!({"operation": "tail", "session": "all"}), &ctx())
        .await
        .unwrap();
    assert!(!result.success, "'all' makes no sense for tail");
    let err = result.error.as_deref().unwrap_or(&result.output);
    assert!(err.contains("single session"), "guidance expected: {}", err);
}

#[tokio::test]
async fn tail_empty_session_reports_no_messages() {
    let (db, tool) = setup().await;
    let srepo = SessionRepository::new(db.pool().clone());

    let mut empty = Session::new(Some("hollow".to_string()), Some("m".to_string()), None);
    empty.updated_at = Utc::now();
    srepo.create(&empty).await.unwrap();

    let result = tool
        .execute(json!({"operation": "tail"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    assert!(
        result.output.contains("no messages"),
        "got: {}",
        result.output
    );
}
