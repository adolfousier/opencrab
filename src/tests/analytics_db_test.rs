//! Tests for the analytics event DB layer (#898).
//!
//! Covers: phantom_events, streaming_recoveries, brain_verify_events
//! insert/query round-trips, and tool_executions nullable columns.

use crate::db::Database;
use crate::db::repository::AnalyticsEventRepository;

/// Helper: create an in-memory DB with all migrations applied.
async fn test_repo() -> AnalyticsEventRepository {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB should connect");
    db.run_migrations().await.expect("migrations should apply");
    AnalyticsEventRepository::new(db.pool().clone())
}

// ─── Phantom Events ───────────────────────────────────────────────────

#[tokio::test]
async fn phantom_record_and_stats_round_trip() {
    let repo = test_repo().await;

    repo.record_phantom(
        "phantom-1",
        "session-abc",
        Some("anthropic"),
        Some("claude-opus-4-8"),
    )
    .await
    .expect("record should succeed");

    let stats = repo
        .phantom_stats(None)
        .await
        .expect("stats should succeed");
    assert_eq!(stats.total, 1);
    assert_eq!(stats.resolved, 0);
    assert_eq!(stats.by_model.len(), 1);
    assert_eq!(stats.by_model[0].0, "claude-opus-4-8");
    assert_eq!(stats.by_model[0].1, 1); // total
    assert_eq!(stats.by_model[0].2, 0); // resolved
}

#[tokio::test]
async fn phantom_resolve_updates_stats() {
    let repo = test_repo().await;

    repo.record_phantom("phantom-2", "session-abc", None, None)
        .await
        .expect("record should succeed");

    repo.resolve_phantom("session-abc", 3, 22)
        .await
        .expect("resolve should succeed");

    let stats = repo
        .phantom_stats(None)
        .await
        .expect("stats should succeed");
    assert_eq!(stats.total, 1);
    assert_eq!(stats.resolved, 1);
}

#[tokio::test]
async fn phantom_null_provider_model_grouped_as_unknown() {
    let repo = test_repo().await;

    repo.record_phantom("phantom-3", "session-xyz", None, None)
        .await
        .expect("record should succeed");

    let stats = repo
        .phantom_stats(None)
        .await
        .expect("stats should succeed");
    assert_eq!(stats.by_model.len(), 1);
    assert_eq!(stats.by_model[0].0, "unknown"); // COALESCE(model, 'unknown')
}

#[tokio::test]
async fn phantom_time_filter() {
    let repo = test_repo().await;

    // Record two events (detected_at defaults to unixepoch())
    repo.record_phantom("p-1", "s1", None, None).await.unwrap();
    repo.record_phantom("p-2", "s1", None, None).await.unwrap();

    // All events visible with no filter
    let stats = repo.phantom_stats(None).await.unwrap();
    assert_eq!(stats.total, 2);

    // Future epoch filters everything out
    let stats = repo.phantom_stats(Some(9999999999)).await.unwrap();
    assert_eq!(stats.total, 0);

    // Past epoch includes everything
    let stats = repo.phantom_stats(Some(1)).await.unwrap();
    assert_eq!(stats.total, 2);
}

#[tokio::test]
async fn phantom_multiple_models_breakdown() {
    let repo = test_repo().await;

    repo.record_phantom("p-a", "s1", Some("anthropic"), Some("claude-opus-4-8"))
        .await
        .unwrap();
    repo.record_phantom("p-b", "s1", Some("modelstudio"), Some("qwen3.8-max"))
        .await
        .unwrap();
    repo.record_phantom("p-c", "s1", Some("anthropic"), Some("claude-opus-4-8"))
        .await
        .unwrap();

    let stats = repo.phantom_stats(None).await.unwrap();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.by_model.len(), 2);
    // Ordered by COUNT(*) DESC: claude-opus-4-8 has 2, qwen has 1
    assert_eq!(stats.by_model[0].0, "claude-opus-4-8");
    assert_eq!(stats.by_model[0].1, 2);
    assert_eq!(stats.by_model[1].0, "qwen3.8-max");
    assert_eq!(stats.by_model[1].1, 1);
}

