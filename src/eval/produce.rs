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

/// Send a single user prompt to a live provider and return its text. When
/// `system` is set (e.g. the real OpenCrabs system brain), it is attached so
/// the model answers with its actual runtime context, not a bare prompt. A
/// provider error returns a marked failure string, never a silent empty.
pub async fn produce_response(
    provider: &dyn Provider,
    model: &str,
    prompt: &str,
    system: Option<&str>,
) -> String {
    let mut request = LLMRequest::new(model.to_string(), vec![Message::user(prompt.to_string())]);
    if let Some(sys) = system {
        request = request.with_system(sys);
    }
    match provider.complete(request).await {
        Ok(resp) => response_text(&resp.content),
        Err(e) => format!("[produce failed: {e}]"),
    }
}

/// Ask a live provider to compact a conversation into a continuation document.
/// When `system` is set it is attached (e.g. the real system brain). A provider
/// error returns a marked failure string (never a silent empty), so the grader
/// scores it as full fact loss rather than a spurious pass.
pub async fn produce_compaction_summary(
    provider: &dyn Provider,
    model: &str,
    conversation: &[Message],
    system: Option<&str>,
) -> String {
    let mut messages = conversation.to_vec();
    messages.push(Message::user(CONTINUATION_INSTRUCTION.to_string()));
    let mut request = LLMRequest::new(model.to_string(), messages);
    if let Some(sys) = system {
        request = request.with_system(sys);
    }
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
    system: Option<&str>,
) -> Scorecard {
    let summary =
        produce_compaction_summary(producer, producer_model, &dataset.messages(), system).await;
    dataset.judge_scorecard(judge, &summary).await
}
