//! Startup must survive a database that already ran the `channel_thread_id`
//! migration under an older list order (#1401 again).
//!
//! `rusqlite_migration` applies migrations by list INDEX.
//! `20260908000001_pending_requests_thread_id.sql` sat at index 40 and was
//! applied there, stamping `user_version = 40`. Merging
//! `20260906000001_add_notify_queue.sql`, whose filename date is earlier,
//! re-sorted the list and pushed the thread-id migration to index 41. On the
//! next boot `to_latest` replays it against a table that already has the
//! column, and SQLite rejects the duplicate. That does not fail one migration,
//! it fails startup: the binary refuses to launch.
//!
//! The other heals in this module repair a migration that was skipped, and can
//! run after `to_latest`. This one has to run before it, because the crash it
//! prevents happens inside it and there is no "after".

use crate::db::Database;

/// The exact broken shape: stamped 40, `channel_thread_id` already present,
/// `notify_queue` never created. Reproduced by applying migrations to 41 (so
/// the column exists), dropping the table the real database is missing, then
/// winding the stamp back to 40.
async fn db_in_the_broken_state() -> Database {
    let db = Database::connect_in_memory().await.unwrap();
    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| -> Result<(), String> {
            crate::db::database::build_migrations()
                .to_version(conn, 41)
                .map_err(|e| e.to_string())?;
            conn.execute_batch("DROP TABLE IF EXISTS notify_queue;")
                .map_err(|e| e.to_string())?;
            conn.pragma_update(None, "user_version", 40i64)
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap()
        .unwrap();
    db
}

fn has_column(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .any(|c| c == column)
}

fn has_table(conn: &rusqlite::Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

#[tokio::test]
async fn migrations_survive_a_thread_id_column_that_already_exists() {
    let db = db_in_the_broken_state().await;

    db.run_migrations()
        .await
        .expect("startup must not fail on a migration that already ran");

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            assert!(
                has_column(conn, "pending_requests", "channel_thread_id"),
                "the column that caused the clash must still be there"
            );
            // The migration this database genuinely was missing: index 40 was
            // never applied, because the thread-id migration occupied that slot
            // when this database was stamped. The post-pass heal creates it.
            assert!(
                has_table(conn, "notify_queue"),
                "the skipped migration's table must be healed, not left behind"
            );
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_healthy_database_is_left_alone() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    // Running again is what every boot does; it must stay a no-op.
    db.run_migrations().await.unwrap();

    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| {
            assert!(has_column(conn, "pending_requests", "channel_thread_id"));
            assert!(has_table(conn, "notify_queue"));
        })
        .await
        .unwrap();
}

#[test]
fn the_guard_refuses_to_skip_an_unapplied_migration() {
    // A stamp one behind but WITHOUT the column is a database that genuinely
    // needs the migration. Stamping past it there would silently lose the
    // column, so the guard must decline.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE pending_requests (id TEXT PRIMARY KEY);")
        .unwrap();

    let stamped = crate::db::migration_heal::skip_applied_thread_id_migration(&conn, 40).unwrap();

    assert!(
        !stamped,
        "without the column the migration still has work to do"
    );
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0, "the stamp must be left untouched");
}
