//! Pending Request Repository
//!
//! Tracks in-flight agent requests so they can be replayed after a restart.
//! Rows only exist while a request is PROCESSING — they are deleted on
//! completion (success or failure). Any rows left in the table on startup
//! indicate the process crashed mid-request and should be replayed.

use crate::db::Pool;
use crate::db::database::interact_err;
use anyhow::{Context, Result};
use rusqlite::params;
use uuid::Uuid;

/// A pending request row
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub id: String,
    pub session_id: String,
    pub user_message: String,
    pub channel: String,
    pub channel_chat_id: Option<String>,
}

/// Repository for pending request operations
#[derive(Clone)]
pub struct PendingRequestRepository {
    pool: Pool,
}

impl PendingRequestRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Insert a new in-flight request
    pub async fn insert(
        &self,
        id: Uuid,
        session_id: Uuid,
        user_message: &str,
        channel: &str,
        channel_chat_id: Option<&str>,
    ) -> Result<()> {
        let id_s = id.to_string();
        let sid = session_id.to_string();
        let msg = user_message.to_string();
        let ch = channel.to_string();
        let cid = channel_chat_id.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO pending_requests (id, session_id, user_message, channel, channel_chat_id, status) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 'PROCESSING')",
                    params![id_s, sid, msg, ch, cid],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to insert pending request")?;
        Ok(())
    }

    /// Bump a request's `updated_at` to now — its "last interaction". Called
    /// as mid-turn progress persists so a long-running turn never trips the
    /// 24h crash-debris cutoff in [`get_interrupted`].
    ///
    /// [`get_interrupted`]: Self::get_interrupted
    pub async fn touch(&self, id: Uuid) -> Result<()> {
        let id_s = id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE pending_requests SET updated_at = unixepoch() WHERE id = ?1",
                    params![id_s],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to touch pending request")?;
        Ok(())
    }

    /// Bump `updated_at` for every pending row of a session. The agent's
    /// mid-turn persistence calls this (it knows the session, not the row id)
    /// so a long-running turn's last interaction stays fresh.
    pub async fn touch_session(&self, session_id: Uuid) -> Result<()> {
        let sid = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE pending_requests SET updated_at = unixepoch() WHERE session_id = ?1",
                    params![sid],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to touch pending requests for session")?;
        Ok(())
    }

    /// Delete a request (called when it finishes, regardless of outcome)
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let id_s = id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute("DELETE FROM pending_requests WHERE id = ?1", params![id_s])
            })
            .await
            .map_err(interact_err)?
            .context("Failed to delete pending request")?;
        Ok(())
    }

    /// Get ALL surviving rows (process died while these were in-flight).
    ///
    /// A row only exists while a request is PROCESSING — completion deletes it
    /// and the startup resume path clears the table after reading — so any
    /// surviving row IS interrupted work, no matter how long ago the turn
    /// STARTED. The old 10-minute created_at window silently dropped exactly
    /// the turns that most need resuming: long agentic runs (an interrupted
    /// 28-minute CLI coding turn was purged while a 5-minute-old one resumed).
    /// The only age guard left is a 24h cap on updated_at (last interaction)
    /// to clear crash debris from installs that never reached the resume path.
    pub async fn get_interrupted(&self) -> Result<Vec<PendingRequest>> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(|conn| {
                // Crash debris only: rows whose LAST interaction is over a day old.
                let _ = conn.execute(
                    "DELETE FROM pending_requests WHERE updated_at < unixepoch() - 86400",
                    [],
                );
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, user_message, channel, channel_chat_id \
                     FROM pending_requests \
                     ORDER BY created_at ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(PendingRequest {
                        id: row.get("id")?,
                        session_id: row.get("session_id")?,
                        user_message: row.get("user_message")?,
                        channel: row.get("channel")?,
                        channel_chat_id: row.get("channel_chat_id")?,
                    })
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to get interrupted requests")
    }

    /// Get interrupted requests for a specific channel. Same semantics as
    /// [`get_interrupted`]: every surviving row is interrupted work; only
    /// 24h-stale crash debris (by last interaction) is purged.
    ///
    /// [`get_interrupted`]: Self::get_interrupted
    pub async fn get_interrupted_for_channel(&self, channel: &str) -> Result<Vec<PendingRequest>> {
        let ch = channel.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let _ = conn.execute(
                    "DELETE FROM pending_requests WHERE channel = ?1 AND updated_at < unixepoch() - 86400",
                    params![ch],
                );
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, user_message, channel, channel_chat_id \
                     FROM pending_requests WHERE channel = ?1 \
                     ORDER BY created_at ASC",
                )?;
                let rows = stmt.query_map(params![ch], |row| {
                    Ok(PendingRequest {
                        id: row.get("id")?,
                        session_id: row.get("session_id")?,
                        user_message: row.get("user_message")?,
                        channel: row.get("channel")?,
                        channel_chat_id: row.get("channel_chat_id")?,
                    })
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to get interrupted requests for channel")
    }

    /// Delete specific requests by ID
    pub async fn delete_ids(&self, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                for id in &ids {
                    conn.execute("DELETE FROM pending_requests WHERE id = ?1", params![id])?;
                }
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(interact_err)?
            .context("Failed to delete pending requests")?;
        Ok(())
    }

    /// Delete all rows (called on startup after reading interrupted requests)
    pub async fn clear_all(&self) -> Result<()> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(|conn| conn.execute("DELETE FROM pending_requests", []))
            .await
            .map_err(interact_err)?
            .context("Failed to clear pending requests")?;
        Ok(())
    }
}
