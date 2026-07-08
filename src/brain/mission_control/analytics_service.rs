//! Analytics data service: brain sizes, tool usage/reliability, and RSI
//! application counts for the Mission Control analytics panel.
//!
//! Reads only data OpenCrabs already owns: the `tool_executions` and
//! `feedback_ledger` tables and the active profile's brain `.md` files. No
//! secrets, no message content, nothing leaves the machine. This is the
//! native, real-time home for the analytics that the external
//! `opencrabs-analytics` tool produced as a static HTML upload (discussion
//! #178), same numbers, no public paste, no CDN.
//!
//! `summary` is async because the stats live in SQLite; the renderer
//! pre-fetches once (like the other panels) rather than per `draw`.

use super::types::{McAnalytics, McBrainFile, McToolStat};
use crate::db::Pool;
use crate::db::repository::{FeedbackLedgerRepository, ToolExecutionRepository};

/// How many rows to surface in each ranked list.
const TOP_N: usize = 10;
/// A tool needs at least this many calls before its fail rate is meaningful.
const FLAKY_MIN_CALLS: i64 = 5;

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Build the analytics snapshot. Never errors: a DB blip yields zeros so the
/// panel degrades gracefully instead of taking Mission Control down.
pub async fn summary(pool: Pool) -> McAnalytics {
    let tools = tool_stats(pool.clone()).await;
    let (rsi_last_call_ts, tool_events_since_rsi) = rsi_staleness(pool.clone()).await;
    let (rsi_applied_total, rsi_top_dimensions) = rsi_stats(pool).await;
    let brain_files = collect_brain_sizes();
    let brain_total_kb = round1(brain_files.iter().map(|b| b.kb).sum::<f64>());

    let tool_total_calls = tools.iter().map(|t| t.total).sum();
    let tool_total_fails = tools.iter().map(|t| t.failures).sum();
    let top_tools = tools.iter().take(TOP_N).cloned().collect();

    let mut flakiest_tools: Vec<McToolStat> = tools
        .into_iter()
        .filter(|t| t.total >= FLAKY_MIN_CALLS && t.failures > 0)
        .collect();
    flakiest_tools.sort_by(|a, b| {
        b.fail_rate
            .partial_cmp(&a.fail_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    flakiest_tools.truncate(TOP_N);

    McAnalytics {
        tool_total_calls,
        tool_total_fails,
        top_tools,
        flakiest_tools,
        rsi_applied_total,
        rsi_last_call_ts,
        tool_events_since_rsi,
        rsi_top_dimensions,
        brain_files,
        brain_total_kb,
    }
}

/// Per-tool usage with fail rate, most-used first.
async fn tool_stats(pool: Pool) -> Vec<McToolStat> {
    let repo = ToolExecutionRepository::new(pool);
    let rows = repo.stats_with_failures(None).await.unwrap_or_else(|e| {
        tracing::warn!("analytics_service: tool stats query failed: {e:#}");
        Vec::new()
    });
    rows.into_iter()
        .map(|r| {
            let fail_rate = if r.total > 0 {
                round1((r.failures as f64 / r.total as f64) * 100.0)
            } else {
                0.0
            };
            McToolStat {
                name: r.tool_name,
                total: r.total,
                failures: r.failures,
                fail_rate,
            }
        })
        .collect()
}

/// Last RSI activity and tool events recorded after it (#469).
async fn rsi_staleness(pool: Pool) -> (Option<i64>, i64) {
    let repo = ToolExecutionRepository::new(pool);
    repo.rsi_staleness().await.unwrap_or_else(|e| {
        tracing::warn!("analytics_service: rsi staleness query failed: {e:#}");
        (None, 0)
    })
}

/// Total `improvement_applied` RSI events and their top dimensions.
async fn rsi_stats(pool: Pool) -> (i64, Vec<(String, i64)>) {
    let repo = FeedbackLedgerRepository::new(pool);
    let dims = repo
        .stats_by_dimension("improvement_applied")
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("analytics_service: rsi stats query failed: {e:#}");
            Vec::new()
        });
    let total = dims.iter().map(|d| d.total_events).sum();
    let mut by_dim: Vec<(String, i64)> = dims
        .into_iter()
        .map(|d| (d.dimension, d.total_events))
        .collect();
    by_dim.sort_by_key(|d| std::cmp::Reverse(d.1));
    by_dim.truncate(TOP_N);
    (total, by_dim)
}

/// Sizes (KB) of the active profile's brain `.md` files, largest first.
/// Only file names and sizes, never contents or headings, so the snapshot
/// carries no personal context.
fn collect_brain_sizes() -> Vec<McBrainFile> {
    let dir = crate::config::opencrabs_home();
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md")
                && let Ok(meta) = entry.metadata()
            {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                files.push(McBrainFile {
                    name,
                    kb: round1(meta.len() as f64 / 1024.0),
                });
            }
        }
    }
    files.sort_by(|a, b| b.kb.partial_cmp(&a.kb).unwrap_or(std::cmp::Ordering::Equal));
    files
}