// ─── Streaming Recoveries ─────────────────────────────────────────────

#[tokio::test]
async fn streaming_recovery_record_and_stats() {
    let repo = test_repo().await;

    repo.record_streaming_recovery(
        "recovery-1",
        "session-abc",
        Some("modelstudio"),
        Some("qwen3.8-max-preview"),
        3,
    )
    .await
    .expect("record should succeed");

    let stats = repo
        .streaming_stats(None)
        .await
        .expect("stats should succeed");
    assert_eq!(stats.total, 1);
    assert_eq!(stats.total_tools, 3);
    assert_eq!(stats.by_model.len(), 1);
    assert_eq!(stats.by_model[0].0, "qwen3.8-max-preview");
    assert_eq!(stats.by_model[0].1, 1);
}

#[tokio::test]
async fn streaming_recovery_null_provider() {
    let repo = test_repo().await;

    repo.record_streaming_recovery("recovery-2", "s1", None, None, 1)
        .await
        .unwrap();

    let stats = repo.streaming_stats(None).await.unwrap();
    assert_eq!(stats.total, 1);
    assert_eq!(stats.by_model[0].0, "unknown");
}

#[tokio::test]
async fn streaming_recovery_time_filter() {
    let repo = test_repo().await;

    repo.record_streaming_recovery("r-1", "s1", None, None, 2)
        .await
        .unwrap();

    // Future epoch filters out
    let stats = repo.streaming_stats(Some(9999999999)).await.unwrap();
    assert_eq!(stats.total, 0);

    // Past epoch includes
    let stats = repo.streaming_stats(Some(1)).await.unwrap();
    assert_eq!(stats.total, 1);
    assert_eq!(stats.total_tools, 2);
}

// ─── Brain Verify Events ──────────────────────────────────────────────

#[tokio::test]
async fn brain_verify_record_and_stats() {
    let repo = test_repo().await;

    repo.record_brain_verify(
        "verify-1",
        "SOUL.md",
        "rollback",
        Some("contradiction pair A+B matched in entry 3"),
    )
    .await
    .expect("record should succeed");

    let stats = repo
        .brain_verify_stats(None)
        .await
        .expect("stats should succeed");
    assert_eq!(stats.passes, 0);
    assert_eq!(stats.rollbacks, 1);
    assert_eq!(stats.fail_closed, 0);
}

#[tokio::test]
async fn brain_verify_all_event_types() {
    let repo = test_repo().await;

    repo.record_brain_verify("v-pass", "AGENTS.md", "pass", None)
        .await
        .unwrap();
    repo.record_brain_verify("v-rollback", "SOUL.md", "rollback", Some("contradiction"))
        .await
        .unwrap();
    repo.record_brain_verify("v-fail", "MEMORY.md", "fail_closed", Some("TOML absent"))
        .await
        .unwrap();

    let stats = repo.brain_verify_stats(None).await.unwrap();
    assert_eq!(stats.passes, 1);
    assert_eq!(stats.rollbacks, 1);
    assert_eq!(stats.fail_closed, 1);
}

#[tokio::test]
async fn brain_verify_time_filter() {
    let repo = test_repo().await;

    repo.record_brain_verify("v-1", "SOUL.md", "pass", None)
        .await
        .unwrap();

    // Future epoch filters out
    let stats = repo.brain_verify_stats(Some(9999999999)).await.unwrap();
    assert_eq!(stats.passes, 0);

    // Past epoch includes
    let stats = repo.brain_verify_stats(Some(1)).await.unwrap();
    assert_eq!(stats.passes, 1);
}

// ─── Tool Executions Nullable Columns ─────────────────────────────────

