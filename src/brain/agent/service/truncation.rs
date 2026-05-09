//! Truncated-mid-sentence response detection and one-shot continuation.
//!
//! Local reasoning models (notably Qwen3.6-35B on MLX) periodically emit
//! an EOS token mid-sentence — the response looks complete from a
//! protocol standpoint (proper `finish_reason=stop` + usage chunk) but
//! the visible text ends mid-word. This module owns the decision to
//! ask the model to continue once.
//!
//! Extracted from `tool_loop.rs` (was lines 2800-2866) as part of the
//! 2026-05-04 Linor-flagged refactor: tool_loop.rs was 4,047 lines.
//! Behaviour is unchanged from the pre-extraction version. The
//! detection heuristic itself lives in `phantom::looks_truncated_mid_
//! sentence` — that boundary already existed.
//!
//! Coupling with bug-B (commit 03d0524e):
//! `current_iter_is_truncation_continue` is set by the caller AFTER
//! this returns true, so the stream-error path on the NEXT iteration
//! skips cross-provider fallback. We don't try to bundle that flag
//! into this module because it lives on the loop's own state and only
//! has meaning relative to the next stream attempt.

use super::phantom::looks_truncated_mid_sentence;
use super::types::{ProgressCallback, ProgressEvent};
use crate::brain::agent::context::AgentContext;
use crate::brain::provider::Message;
use uuid::Uuid;

/// Detect a mid-sentence cut-off and inject the one-shot continuation
/// prompt into the context.
///
/// Returns `true` when the text was detected as truncated AND the
/// caller should `continue;` to the next loop iteration. Returns
/// `false` when the text reads as complete — the caller proceeds with
/// normal end-of-turn handling.
///
/// Side effects when returning `true`:
///   * `tracing::warn!` with the last-60-char preview for debugging
///   * `progress_callback` fires `SelfHealingAlert` + `IntermediateText`
///     so the user sees the partial reply AND a nudge that we're
///     asking for continuation
///   * Two messages appended to `context`: the partial assistant reply
///     (so it's visible AND part of context), then a system-style user
///     message instructing the model to continue from where it left off
///     without restarting or re-planning
///
/// Caller is responsible for the gating preconditions (one-shot guard,
/// CLI-provider exclusion, `iteration > 0`, `StopReason::EndTurn`)
/// because those depend on the surrounding loop state and would just
/// be more parameters here without making the function clearer.
pub(super) fn try_emit_truncation_continue(
    iteration_text: &str,
    reasoning_text: Option<&String>,
    context: &mut AgentContext,
    session_id: Uuid,
    progress_callback: &Option<ProgressCallback>,
) -> bool {
    if !looks_truncated_mid_sentence(iteration_text.trim_end()) {
        return false;
    }

    let preview: String = iteration_text
        .chars()
        .rev()
        .take(60)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    tracing::warn!(
        "Response ended with finish_reason=stop but last chars look \
         mid-sentence (tail={:?}) — asking model to continue once.",
        preview,
    );

    if let Some(cb) = progress_callback {
        cb(
            session_id,
            ProgressEvent::SelfHealingAlert {
                message: "Response was cut off mid-sentence — asking model to continue".into(),
            },
        );
    }
    // Keep the partial as a real intermediate message so the user sees
    // what DID arrive, then nudge continuation.
    if !iteration_text.is_empty()
        && let Some(cb) = progress_callback
    {
        cb(
            session_id,
            ProgressEvent::IntermediateText {
                text: iteration_text.to_string(),
                reasoning: reasoning_text.cloned(),
            },
        );
    }

    context.add_message(Message::assistant(iteration_text.to_string()));
    context.add_message(Message::user(
        "[System: Your previous reply was cut off mid-sentence (no terminal \
         punctuation). Continue from exactly where you left off — do NOT repeat \
         what you already wrote, do NOT restart the answer, do NOT re-plan. \
         Just keep writing.]"
            .to_string(),
    ));

    true
}
