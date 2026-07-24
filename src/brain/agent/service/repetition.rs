//! Recovery from a repetitive-tool-call poisoned history (#740).
//!
//! When an agent makes the same tool call (identical name + arguments) across
//! many consecutive rounds — e.g. polling a down endpoint — the conversation
//! history fills with identical `tool_use`/`tool_result` pairs. Some providers
//! run a server-side loop guardrail that then rejects EVERY subsequent request
//! with an HTTP 500 (`Repetitive tool calls detected ...`), permanently bricking
//! the session: each new message re-sends the poisoned history and 500s again,
//! with no recovery short of a manual DB delete.
//!
//! These pure helpers detect that error and collapse consecutive identical tool
//! rounds so the caller can retry with a healed history.

use crate::brain::provider::{ContentBlock, Message};

/// Whether a provider error message is the server-side repetitive-tool-call
/// guardrail (as opposed to an unrelated 500). Delegates to the single matcher
/// in the provider layer so `is_retryable` and this recovery never drift.
pub fn is_repetitive_tool_error(msg: &str) -> bool {
    crate::brain::provider::error::is_repetitive_tool_guardrail(msg)
}

/// Signature of a message that is PURELY a tool call (or parallel tool calls) —
/// `name|args` per tool, joined. Returns `None` when the message carries real
/// text (so a message that both narrates and calls a tool is never collapsed)
/// or contains no tool call at all.
fn tool_use_signature(m: &Message) -> Option<String> {
    let mut sigs = Vec::new();
    for b in &m.content {
        match b {
            ContentBlock::ToolUse { name, input, .. } => sigs.push(format!("{name}|{input}")),
            // A non-empty text block means the round said something worth
            // keeping — do not collapse it even if it also called a tool.
            ContentBlock::Text { text } if !text.trim().is_empty() => return None,
            _ => {}
        }
    }
    (!sigs.is_empty()).then(|| sigs.join("\n"))
}

/// Whether a message carries a tool result (the paired reply to a tool call).
fn is_tool_result(m: &Message) -> bool {
    m.content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
}

/// Collapse runs of CONSECUTIVE identical tool rounds to a single round,
/// dropping the duplicate `tool_use` messages and their paired `tool_result`
/// messages while keeping the first occurrence (#740). Non-consecutive repeats
/// (a different message in between) are left alone — the provider guardrail
/// only fires on consecutive rounds. Returns `(pruned messages, removed count)`.
pub fn prune_repetitive_tool_calls(messages: &[Message]) -> (Vec<Message>, usize) {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut removed = 0usize;
    let mut last_sig: Option<String> = None;
    let mut i = 0;
    while i < messages.len() {
        let m = &messages[i];
        if let Some(sig) = tool_use_signature(m) {
            if last_sig.as_deref() == Some(sig.as_str()) {
                // Consecutive duplicate round: drop this tool_use message and
                // its immediately-following tool_result, keeping the first.
                removed += 1;
                i += 1;
                if i < messages.len() && is_tool_result(&messages[i]) {
                    removed += 1;
                    i += 1;
                }
                continue;
            }
            last_sig = Some(sig);
            out.push(m.clone());
        } else {
            // A tool_result belongs to the kept round and must NOT reset the
            // run (the next duplicate tool_use still needs to see `last_sig`).
            // Any other message (user/assistant text) ends the run.
            if !is_tool_result(m) {
                last_sig = None;
            }
            out.push(m.clone());
        }
        i += 1;
    }
    (out, removed)
}
