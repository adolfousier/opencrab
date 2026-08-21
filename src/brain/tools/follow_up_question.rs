//! `follow_up_question` tool.
//!
//! Lets the agent ask the user a discrete-choice question mid-task and
//! block until the user picks one of the provided options. Channel
//! handlers wire a `QuestionCallback` (Telegram inline keyboard,
//! Discord components, Slack actions, TUI overlay, WhatsApp numbered
//! text) that renders the question, suspends on a oneshot, and returns
//! the chosen option string.
//!
//! Intended for "I cannot proceed without picking one of these" cases.
//! Not for general open-ended questions — the prompt should steer the
//! agent toward typing a question in natural language for those.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::agent::FollowUpQuestionInfo;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

/// Hard cap on number of options. Telegram inline keyboards stack
/// vertically, Discord components allow 25 per row but degrade UX
/// well before that, TUI overlays get unreadable past ~8. Keep it
/// tight — if the agent needs more than 8 it should narrow the
/// question first.
pub const MAX_OPTIONS: usize = 8;

pub struct FollowUpQuestionTool;

#[derive(Debug, Deserialize)]
struct FollowUpInput {
    question: String,
    options: Vec<String>,
}

#[async_trait]
impl Tool for FollowUpQuestionTool {
    fn name(&self) -> &str {
        "follow_up_question"
    }

    fn description(&self) -> &str {
        "Ask the user a discrete-choice question with up to 8 button options. \
         Use this ONLY when you cannot proceed without the user picking from a short \
         list of specific values (e.g. \"which file did you mean?\", \"target environment?\"). \
         Do not use it for general questions, confirmations (use the normal approval flow), \
         or anything you could resolve yourself by reading code or running a tool. Returns \
         the chosen option string. Call this tool silently. Do not repeat the question or \
         options in surrounding prose."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to display above the option buttons. Keep it under 200 chars.",
                    "maxLength": 500
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 2,
                    "maxItems": MAX_OPTIONS,
                    "description": "Between 2 and 8 distinct option strings. Each becomes one clickable button. Recommended under 40 chars for clean rendering on all channels."
                }
            },
            "required": ["question", "options"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Pure user-interaction. No filesystem, shell, or network.
        vec![]
    }

    fn requires_approval(&self) -> bool {
        // The tool IS the user-interaction surface — gating it behind
        // an approval prompt would be silly.
        false
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let parsed: FollowUpInput = serde_json::from_value(input)?;

        let question = parsed.question.trim();
        if question.is_empty() {
            return Ok(ToolResult::error(
                "follow_up_question requires a non-empty question.".into(),
            ));
        }

        let options: Vec<String> = parsed
            .options
            .into_iter()
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect();
        if options.len() < 2 {
            return Ok(ToolResult::error(
                "follow_up_question needs at least 2 non-empty options. If you only have one \
                 option, just do it instead of asking."
                    .into(),
            ));
        }
        if options.len() > MAX_OPTIONS {
            return Ok(ToolResult::error(format!(
                "Too many options ({}). Cap is {}. Narrow the question.",
                options.len(),
                MAX_OPTIONS
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for opt in &options {
            if !seen.insert(opt.as_str()) {
                return Ok(ToolResult::error(format!(
                    "Duplicate option '{}'. Options must be distinct.",
                    opt
                )));
            }
        }

        let cb = match context.question_callback.as_ref() {
            Some(c) => c.clone(),
            None => {
                // No interactive surface (cron, webhook, A2A): degrade gracefully
                // (#716) instead of hard-erroring. Hand the agent the question as
                // plain text so it relays it in its reply and doesn't burn the
                // call on an error it then has to work around every time.
                return Ok(ToolResult::success(render_plaintext_question(
                    question, &options,
                )));
            }
        };

        let info = FollowUpQuestionInfo {
            session_id: context.session_id,
            question: question.to_string(),
            options: options.clone(),
        };

        match cb(info).await {
            Ok(answer) => Ok(ToolResult::success(format!("User chose: {}", answer))),
            Err(e) => Ok(ToolResult::error(format!(
                "follow_up_question failed: {}",
                e
            ))),
        }
    }
}

impl FollowUpQuestionInfo {
    /// Compact over-long option labels for channels with cramped button
    /// rendering (#1143).
    ///
    /// If ANY option exceeds `threshold` chars, the full option texts are
    /// folded into the question body as a numbered list and the option
    /// labels are replaced with `"1"`..`"N"`. Short label sets pass through
    /// unchanged (identity).
    ///
    /// Answer resolution is index-based (`q:{id}:{idx}` callback data →
    /// `options[idx]` in the channel's pending-question map), so a caller
    /// that clones the ORIGINAL options before calling this and registers
    /// the clone still delivers the real text to the model on tap — never
    /// a bare number. Channel renderers do exactly that.
    pub fn compact_options(mut self, threshold: usize) -> Self {
        if self.options.iter().any(|o| o.chars().count() > threshold) {
            let mut question = self.question;
            question.push_str("\n\n");
            for (i, opt) in self.options.iter().enumerate() {
                question.push_str(&format!("\n{}. {}", i + 1, opt));
            }
            self.question = question;
            self.options = (1..=self.options.len()).map(|i| i.to_string()).collect();
        }
        self
    }
}

/// Render a question + numbered options as plain text for surfaces with no
/// interactive callback (cron, webhook, A2A). The agent relays this in its reply
/// instead of the tool hard-erroring (#716). Extracted for direct testing.
pub(crate) fn render_plaintext_question(question: &str, options: &[String]) -> String {
    let mut out = String::from(
        "No interactive buttons on this surface. Ask the user this question in plain text and \
         wait for their reply:\n\n",
    );
    out.push_str(question);
    out.push('\n');
    for (i, opt) in options.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, opt));
    }
    out.trim_end().to_string()
}
