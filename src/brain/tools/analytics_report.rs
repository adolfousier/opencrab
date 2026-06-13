//! Analytics Report Tool
//!
//! Renders this instance's analytics (brain sizes, tool usage and failure
//! rates, RSI applications) as a Markdown report the agent can send through
//! whatever channel it is on. Same data as the Mission Control Analytics panel
//! and the external opencrabs-analytics HTML tool (discussion #178), built
//! natively from the `tool_executions` / `feedback_ledger` tables and brain
//! `.md` sizes. No secrets, no message content, nothing leaves the machine
//! unless the agent chooses to send the report.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::mission_control::{McAnalytics, analytics_service};
use crate::db::Pool;
use async_trait::async_trait;
use serde_json::Value;

/// Generates a shareable analytics report from local stats.
pub struct AnalyticsReportTool {
    pool: Pool,
}

impl AnalyticsReportTool {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for AnalyticsReportTool {
    fn name(&self) -> &str {
        "analytics_report"
    }

    fn description(&self) -> &str {
        "Generate a shareable analytics report of this OpenCrabs instance: tool usage and \
         failure rates, RSI improvements applied, and brain file sizes. Returns Markdown you \
         can send straight to a chat. Reads only aggregate stats from the local database and \
         brain file sizes, never message content or secrets."
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
        Ok(ToolResult::success(render_markdown(&analytics)))
    }
}

/// Render an [`McAnalytics`] snapshot as channel-friendly Markdown. Pure, so it
/// is unit-testable without a database.
pub(crate) fn render_markdown(a: &McAnalytics) -> String {
    let mut s = String::new();
    let fail_pct = if a.tool_total_calls > 0 {
        (a.tool_total_fails as f64 / a.tool_total_calls as f64) * 100.0
    } else {
        0.0
    };

    s.push_str("🦀 *OpenCrabs Analytics*\n\n");
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
        s.push_str("\n*Flakiest (≥5 calls)*\n");
        for t in a.flakiest_tools.iter().take(8) {
            s.push_str(&format!(
                "• {}: {:.1}% fail ({} calls)\n",
                t.name, t.fail_rate, t.total
            ));
        }
    }

    if !a.rsi_top_dimensions.is_empty() {
        s.push_str("\n*RSI applied by dimension*\n");
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

    s
}
