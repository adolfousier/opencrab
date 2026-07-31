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
    repo.record("id-empty", "msg-1", "sess-1", "", "error", None, None, None)
        .await
        .expect("empty record returns Ok, just skips the insert");
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty(), "empty tool_name must not land in DB");
}

#[tokio::test]
async fn record_skips_whitespace_only_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record(
        "id-ws", "msg-1", "sess-1", "   \t  ", "error", None, None, None,
    )
    .await
    .expect("whitespace-only is treated as empty");
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty());
}

#[tokio::test]
async fn record_accepts_normal_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record(
        "id-bash",
        "msg-1",
        "sess-1",
        "bash",
        "completed",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    repo.record(
        "id-grep",
        "msg-2",
        "sess-1",
        "grep",
        "completed",
        None,
        None,
        None,
    )
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
    repo.record(
        "id-bash",
        "msg-2",
        "sess-1",
        "bash",
        "completed",
        None,
        None,
        None,
    )
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

// #687: garbage tool names from phantom tool calls must be rejected.
#[tokio::test]
async fn record_skips_garbage_tool_name_with_xml_fragment() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    let garbage = "i_apologize... i'll_call_write_opencrabs_file...";
    repo.record(
        "id-garbage",
        "msg-1",
        "sess-1",
        garbage,
        "error",
        None,
        None,
        None,
    )
    .await
    .expect("garbage record returns Ok, just skips the insert");
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty(), "garbage tool_name must not land in DB");
}

#[tokio::test]
async fn record_skips_tool_name_with_uppercase() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record(
        "id-upper", "msg-1", "sess-1", "Bash", "error", None, None, None,
    )
    .await
    .unwrap();
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty(), "uppercase tool_name must be rejected");
}

#[tokio::test]
async fn record_skips_overlong_tool_name() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    let long_name = "a".repeat(65);
    repo.record(
        "id-long", "msg-1", "sess-1", &long_name, "error", None, None, None,
    )
    .await
    .unwrap();
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert!(stats.is_empty(), "tool_name >64 chars must be rejected");
}

#[tokio::test]
async fn record_accepts_tool_name_with_digits_and_underscores() {
    let db = make_db().await;
    let repo = ToolExecutionRepository::new(db.pool().clone());
    repo.record(
        "id-ok",
        "msg-1",
        "sess-1",
        "web_search_v2",
        "success",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let stats = repo.stats_by_tool(None).await.unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].tool_name, "web_search_v2");
}
