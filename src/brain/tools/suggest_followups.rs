//! `suggest_followups` tool.
//!
//! Lets the agent surface OPTIONAL next-step suggestions the user may accept or
//! ignore. Unlike `follow_up_question` this is non-blocking: it fires a
//! `ProgressEvent::SuggestedFollowups` and returns immediately without awaiting
//! any answer. Each surface renders the options as its own INTERACTIVE UI —
//! tap-to-send buttons under the reply on chat channels (Telegram/Discord/…), a
//! pick-list or gray ghost-text accept in the TUI. The rendering is the tool's
//! job; the model must NOT write the suggestions as plain text in its reply, or
//! they land as dead text with no button to tap.
//!
//! Intended for "here's a likely next thing you might ask" — a convenience, not
//! a question. If the agent genuinely cannot proceed without a choice, it should
//! use `follow_up_question` (blocking) instead.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::agent::ProgressEvent;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

/// Hard cap on suggestions. One renders as ghost text; a few render as a
/// pick-list. More than a handful is noise the user won't read.
pub const MAX_SUGGESTIONS: usize = 4;

pub struct SuggestFollowupsTool;

#[derive(Debug, Deserialize)]
struct SuggestInput {
    options: Vec<String>,
}

#[async_trait]
impl Tool for SuggestFollowupsTool {
    fn name(&self) -> &str {
        "suggest_followups"
    }

    fn description(&self) -> &str {
        "Optionally surface 1-4 short follow-up messages the user might send next. \
         Non-blocking and CHANNEL-AGNOSTIC: each surface renders the options as \
         interactive UI — tap-to-send buttons under your reply on chat channels \
         (Telegram/Discord/…), a pick-list or gray ghost-text accept in the TUI — \
         and the user may accept, edit, or ignore them. You MUST call the tool to \
         make the options interactive: writing them as plain text in your reply \
         leaves dead text with no button to tap. Call this at the END of a turn \
         when there is an obvious next step (1 option) or a small set of distinct \
         next directions (2-4 options). Each option must be a complete, \
         ready-to-send user message phrased in the user's voice (e.g. \"Add tests \
         for the new endpoint\", not \"I could add tests\"). Keep each under ~60 \
         chars. Do NOT use this to ask a question you need answered to proceed (use \
         follow_up_question), and do not also repeat the options in your prose."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": MAX_SUGGESTIONS,
                    "description": "1 to 4 distinct, ready-to-send follow-up messages in the user's voice. Rendered as interactive UI on every surface (tap-to-send buttons on chat channels; ghost-text/pick-list in the TUI) — never as plain text."
                }
            },
            "required": ["options"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Pure UI signal. No filesystem, shell, or network.
        vec![]
    }

    fn requires_approval(&self) -> bool {
        // The tool IS a passive UI hint — nothing to approve.
        false
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let parsed: SuggestInput = serde_json::from_value(input)?;

        let options = sanitize_options(parsed.options);
        if let Err(msg) = options {
            return Ok(ToolResult::error(msg));
        }
        let options = options.expect("checked Ok above");

        // Non-blocking: fire the event and return. Surfaces without a progress
        // bridge (channels, A2A) simply don't render it — not an error, the
        // suggestions are always optional.
        let count = options.len();
        if let Some(cb) = context.progress_callback.as_ref() {
            cb(
                context.session_id,
                ProgressEvent::SuggestedFollowups(options),
            );
        }

        Ok(ToolResult::success(format!(
            "Surfaced {count} follow-up suggestion(s)."
        )))
    }
}

/// Trim, drop empties, enforce the 1..=MAX distinct contract. Extracted so the
/// validation is unit-testable without a live progress callback.
pub(crate) fn sanitize_options(raw: Vec<String>) -> std::result::Result<Vec<String>, String> {
    let options: Vec<String> = raw
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();

    if options.is_empty() {
        return Err("suggest_followups needs at least 1 non-empty option.".into());
    }
    if options.len() > MAX_SUGGESTIONS {
        return Err(format!(
            "Too many suggestions ({}). Cap is {}.",
            options.len(),
            MAX_SUGGESTIONS
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for opt in &options {
        if !seen.insert(opt.as_str()) {
            return Err(format!(
                "Duplicate suggestion '{opt}'. Suggestions must be distinct."
            ));
        }
    }
    Ok(options)
}
