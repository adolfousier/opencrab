//! Analytics Event Repository
//!
//! Tracks phantom detections, streaming recoveries, and brain verify events
//! for Mission Control analytics (#897). Append-only event log.

use crate::db::Pool;
use crate::db::database::interact_err;
use anyhow::{Context, Result};
use rusqlite::params;

/// A phantom tool call detection event.
#[derive(Debug, Clone)]
pub struct PhantomEvent {
    pub id: String,
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub detected_at: i64,
    pub resolved: bool,
    pub retry_count: i64,
    pub tools_after_retry: i64,
}

/// A streaming tool call recovery event.
#[derive(Debug, Clone)]
pub struct StreamingRecovery {
    pub id: String,
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub recovered_at: i64,
    pub tool_count: i64,
}

/// A brain verify gate event.
#[derive(Debug, Clone)]
pub struct BrainVerifyEvent {
    pub id: String,
    pub file_name: String,
    pub event_type: String,
    pub violations: Option<String>,
    pub created_at: i64,
}

/// Aggregated phantom stats for a time window.
#[derive(Debug, Clone, Default)]
pub struct PhantomStats {
    pub total: i64,
    pub resolved: i64,
    pub by_model: Vec<(String, i64, i64)>, // (model, total, resolved)
}

/// Aggregated streaming recovery stats.
#[derive(Debug, Clone, Default)]
pub struct StreamingStats {
    pub total: i64,
    pub total_tools: i64,
    pub by_model: Vec<(String, i64)>, // (model, count)
}

/// Aggregated brain verify stats.
#[derive(Debug, Clone, Default)]
pub struct BrainVerifyStats {
    pub passes: i64,
    pub rollbacks: i64,
    pub fail_closed: i64,
}

/// Per-model tool failure stats.
#[derive(Debug, Clone)]
pub struct ModelToolStats {
    pub model: String,
    pub total: i64,
    pub failures: i64,
}

/// Repository for analytics event tracking.
#[derive(Clone)]
pub struct AnalyticsEventRepository {
    pool: Pool,
}

