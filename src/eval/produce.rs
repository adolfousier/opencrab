//! Live artifact producers (live-L2, #631).
//!
//! Produces a REAL artifact from a live provider so an eval grades real model
//! output rather than a fixture. [`produce_compaction_summary`] asks a provider
//! to compact a conversation into a continuation document, then the compaction
//! dataset's probes grade what survived.
//!
//! The continuation-document instruction mirrors the SECTION STRUCTURE of the
//! production compaction prompt (agent/service/context.rs) — immediate task,
//! files modified, user preferences, errors, pending tasks — without its live
//! token-budget preamble, which is meaningless for a fixed dataset. Kept
//! self-contained here so the production compaction path is untouched; sharing
//! the exact prompt is a possible later refinement.

use crate::brain::provider::{ContentBlock, LLMRequest, Message, Provider};

use super::compaction::CompactionDataset;
use super::scorer::{Judge, Scorecard};

/// Instruction appended to a conversation asking the model to compact it into a
/// fact-preserving continuation document.
pub const CONTINUATION_INSTRUCTION: &str = "The conversation must be compacted now. Produce a \
     COMPREHENSIVE CONTINUATION DOCUMENT so a fresh agent can resume with only this summary. \
     Analyze the entire conversation and preserve, with exact detail:\n\
     ## Immediate task — the user's last instruction and the exact next action.\n\
     ## Files modified — every file created/edited/read, with paths and what changed.\n\
     ## User preferences & constraints — everything the user said to do or never do.\n\
     ## Errors & corrections — every error message and how it was resolved.\n\
     ## Pending tasks — everything not yet done.\n\
     Preserve exact identifiers (file paths, error codes, commands) verbatim.";

/// Extract the concatenated text of a provider response.
fn response_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Ask a live provider to compact a conversation into a continuation document.
/// A provider error returns a marked failure string (never a silent empty), so
/// the grader scores it as full fact loss rather than a spurious pass.
pub async fn produce_compaction_summary(
    provider: &dyn Provider,
    model: &str,
    conversation: &[Message],
) -> String {
    let mut messages = conversation.to_vec();
    messages.push(Message::user(CONTINUATION_INSTRUCTION.to_string()));
    let request = LLMRequest::new(model.to_string(), messages);
    match provider.complete(request).await {
        Ok(resp) => response_text(&resp.content),
        Err(e) => format!("[compaction failed: {e}]"),
    }
}

/// End-to-end: produce a real compaction summary with `producer`, then grade the
/// dataset's probes against it with `judge`.
pub async fn run_compaction_eval(
    producer: &dyn Provider,
    producer_model: &str,
    judge: &dyn Judge,
    dataset: &CompactionDataset,
) -> Scorecard {
    let summary = produce_compaction_summary(producer, producer_model, &dataset.messages()).await;
    dataset.judge_scorecard(judge, &summary).await
}
