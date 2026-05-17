//! Tests for `ToolExecutionRepository`.
//!
//! Pins the empty-`tool_name` guard: on 2026-04-28 a model emitted 44
//! `tool_use` blocks with no name field; each dispatch errored but the
//! failure still got recorded with `tool_name = ""`, producing a blank
//! row in the /usage dashboard's "Core Tools" card. The repository now
//! refuses to insert empties.

use crate::db::Database;
use crate::db::repository::ToolExecutionRepository;

async fn make_db() -> Database {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    db
}

#[tokio::test]
async fn record_skips_empty_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record("id-empty", "msg-1", "sess-1", "", "error")
        .await
        .expect("empty record returns Ok, just skips the insert");
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty(), "empty tool_name must not land in DB");
}

#[tokio::test]
async fn record_skips_whitespace_only_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record("id-ws", "msg-1", "sess-1", "   \t  ", "error")
        .await
        .expect("whitespace-only is treated as empty");
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty());
}

#[tokio::test]
async fn record_accepts_normal_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record("id-bash", "msg-1", "sess-1", "bash", "completed")
        .await
        .unwrap();
    repo.record("id-grep", "msg-2", "sess-1", "grep", "completed")
        .await
        .unwrap();
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert_eq!(stats.len(), 2);
    let names: Vec<&str> = stats.iter().map(|s| s.tool_name.as_str()).collect();
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"grep"));
}

#[tokio::test]
async fn stats_by_tool_filters_legacy_empty_rows() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    // Insert directly via the raw pool, bypassing the guard, to simulate
    // legacy pre-guard rows that already live in users' production DBs.
    db.pool()
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            conn.execute(
                "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["legacy-1", "msg-1", "sess-1", "", "error"],
            )
        })
        .await
        .unwrap()
        .unwrap();
    repo.record("id-bash", "msg-2", "sess-1", "bash", "completed")
        .await
        .unwrap();

    let stats = repo.stats_by_tool(None).await.unwrap();
    assert_eq!(
        stats.len(),
        1,
        "the legacy empty-name row must be filtered out by the SELECT"
    );
    assert_eq!(stats[0].tool_name, "bash");
}