impl AnalyticsEventRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Access the underlying pool (for tests and cross-repo queries).
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    // ─── Best-effort Emitters ─────────────────────────────────────────
    //
    // These wrap the async record_* methods in a fire-and-forget spawn so
    // analytics never blocks or breaks the hot path. Errors are logged, never
    // propagated. All read provider/model from the global pool at call time.

    /// Fire-and-forget phantom detection event. Called when the phantom
    /// detector fires (WARN). Provider/model are read live so the event is
    /// tagged with whatever was active when the phantom was emitted.
    pub fn emit_phantom(session_id: &str, provider: Option<&str>, model: Option<&str>) {
        let Some(pool) = crate::db::global_pool() else {
            return;
        };
        let repo = Self::new(pool.clone());
        let id = format!("phantom-{}", uuid::Uuid::new_v4());
        let session_id = session_id.to_string();
        let provider = provider.map(|s| s.to_string());
        let model = model.map(|s| s.to_string());
        tokio::spawn(async move {
            if let Err(e) = repo
                .record_phantom(&id, &session_id, provider.as_deref(), model.as_deref())
                .await
            {
                tracing::error!("[ANALYTICS] phantom record failed: {e}");
            }
        });
    }

    /// Fire-and-forget phantom resolution. Called when, after a phantom, the
    /// retry produces real tool calls.
    pub fn emit_resolve_phantom(session_id: &str, retry_count: i64, tools_after_retry: i64) {
        let Some(pool) = crate::db::global_pool() else {
            return;
        };
        let repo = Self::new(pool.clone());
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = repo
                .resolve_phantom(&session_id, retry_count, tools_after_retry)
                .await
            {
                tracing::error!("[ANALYTICS] phantom resolve failed: {e}");
            }
        });
    }

    /// Fire-and-forget streaming recovery event.
    pub fn emit_streaming_recovery(
        session_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
        tool_count: i64,
    ) {
        let Some(pool) = crate::db::global_pool() else {
            return;
        };
        let repo = Self::new(pool.clone());
        let id = format!("stream-recover-{}", uuid::Uuid::new_v4());
        let session_id = session_id.to_string();
        let provider = provider.map(|s| s.to_string());
        let model = model.map(|s| s.to_string());
        tokio::spawn(async move {
            if let Err(e) = repo
                .record_streaming_recovery(
                    &id,
                    &session_id,
                    provider.as_deref(),
                    model.as_deref(),
                    tool_count,
                )
                .await
            {
                tracing::error!("[ANALYTICS] streaming recovery record failed: {e}");
            }
        });
    }

    /// Fire-and-forget brain verify gate event. `event_type` is one of
    /// "pass" | "rollback" | "fail_closed".
    pub fn emit_brain_verify(file_name: &str, event_type: &str, violations: Option<&str>) {
        let Some(pool) = crate::db::global_pool() else {
            return;
        };
        let repo = Self::new(pool.clone());
        let id = format!("brain-verify-{}", uuid::Uuid::new_v4());
        let file_name = file_name.to_string();
        let event_type = event_type.to_string();
        let violations = violations.map(|s| s.to_string());
        tokio::spawn(async move {
            if let Err(e) = repo
                .record_brain_verify(&id, &file_name, &event_type, violations.as_deref())
                .await
            {
                tracing::error!("[ANALYTICS] brain verify record failed: {e}");
            }
        });
    }

    // ─── Phantom Events ───────────────────────────────────────────────

    /// Record a phantom detection event.
    pub async fn record_phantom(
        &self,
        id: &str,
        session_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<()> {
        let id = id.to_string();
        let session_id = session_id.to_string();
        let provider = provider.map(|s| s.to_string());
        let model = model.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO phantom_events (id, session_id, provider, model) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, session_id, provider, model],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to record phantom event")?;
        Ok(())
    }

    /// Mark a phantom event as resolved (retry produced real tool calls).
    pub async fn resolve_phantom(
        &self,
        session_id: &str,
        retry_count: i64,
        tools_after_retry: i64,
    ) -> Result<()> {
        let session_id = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE phantom_events SET resolved = 1, retry_count = ?2, tools_after_retry = ?3 \
                     WHERE session_id = ?1 AND resolved = 0 \
                     AND detected_at = (SELECT MAX(detected_at) FROM phantom_events WHERE session_id = ?1 AND resolved = 0)",
                    params![session_id, retry_count, tools_after_retry],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to resolve phantom event")?;
        Ok(())
    }

    /// Query phantom stats since an epoch.
    pub async fn phantom_stats(&self, since_epoch: Option<i64>) -> Result<PhantomStats> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| -> rusqlite::Result<PhantomStats> {
                let (total, resolved) = if let Some(since) = since_epoch {
                    let t: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM phantom_events WHERE detected_at >= ?1",
                        params![since],
                        |r| r.get(0),
                    )?;
                    let res: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM phantom_events WHERE detected_at >= ?1 AND resolved = 1",
                        params![since],
                        |r| r.get(0),
                    )?;
                    (t, res)
                } else {
                    let t: i64 =
                        conn.query_row("SELECT COUNT(*) FROM phantom_events", [], |r| r.get(0))?;
                    let res: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM phantom_events WHERE resolved = 1",
                        [],
                        |r| r.get(0),
                    )?;
                    (t, res)
                };

                let by_model_query = if since_epoch.is_some() {
                    "SELECT COALESCE(model, 'unknown'), COUNT(*), SUM(CASE WHEN resolved = 1 THEN 1 ELSE 0 END) \
                     FROM phantom_events WHERE detected_at >= ?1 GROUP BY model ORDER BY COUNT(*) DESC"
                } else {
                    "SELECT COALESCE(model, 'unknown'), COUNT(*), SUM(CASE WHEN resolved = 1 THEN 1 ELSE 0 END) \
                     FROM phantom_events GROUP BY model ORDER BY COUNT(*) DESC"
                };
                let mut stmt = conn.prepare_cached(by_model_query)?;
                let by_model: Vec<(String, i64, i64)> = if let Some(since) = since_epoch {
                    let rows = stmt.query_map(params![since], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        ))
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                } else {
                    let rows = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        ))
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                };

                Ok(PhantomStats {
                    total,
                    resolved,
                    by_model,
                })
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query phantom stats")
    }

    // ─── Streaming Recoveries ─────────────────────────────────────────

    /// Record a streaming tool call recovery.
    pub async fn record_streaming_recovery(
        &self,
        id: &str,
        session_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
        tool_count: i64,
    ) -> Result<()> {
        let id = id.to_string();
        let session_id = session_id.to_string();
        let provider = provider.map(|s| s.to_string());
        let model = model.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO streaming_recoveries (id, session_id, provider, model, tool_count) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, session_id, provider, model, tool_count],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to record streaming recovery")?;
        Ok(())
    }

    /// Query streaming recovery stats since an epoch.
    pub async fn streaming_stats(&self, since_epoch: Option<i64>) -> Result<StreamingStats> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| -> rusqlite::Result<StreamingStats> {
                let (total, total_tools) = if let Some(since) = since_epoch {
                    let t: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM streaming_recoveries WHERE recovered_at >= ?1",
                        params![since],
                        |r| r.get(0),
                    )?;
                    let tools: i64 = conn.query_row(
                        "SELECT COALESCE(SUM(tool_count), 0) FROM streaming_recoveries WHERE recovered_at >= ?1",
                        params![since],
                        |r| r.get(0),
                    )?;
                    (t, tools)
                } else {
                    let t: i64 = conn
                        .query_row("SELECT COUNT(*) FROM streaming_recoveries", [], |r| {
                            r.get(0)
                        })?;
                    let tools: i64 = conn.query_row(
                        "SELECT COALESCE(SUM(tool_count), 0) FROM streaming_recoveries",
                        [],
                        |r| r.get(0),
                    )?;
                    (t, tools)
                };

                let by_model_query = if since_epoch.is_some() {
                    "SELECT COALESCE(model, 'unknown'), COUNT(*) FROM streaming_recoveries \
                     WHERE recovered_at >= ?1 GROUP BY model ORDER BY COUNT(*) DESC"
                } else {
                    "SELECT COALESCE(model, 'unknown'), COUNT(*) FROM streaming_recoveries \
                     GROUP BY model ORDER BY COUNT(*) DESC"
                };
                let mut stmt = conn.prepare_cached(by_model_query)?;
                let by_model: Vec<(String, i64)> = if let Some(since) = since_epoch {
                    let rows = stmt.query_map(params![since], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                } else {
                    let rows = stmt.query_map([], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                };

                Ok(StreamingStats {
                    total,
                    total_tools,
                    by_model,
                })
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query streaming stats")
    }

    // ─── Brain Verify Events ──────────────────────────────────────────

    /// Record a brain verify gate event.
    pub async fn record_brain_verify(
        &self,
        id: &str,
        file_name: &str,
        event_type: &str,
        violations: Option<&str>,
    ) -> Result<()> {
        let id = id.to_string();
        let file_name = file_name.to_string();
        let event_type = event_type.to_string();
        let violations = violations.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO brain_verify_events (id, file_name, event_type, violations) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, file_name, event_type, violations],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to record brain verify event")?;
        Ok(())
    }

    /// Query brain verify stats since an epoch.
    pub async fn brain_verify_stats(&self, since_epoch: Option<i64>) -> Result<BrainVerifyStats> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| -> rusqlite::Result<BrainVerifyStats> {
                let query = if since_epoch.is_some() {
                    "SELECT \
                        SUM(CASE WHEN event_type = 'pass' THEN 1 ELSE 0 END), \
                        SUM(CASE WHEN event_type = 'rollback' THEN 1 ELSE 0 END), \
                        SUM(CASE WHEN event_type = 'fail_closed' THEN 1 ELSE 0 END) \
                     FROM brain_verify_events WHERE created_at >= ?1"
                } else {
                    "SELECT \
                        SUM(CASE WHEN event_type = 'pass' THEN 1 ELSE 0 END), \
                        SUM(CASE WHEN event_type = 'rollback' THEN 1 ELSE 0 END), \
                        SUM(CASE WHEN event_type = 'fail_closed' THEN 1 ELSE 0 END) \
                     FROM brain_verify_events"
                };
                let result = if let Some(since) = since_epoch {
                    conn.query_row(query, params![since], |r| {
                        Ok(BrainVerifyStats {
                            passes: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                            rollbacks: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                            fail_closed: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        })
                    })?
                } else {
                    conn.query_row(query, [], |r| {
                        Ok(BrainVerifyStats {
                            passes: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                            rollbacks: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                            fail_closed: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        })
                    })?
                };
                Ok(result)
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query brain verify stats")
    }

    // ─── Per-Model Tool Stats ─────────────────────────────────────────

    /// Per-model tool failure stats since an epoch.
    pub async fn tool_stats_by_model(
        &self,
        since_epoch: Option<i64>,
    ) -> Result<Vec<ModelToolStats>> {
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let (query, param): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                    if let Some(since) = since_epoch {
                        (
                            "SELECT COALESCE(model, 'unknown'), COUNT(*), \
                                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) \
                             FROM tool_executions \
                             WHERE created_at >= ?1 AND tool_name <> '' \
                             GROUP BY model ORDER BY COUNT(*) DESC"
                                .to_string(),
                            vec![Box::new(since)],
                        )
                    } else {
                        (
                            "SELECT COALESCE(model, 'unknown'), COUNT(*), \
                                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) \
                             FROM tool_executions \
                             WHERE tool_name <> '' \
                             GROUP BY model ORDER BY COUNT(*) DESC"
                                .to_string(),
                            vec![],
                        )
                    };
                let mut stmt = conn.prepare_cached(&query)?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    param.iter().map(|p| p.as_ref()).collect();
                let rows = stmt.query_map(param_refs.as_slice(), |row| {
                    Ok(ModelToolStats {
                        model: row.get(0)?,
                        total: row.get(1)?,
                        failures: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    })
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to query per-model tool stats")
    }
}