#[tokio::test]
async fn tool_executions_nullable_columns_accept_null() {
    let repo = test_repo().await;
    let pool = repo.pool();

    let conn = pool.get().await.expect("should get connection");

    // Insert a tool execution WITHOUT provider/model/duration (pre-migration style)
    conn.interact(|conn| {
        conn.execute(
            "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["te-1", "msg-1", "sess-1", "bash", "success", 1722400000i64],
        )
    })
    .await
    .expect("interact should succeed")
    .expect("insert should succeed");

    // Query it back - nullable columns should be NULL
    let row = conn
        .interact(|conn| {
            conn.query_row(
                "SELECT provider, model, duration_ms FROM tool_executions WHERE id = ?1",
                rusqlite::params!["te-1"],
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
        .expect("query should succeed");

    assert_eq!(row.0, None); // provider is NULL
    assert_eq!(row.1, None); // model is NULL
    assert_eq!(row.2, None); // duration_ms is NULL
}

#[tokio::test]
async fn tool_executions_with_provider_model_duration() {
    let repo = test_repo().await;
    let pool = repo.pool();

    let conn = pool.get().await.expect("should get connection");

    // Insert a tool execution WITH provider/model/duration (new style)
    conn.interact(|conn| {
        conn.execute(
            "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, provider, model, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                "te-2", "msg-2", "sess-2", "read_file", "success", 1722400000i64,
                "anthropic", "claude-opus-4-8", 150i64
            ],
        )
    })
    .await
    .expect("interact should succeed")
    .expect("insert should succeed");

    let row = conn
        .interact(|conn| {
            conn.query_row(
                "SELECT provider, model, duration_ms FROM tool_executions WHERE id = ?1",
                rusqlite::params!["te-2"],
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
        .expect("query should succeed");

    assert_eq!(row.0.as_deref(), Some("anthropic"));
    assert_eq!(row.1.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(row.2, Some(150));
}

// ─── Per-Model Tool Stats ─────────────────────────────────────────────

#[tokio::test]
async fn tool_stats_by_model_groups_correctly() {
    let repo = test_repo().await;
    let pool = repo.pool();

    {
        let conn = pool.get().await.expect("should get connection");

        // Insert tool executions with different models
        conn.interact(|conn| {
            conn.execute_batch(
                "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, model)
                 VALUES ('t1', 'm1', 's1', 'bash', 'success', 1722400000, 'claude-opus-4-8');
                 INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, model)
                 VALUES ('t2', 'm1', 's1', 'read_file', 'error', 1722400001, 'claude-opus-4-8');
                 INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, model)
                 VALUES ('t3', 'm1', 's1', 'grep', 'success', 1722400002, 'qwen3.8-max');
                 INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, model)
                 VALUES ('t4', 'm1', 's1', 'bash', 'error', 1722400003, 'qwen3.8-max');
                 INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, model)
                 VALUES ('t5', 'm1', 's1', 'ls', 'error', 1722400004, 'qwen3.8-max');",
            )
        })
        .await
        .expect("interact should succeed")
        .expect("insert should succeed");
    } // conn dropped here, pool connection released

    let stats = repo.tool_stats_by_model(None).await.unwrap();
    assert_eq!(stats.len(), 2);

    // qwen has 3 calls (2 errors), claude has 2 calls (1 error)
    // Ordered by COUNT(*) DESC
    assert_eq!(stats[0].model, "qwen3.8-max");
    assert_eq!(stats[0].total, 3);
    assert_eq!(stats[0].failures, 2);

    assert_eq!(stats[1].model, "claude-opus-4-8");
    assert_eq!(stats[1].total, 2);
    assert_eq!(stats[1].failures, 1);
}

#[tokio::test]
async fn tool_stats_by_model_null_model_is_unknown() {
    let repo = test_repo().await;
    let pool = repo.pool();

    {
        let conn = pool.get().await.expect("should get connection");

        conn.interact(|conn| {
            conn.execute(
                "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at)
                 VALUES ('t-null', 'm1', 's1', 'bash', 'success', 1722400000)",
                [],
            )
        })
        .await
        .expect("interact should succeed")
        .expect("insert should succeed");
    } // conn dropped here, pool connection released

    let stats = repo.tool_stats_by_model(None).await.unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].model, "unknown");
    assert_eq!(stats[0].total, 1);
    assert_eq!(stats[0].failures, 0);
}
