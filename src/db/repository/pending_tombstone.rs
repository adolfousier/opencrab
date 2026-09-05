//! Pending Tombstone Repository
//!
//! Durable parking for sub-agent death reports (#73).
//!
//! A tombstone is born at startup, when reconciliation finds a sub-agent
//! status file still mid-flight from a dead process. The report goes to the
//! parent session, but startup runs before channels register routes — so the
//! report usually parks in memory. Memory does not survive another restart,
//! and the status file has already gone terminal by then, so nothing would
//! ever produce the report again: the parent would simply never hear that
//! its agent died.
//!
//! A row exists only while the report is undelivered. Startup re-offers
//! every surviving row; [`Self::clear`] and [`Self::clear_for_session`]
//! remove rows the moment their report reaches a surface, so delivery
//! happens at most once per report.

use crate::db::Pool;
use crate::db::database::interact_err;
use anyhow::{Context, Result};
use rusqlite::params;
use uuid::Uuid;

/// A sub-agent death report that has not reached its session yet.
#[derive(Debug, Clone)]
pub struct PendingTombstoneRow {
    pub id: Uuid,
    pub session_id: Uuid,
    pub context_text: String,
    pub display_text: String,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct PendingTombstoneRepository {
    pool: Pool,
}

impl PendingTombstoneRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Persist an undelivered tombstone so a restart cannot lose it.
    pub async fn record(
        &self,
        id: Uuid,
        session_id: Uuid,
        context_text: &str,
        display_text: &str,
    ) -> Result<()> {
        let (id, session_id) = (id.to_string(), session_id.to_string());
        let (context_text, display_text) = (context_text.to_string(), display_text.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO pending_tombstones \
                     (id, session_id, context_text, display_text, created_at) \
                     VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))",
                    params![id, session_id, context_text, display_text],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to record pending tombstone")?;
        Ok(())
    }

    /// Every surviving row, oldest first.
    pub async fn all(&self) -> Result<Vec<PendingTombstoneRow>> {
        let rows = self
            .pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, context_text, display_text, created_at \
                     FROM pending_tombstones ORDER BY created_at ASC",
                )?;
                let mapped = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok::<_, rusqlite::Error>(mapped)
            })
            .await
            .map_err(interact_err)?
            .context("Failed to list pending tombstones")?;

        Ok(rows
            .into_iter()
            .filter_map(|(id, session_id, context_text, display_text, created_at)| {
                // A row whose ids no longer parse is corrupt, not fatal: skip
                // it rather than failing startup over one unusable record.
                Some(PendingTombstoneRow {
                    id: Uuid::parse_str(&id).ok()?,
                    session_id: Uuid::parse_str(&session_id).ok()?,
                    context_text,
                    display_text,
                    created_at,
                })
            })
            .collect())
    }

    /// Drop the row for a tombstone that has been delivered.
    pub async fn clear(&self, id: Uuid) -> Result<()> {
        let id = id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute("DELETE FROM pending_tombstones WHERE id = ?1", params![id])
            })
            .await
            .map_err(interact_err)?
            .context("Failed to clear pending tombstone")?;
        Ok(())
    }

    /// Drop every undelivered tombstone for a session.
    ///
    /// Called when that session's route claims (or a flush delivers) the
    /// in-memory copies: the durable copies are redundant from that moment.
    pub async fn clear_for_session(&self, session_id: Uuid) -> Result<()> {
        let session_id = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "DELETE FROM pending_tombstones WHERE session_id = ?1",
                    params![session_id],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to clear pending tombstones for session")?;
        Ok(())
    }
}
