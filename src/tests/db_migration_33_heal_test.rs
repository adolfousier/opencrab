//! Regression tests for the migration 33 heal (#937, PR #938).
//!
//! Migration 33 (`add_analytics_events`) ALTERs `tool_executions` to add
//! `provider`, `model`, and `duration_ms`. Databases that already carry some
//! of those artifacts (intermediate builds, interrupted runs) used to crash
//! with "duplicate column name: provider". The heal in `run_migrations`
//! detects pre-existing artifacts at `user_version == 32` and completes the
//! schema instead of failing.

use crate::db::Database;

/// In-memory DB with migrations 1..=32 applied (user_version == 32), the
/// exact state where #937 crashes on the next `run_migrations()` call.
async fn db_at_version_32() -> Database {
    let db = Database::connect_in_memory().await.unwrap();
    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| -> Result<(), String> {
            crate::db::database::build_migrations()
                .to_version(conn, 32)
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap()
        .unwrap();
    db
}

fn assert_analytics_columns(conn: &rusqlite::Connection) {
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(tool_executions)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    for col in ["provider", "model", "duration_ms"] {
        assert!(
            cols.iter().any(|c| c == col),
            "tool_executions column {col} present, got: {cols:?}"
        );
    }
}

fn assert_analytics_tables(conn: &rusqlite::Connection) {
    for table in [
        "phantom_events",
        "streaming_recoveries",
        "brain_verify_events",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "analytics table {table} exists");
    }
}

/// Mirrors the repository insert in db/repository/tool_execution.rs: all
/// three migration-33 columns must accept data after the migration runs.
fn assert_tool_execution_insert(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tool_executions
            (id, message_id, tool_name, provider, model, duration_ms)
         VALUES
            ('heal-test-1', 'm1', 'bash', 'test-provider', 'test-model', 42)",
        [],
    )?;
    Ok(())
}

/// The #937 scenario: user_version == 32 and `provider` already exists.
/// Before the heal, `run_migrations` crashed with "duplicate column name".
#[tokio::test]
async fn heals_pre_existing_provider_column_at_v32() {
    let db = db_at_version_32().await;

    // Simulate the broken pre-state: provider column added by an
    // intermediate build or interrupted run.
    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            conn.execute_batch("ALTER TABLE tool_executions ADD COLUMN provider TEXT;")?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

    // Must not fail with "duplicate column name: provider".
    db.run_migrations().await.unwrap();

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(version, Database::MIGRATION_COUNT as i64);
            assert_analytics_columns(conn);
            assert_analytics_tables(conn);
            assert_tool_execution_insert(conn)?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
}

/// Partial pre-state: provider AND model exist, duration_ms missing. The
/// heal must add the missing column, not skip it.
#[tokio::test]
async fn heals_partial_columns_adding_missing_duration_ms() {
    let db = db_at_version_32().await;

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            conn.execute_batch(
                "ALTER TABLE tool_executions ADD COLUMN provider TEXT;
                 ALTER TABLE tool_executions ADD COLUMN model TEXT;",
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

    db.run_migrations().await.unwrap();

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(version, Database::MIGRATION_COUNT as i64);
            assert_analytics_columns(conn);
            assert_analytics_tables(conn);
            assert_tool_execution_insert(conn)?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
}

/// Normal path guard: a fresh DB with nothing pre-existing must go through
/// migration 33 untouched and end with the full analytics schema.
#[tokio::test]
async fn fresh_migration_adds_analytics_schema_normally() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(version, Database::MIGRATION_COUNT as i64);
            assert_analytics_columns(conn);
            assert_analytics_tables(conn);
            assert_tool_execution_insert(conn)?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
}

/// Idempotency: running migrations again after a healed run must stay green
/// (heal gate is user_version == 32, second run is at 33 and skips it).
#[tokio::test]
async fn heal_is_idempotent_across_double_run() {
    let db = db_at_version_32().await;

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            conn.execute_batch("ALTER TABLE tool_executions ADD COLUMN provider TEXT;")?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

    db.run_migrations().await.unwrap();
    db.run_migrations().await.unwrap();

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(version, Database::MIGRATION_COUNT as i64);
            assert_analytics_columns(conn);
            assert_analytics_tables(conn);
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
}
