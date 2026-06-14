//! Mission Control Report Tool
//!
//! Renders the full Mission Control snapshot (analytics, activity feed,
//! inbox proposals, schedule) as a Markdown report the agent can send
//! through whatever channel it is on. Same data as the TUI Mission
//! Control view, exposed as a shareable report. No secrets, no message
//! content, nothing leaves the machine unless the agent chooses to send
//! the report.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::mission_control::types::{McActivity, McAnalytics, McInboxItem, McScheduleItem};
use crate::brain::mission_control::{activity_service, analytics_service, inbox_service, schedule_service};
use crate::db::Pool;
use async_trait::async_trait;
use serde_json::Value;

/// Generates a shareable mission control report from local stats.
pub struct MissionControlReportTool {
    pool: Pool,
}

impl MissionControlReportTool {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for MissionControlReportTool {
    fn name(&self) -> &str {
        "mission_control_report"
    }

    fn description(&self) -> &str {
        "Generate a shareable mission control report of this OpenCrabs instance: analytics \
         (tool usage, failure rates, RSI improvements, brain files), activity feed, inbox \
         proposals, and scheduled cron jobs. Returns Markdown you can send straight to a chat. \
         Reads only aggregate stats from the local database, brain file sizes, RSI proposals, \
         and cron jobs. Never exposes message content or secrets."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let analytics = analytics_service::summary(self.pool.clone()).await;
        let activity = activity_service::recent(5);
        let inbox = inbox_service::list();
        let schedule = schedule_service::list(self.pool.clone()).await;
        Ok(ToolResult::success(render_markdown(
            &analytics, &activity, &inbox, &schedule,
        )))
    }
}

/// Level icon for activity feed entries.
fn activity_icon(level: &crate::brain::mission_control::types::McActivityLevel) -> &'static str {
    use crate::brain::mission_control::types::McActivityLevel;
    match level {
        McActivityLevel::Success => "✅",
        McActivityLevel::Warn => "⚠️",
        McActivityLevel::Error => "❌",
        McActivityLevel::Info => "ℹ️",
    }
}

/// Icon for inbox item kinds.
fn inbox_icon(kind: &crate::brain::mission_control::types::McInboxKind) -> &'static str {
    use crate::brain::mission_control::types::McInboxKind;
    match kind {
        McInboxKind::ProposedTool => "🔧",
        McInboxKind::ProposedCommand => "📋",
        McInboxKind::ProposedSkill => "🧠",
        McInboxKind::ProposedBrainDedup => "🧹",
    }
}

/// Icon for schedule item kinds.
fn schedule_icon(item: &McScheduleItem) -> &'static str {
    if item.awaiting_user {
        "⏸️"
    } else {
        "⏰"
    }
}

/// Render a full Mission Control snapshot as channel-friendly Markdown.
/// Pure, so it is unit-testable without a database.
pub(crate) fn render_markdown(
    a: &McAnalytics,
    activity: &[McActivity],
    inbox: &[McInboxItem],
    schedule: &[McScheduleItem],
) -> String {
    let mut s = String::new();
    let fail_pct = if a.tool_total_calls > 0 {
        (a.tool_total_fails as f64 / a.tool_total_calls as f64) * 100.0
    } else {
        0.0
    };

    // ── Header ───────────────────────────────────────────────────────
    s.push_str("🦀 *Mission Control*\n\n");

    // ── Analytics section ────────────────────────────────────────────
    s.push_str("━━ *Analytics* ━━\n");
    s.push_str(&format!(
        "Tools: {} calls, {} fails ({:.1}%)\n",
        a.tool_total_calls, a.tool_total_fails, fail_pct
    ));
    s.push_str(&format!("RSI applied: {}\n", a.rsi_applied_total));
    s.push_str(&format!(
        "Brain: {:.1} KB across {} files\n",
        a.brain_total_kb,
        a.brain_files.len()
    ));

    if !a.top_tools.is_empty() {
        s.push_str("\n*Top tools*\n");
        for t in a.top_tools.iter().take(10) {
            s.push_str(&format!(
                "• {}: {} calls ({:.1}% fail)\n",
                t.name, t.total, t.fail_rate
            ));
        }
    }

    if !a.flakiest_tools.is_empty() {
        s.push_str("\n*Flakiest (>=5 calls)*\n");
        for t in a.flakiest_tools.iter().take(8) {
            s.push_str(&format!(
                "• {}: {:.1}% fail ({} calls)\n",
                t.name, t.fail_rate, t.total
            ));
        }
    }

    if !a.rsi_top_dimensions.is_empty() {
        s.push_str("\n*RSI by dimension*\n");
        for (dim, n) in a.rsi_top_dimensions.iter().take(8) {
            s.push_str(&format!("• {dim}: {n}\n"));
        }
    }

    if !a.brain_files.is_empty() {
        s.push_str("\n*Brain files*\n");
        for f in a.brain_files.iter().take(10) {
            s.push_str(&format!("• {}: {:.1} KB\n", f.name, f.kb));
        }
    }

    // ── Inbox (RSI proposals) ────────────────────────────────────────
    if !inbox.is_empty() {
        s.push_str("\n━━ *Inbox (RSI Proposals)* ━━\n");
        for item in inbox.iter().take(5) {
            let icon = inbox_icon(&item.kind);
            s.push_str(&format!(
                "{} {}: {}\n",
                icon, item.label, item.summary
            ));
        }
    }

    // ── Activity feed ────────────────────────────────────────────────
    if !activity.is_empty() {
        s.push_str("\n━━ *Activity Feed* ━━\n");
        for entry in activity.iter().take(5) {
            let icon = activity_icon(&entry.level);
            let date = entry.timestamp.format("%Y-%m-%d");
            s.push_str(&format!("{} {} - {}\n", icon, date, entry.detail));
        }
    }

    // ── Schedule (cron jobs) ─────────────────────────────────────────
    if !schedule.is_empty() {
        s.push_str("\n━━ *Schedule* ━━\n");
        for item in schedule.iter() {
            let icon = schedule_icon(item);
            s.push_str(&format!(
                "{} {} ({})\n",
                icon, item.label, item.schedule
            ));
        }
    }

    s
}
