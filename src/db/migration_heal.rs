//! Repair a schema that the version stamp says is complete but is not (#1401).
//!
//! `rusqlite_migration` applies migrations by list INDEX. When two branches
//! each append "migration N" and the merge re-sorts the list by date, any
//! database that was stamped N by the first branch's build skips the other
//! branch's migration forever: the stamp says done, so `to_latest` never
//! looks at it. That is how `pending_requests.origin` (migration 37) went
//! missing on a database stamped 38, and with it every restart-recovery row
//! for three days: the INSERT failed on every turn and boot found nothing to
//! resume.
//!
//! A heal is idempotent and checks the schema, not the stamp. It runs after
//! `to_latest` on every boot, so a healed database stays healed and a
//! correct one is untouched.

/// Does `table` carry a column named `column`?
pub(crate) fn has_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let cols: Vec<String> = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .collect();
    Ok(cols.iter().any(|c| c == column))
}

fn has_table(conn: &rusqlite::Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

/// Add `pending_requests.origin` when migration 37 was skipped.
///
/// Mirrors `src/migrations/20260828000001_pending_requests_origin.sql`, which
/// stays the source of truth. Returns `true` when it changed the schema.
pub(crate) fn heal_pending_requests_origin(conn: &rusqlite::Connection) -> rusqlite::Result<bool> {
    if !has_table(conn, "pending_requests")? || has_column(conn, "pending_requests", "origin")? {
        return Ok(false);
    }
    conn.execute_batch(
        "ALTER TABLE pending_requests ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';",
    )?;
    tracing::warn!(
        "Healed pending_requests: the origin column of migration 37 was missing although the \
         schema was stamped past it (#1401). Restart recovery could not record turns until now."
    );
    Ok(true)
}
