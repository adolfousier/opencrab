//! Integration tests for the analytics emitters (#901).
//!
//! Unlike `analytics_db_test.rs` (which inserts via raw SQL and asserts on
//! stats aggregates), these tests drive the ACTUAL production write paths the
//! emitters call — `ToolExecutionRepository::record` and the
//! `AnalyticsEventRepository::record_*` / `resolve_*` methods — and assert on
//! the raw row values those paths produce.

use crate::db::Database;
use crate::db::Pool;
use crate::db::repository::{AnalyticsEventRepository, ToolExecutionRepository};

/// Helper: in-memory DB with all migrations applied.
async fn test_pool() -> Pool {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB should connect");
    db.run_migrations().await.expect("migrations should apply");
    db.pool().clone()
}

/// Query a single tool_executions row's analytics columns.
async fn tool_exec_row(pool: &Pool, id: &str) -> (Option<String>, Option<String>, Option<i64>) {
    let id = id.to_string();
    let conn = pool.get().await.expect("should get connection");
    conn.interact(move |conn| {
        conn.query_row(
            "SELECT provider, model, duration_ms FROM tool_executions WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
    })
    .await
    .expect("interact should succeed")
    .expect("query should succeed")
}

// ─── tool_executions: record() carries provider/model/duration ────────

#[tokio::test]
async fn record_writes_provider_model_duration() {
    let pool = test_pool().await;
    let repo = ToolExecutionRepository::new(pool.clone());

    repo.record(
        "te-emitter-1",
        "msg-1",
        "sess-1",
        "bash",
        "success",
        Some("anthropic"),
        Some("claude-opus-4-8"),
        Some(250),
    )
    .await
    .expect("record should succeed");

    let row = tool_exec_row(&pool, "te-emitter-1").await;
    assert_eq!(row.0.as_deref(), Some("anthropic"));
    assert_eq!(row.1.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(row.2, Some(250));
}

#[tokio::test]
async fn record_writes_nulls_when_context_absent() {
    let pool = test_pool().await;
    let repo = ToolExecutionRepository::new(pool.clone());

    // Parallel-batch sites pass None for all three (no per-tool context).
    repo.record(
        "te-emitter-2",
        "msg-2",
        "sess-2",
        "grep",
        "error",
        None,
        None,
        None,
    )
    .await
    .expect("record should succeed");

    let row = tool_exec_row(&pool, "te-emitter-2").await;
    assert_eq!(row.0, None);
    assert_eq!(row.1, None);
    assert_eq!(row.2, None);
}

// ─── phantom_events: detection row, then resolution mutation ──────────

#[tokio::test]
async fn phantom_detection_row_then_resolution() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    repo.record_phantom(
        "ph-1",
        "sess-ph",
        Some("modelstudio"),
        Some("qwen3.8-max-preview"),
    )
    .await
    .expect("record_phantom should succeed");

    // Freshly detected: unresolved, zeroed retry counters.
    {
        let conn = pool.get().await.expect("should get connection");
        let (resolved, retry, tools, provider, model) = conn
            .interact(|conn| {
                conn.query_row(
                    "SELECT resolved, retry_count, tools_after_retry, provider, model \
                     FROM phantom_events WHERE id = ?1",
                    rusqlite::params!["ph-1"],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
            })
            .await
            .expect("interact should succeed")
            .expect("query should succeed");
        assert_eq!(resolved, 0);
        assert_eq!(retry, 0);
        assert_eq!(tools, 0);
        assert_eq!(provider.as_deref(), Some("modelstudio"));
        assert_eq!(model.as_deref(), Some("qwen3.8-max-preview"));
    }

    // Recovery: resolve_phantom flips resolved and stamps the retry counters.
    repo.resolve_phantom("sess-ph", 2, 17)
        .await
        .expect("resolve_phantom should succeed");

    let conn = pool.get().await.expect("should get connection");
    let (resolved, retry, tools) = conn
        .interact(|conn| {
            conn.query_row(
                "SELECT resolved, retry_count, tools_after_retry FROM phantom_events WHERE id = ?1",
                rusqlite::params!["ph-1"],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
        })
        .await
        .expect("interact should succeed")
        .expect("query should succeed");
    assert_eq!(resolved, 1);
    assert_eq!(retry, 2);
    assert_eq!(tools, 17);
}

// ─── streaming_recoveries: recovery row carries tool_count ────────────

#[tokio::test]
async fn streaming_recovery_row_carries_tool_count() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    repo.record_streaming_recovery(
        "sr-1",
        "sess-sr",
        Some("anthropic"),
        Some("claude-opus-4-8"),
        4,
    )
    .await
    .expect("record_streaming_recovery should succeed");

    let conn = pool.get().await.expect("should get connection");
    let (tool_count, provider, model) = conn
        .interact(|conn| {
            conn.query_row(
                "SELECT tool_count, provider, model FROM streaming_recoveries WHERE id = ?1",
                rusqlite::params!["sr-1"],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
        })
        .await
        .expect("interact should succeed")
        .expect("query should succeed");
    assert_eq!(tool_count, 4);
    assert_eq!(provider.as_deref(), Some("anthropic"));
    assert_eq!(model.as_deref(), Some("claude-opus-4-8"));
}

// ─── brain_verify_events: one row per event_type ──────────────────────

#[tokio::test]
async fn brain_verify_row_per_event_type() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    repo.record_brain_verify("bv-pass", "SOUL.md", "pass", None)
        .await
        .expect("pass event should succeed");
    repo.record_brain_verify(
        "bv-rollback",
        "MEMORY.md",
        "rollback",
        Some("missing owns header"),
    )
    .await
    .expect("rollback event should succeed");
    repo.record_brain_verify(
        "bv-failclosed",
        "AGENTS.md",
        "fail_closed",
        Some("hard rule violated"),
    )
    .await
    .expect("fail_closed event should succeed");

    let conn = pool.get().await.expect("should get connection");
    let rows = conn
        .interact(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, file_name, event_type, violations FROM brain_verify_events ORDER BY id")
                .expect("prepare should succeed");
            let out = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .expect("query_map should succeed")
                .collect::<Result<Vec<_>, _>>()
                .expect("rows should collect");
            Ok::<_, rusqlite::Error>(out)
        })
        .await
        .expect("interact should succeed")
        .expect("query should succeed");

    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0],
        (
            "bv-failclosed".into(),
            "AGENTS.md".into(),
            "fail_closed".into(),
            Some("hard rule violated".into())
        )
    );
    assert_eq!(
        rows[1],
        ("bv-pass".into(), "SOUL.md".into(), "pass".into(), None)
    );
    assert_eq!(
        rows[2],
        (
            "bv-rollback".into(),
            "MEMORY.md".into(),
            "rollback".into(),
            Some("missing owns header".into())
        )
    );
}

// ─── emit_* helpers: safe no-op without a global pool ─────────────────

#[test]
fn emit_helpers_noop_without_global_pool() {
    // No global pool is configured in the test binary, so every emit_*
    // helper must return early without panicking.
    AnalyticsEventRepository::emit_phantom("sess-x", Some("p"), Some("m"));
    AnalyticsEventRepository::emit_resolve_phantom("sess-x", 1, 5);
    AnalyticsEventRepository::emit_streaming_recovery("sess-x", Some("p"), Some("m"), 3);
    AnalyticsEventRepository::emit_brain_verify("SOUL.md", "pass", None);
}
