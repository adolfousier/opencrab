//! Guard that prevents RSI from appending raw failure-event logs to brain
//! files (issue #111). The function under test is private to
//! `brain::tools::self_improve`; we exercise it via the public
//! `self_improve` tool's `apply`/`update` actions, asserting that the
//! known bad-shape headers come back as errors and clean derived rules
//! pass through.

use crate::brain::tools::self_improve::SelfImproveTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

#[tokio::test]
async fn rejects_failure_count_in_section_header() {
    let tool = SelfImproveTool;
    let result = tool
        .execute(
            json!({
                "action": "apply",
                "target_file": "TOOLS.md",
                "description": "log",
                "rationale": "log",
                "content": "### Bash Exit Code 127 — Recurring (6 failures since 2026-05-17)\n\nA failure log.",
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(
        !result.success,
        "failure-count header should be rejected, got success: {}",
        result.output,
    );
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("derived RULES") || err.contains("audit-log"),
        "rejection message should explain the rule vs log distinction, got: {err}",
    );
}

#[tokio::test]
async fn rejects_recurring_failures_header() {
    let tool = SelfImproveTool;
    let result = tool
        .execute(
            json!({
                "action": "apply",
                "target_file": "TOOLS.md",
                "description": "log",
                "rationale": "log",
                "content": "## tg_get_messages — Recurring failures across multiple sessions\n\nlog body",
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(
        !result.success,
        "recurring+failures header should bounce, got success: {}",
        result.output,
    );
}

#[tokio::test]
async fn rejects_failures_count_in_update_content() {
    // Need TOOLS.md to exist for 'update' to even attempt; the guard runs
    // before the file-read so an inline body with a bad header still
    // bounces regardless of file state.
    let tool = SelfImproveTool;
    let result = tool
        .execute(
            json!({
                "action": "update",
                "target_file": "TOOLS.md",
                "description": "log",
                "rationale": "log",
                "old_content": "irrelevant",
                "content": "### Foo (3 failures: 2026-05-22T16:27, 17:01, 17:42)",
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(
        !result.success,
        "update path should also reject failure-log content, got success: {}",
        result.output,
    );
}
