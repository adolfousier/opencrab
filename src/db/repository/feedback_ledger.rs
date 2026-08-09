//! Feedback Ledger Repository
//!
//! Append-only observations for recursive self-improvement.
//! Records tool outcomes, user corrections, provider errors, and performance
//! signals. Entries are never deleted — consumed by analysis tools.

use crate::db::Pool;
use crate::db::database::interact_err;
use crate::db::models::FeedbackEntry;
use anyhow::{Context, Result};
use rusqlite::params;

/// Aggregated stats for a single dimension (tool name, provider, etc.)
#[derive(Debug, Clone)]
pub struct DimensionStats {
    pub dimension: String,
    pub total_events: i64,
    pub successes: i64,
    pub failures: i64,
    pub success_rate: f64,
    pub avg_value: f64,
}

/// Repository for feedback ledger operations
#[derive(Clone)]
pub struct FeedbackLedgerRepository {
    pool: Pool,
}

impl FeedbackLedgerRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Get a clone of the underlying pool.
    pub fn pool(&self) -> Pool {
        self.pool.clone()
    }

    /// Record a feedback event (append-only)
    pub async fn record(
        &self,
        session_id: &str,
        event_type: &str,
        dimension: &str,
        value: f64,
        metadata: Option<&str>,
    ) -> Result<i64> {
        let sid = session_id.to_string();
        let et = event_type.to_string();
        let dim = dimension.to_string();
        let meta = metadata.map(|s| s.to_string());

        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| -> rusqlite::Result<i64> {
                conn.execute(
                    "INSERT INTO feedback_ledger (session_id, event_type, dimension, value, metadata) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![sid, et, dim, value, meta],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .map_err(interact_err)?
            .context("Failed to record feedback")
    }

    /// Get recent feedback entries (most recent first)
    pub async fn recent(&self, limit: u32) -> Result<Vec<FeedbackEntry>> {
        let lim = limit as i64;
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT * FROM feedback_ledger ORDER BY created_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![lim], FeedbackEntry::from_row)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query recent feedback")
    }

    /// Get feedback entries filtered by event type
    pub async fn by_event_type(&self, event_type: &str, limit: u32) -> Result<Vec<FeedbackEntry>> {
        let et = event_type.to_string();
        let lim = limit as i64;
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT * FROM feedback_ledger WHERE event_type = ?1 \
                     ORDER BY created_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![et, lim], FeedbackEntry::from_row)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query feedback by type")
    }

    /// Get aggregated stats per dimension for a given event type.
    /// For tool_success/tool_failure, dimension is the tool name.
    /// Lifetime aggregate — see `stats_by_dimension_since` for windowed.
    pub async fn stats_by_dimension(&self, event_type_prefix: &str) -> Result<Vec<DimensionStats>> {
        self.stats_by_dimension_since(event_type_prefix, None).await
    }

    /// Same as `stats_by_dimension`, but only counts events whose
    /// `created_at` is at or after `since` (ISO 8601 / RFC3339 string
    /// matching the column's storage format). Pass `None` to get the
    /// full lifetime aggregate.
    ///
    /// Without the window, a tool that broke once and was fixed shows
    /// "100% failure" until the success count finally exceeds the
    /// failure count. The 2026-04-25 RSI logs were full of stale
    /// "exa_search 100% failure" / "wait_agent 100% failure"
    /// opportunities long after both bugs landed fixes — old failure
    /// rows never aged out.
    pub async fn stats_by_dimension_since(
        &self,
        event_type_prefix: &str,
        since: Option<&str>,
    ) -> Result<Vec<DimensionStats>> {
        let prefix = format!("{}%", event_type_prefix);
        let since = since.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                if let Some(since_ts) = since {
                    let mut stmt = conn.prepare_cached(
                        "SELECT \
                           dimension, \
                           COUNT(*) as total, \
                           SUM(CASE WHEN event_type = 'tool_success' THEN 1 ELSE 0 END) as successes, \
                           SUM(CASE WHEN event_type = 'tool_failure' THEN 1 ELSE 0 END) as failures, \
                           CASE WHEN COUNT(*) > 0 \
                             THEN CAST(SUM(CASE WHEN event_type = 'tool_success' THEN 1 ELSE 0 END) AS REAL) / COUNT(*) \
                             ELSE 0.0 END as success_rate, \
                           AVG(value) as avg_value \
                         FROM feedback_ledger \
                         WHERE event_type LIKE ?1 AND created_at >= ?2 \
                         GROUP BY dimension \
                         ORDER BY total DESC",
                    )?;
                    let rows = stmt.query_map(params![prefix, since_ts], |row| {
                        Ok(DimensionStats {
                            dimension: row.get(0)?,
                            total_events: row.get(1)?,
                            successes: row.get(2)?,
                            failures: row.get(3)?,
                            success_rate: row.get(4)?,
                            avg_value: row.get(5)?,
                        })
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                } else {
                    let mut stmt = conn.prepare_cached(
                        "SELECT \
                           dimension, \
                           COUNT(*) as total, \
                           SUM(CASE WHEN event_type = 'tool_success' THEN 1 ELSE 0 END) as successes, \
                           SUM(CASE WHEN event_type = 'tool_failure' THEN 1 ELSE 0 END) as failures, \
                           CASE WHEN COUNT(*) > 0 \
                             THEN CAST(SUM(CASE WHEN event_type = 'tool_success' THEN 1 ELSE 0 END) AS REAL) / COUNT(*) \
                             ELSE 0.0 END as success_rate, \
                           AVG(value) as avg_value \
                         FROM feedback_ledger \
                         WHERE event_type LIKE ?1 \
                         GROUP BY dimension \
                         ORDER BY total DESC",
                    )?;
                    let rows = stmt.query_map(params![prefix], |row| {
                        Ok(DimensionStats {
                            dimension: row.get(0)?,
                            total_events: row.get(1)?,
                            successes: row.get(2)?,
                            failures: row.get(3)?,
                            success_rate: row.get(4)?,
                            avg_value: row.get(5)?,
                        })
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query dimension stats")
    }

    /// Count total events
    pub async fn total_count(&self) -> Result<i64> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(|conn| {
                conn.query_row("SELECT COUNT(*) FROM feedback_ledger", [], |row| row.get(0))
            })
            .await
            .map_err(interact_err)?
            .context("Failed to count feedback entries")
    }

    /// Count ACTIONABLE events only: real tool failures, user corrections and
    /// provider errors (#977). `total_count()` includes `tool_success`, which
    /// is recorded on every tool call anywhere, so it climbs on any busy
    /// install and is useless as a "anything new to improve?" signal.
    pub async fn count_actionable(&self) -> Result<i64> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM feedback_ledger \
                     WHERE event_type IN ('tool_failure', 'user_correction', 'provider_error')",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to count actionable feedback entries")
    }

    /// Count events since a given RFC3339 timestamp
    pub async fn count_since(&self, since: &str) -> Result<i64> {
        let s = since.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM feedback_ledger WHERE created_at >= ?1",
                    params![s],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to count feedback since timestamp")
    }

    /// Get summary: total events, unique dimensions, event type breakdown
    pub async fn summary(&self) -> Result<Vec<(String, i64)>> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT event_type, COUNT(*) FROM feedback_ledger \
                     GROUP BY event_type ORDER BY COUNT(*) DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query feedback summary")
    }
}
