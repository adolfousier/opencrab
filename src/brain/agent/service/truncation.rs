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

/// Usage-gap signal (#36): the visible text is far shorter than the billed
/// non-reasoning output tokens would allow. `usage.output_tokens` includes
/// reasoning, so reasoning is subtracted first — thinking-heavy turns would
/// otherwise fake a gap on every reply. The floor is deliberately
/// conservative at 2 chars per visible token (English prose runs ~4): the
/// gap CORROBORATES a structural truncation signal and journals standalone
/// deficits, but never triggers a retry by itself — tokenizer variance and
/// terse/CJK styles make a ratio-only verdict ambiguous. Skipped entirely
/// when the provider reported no output usage.
pub(crate) fn has_usage_gap(text: &str, usage: &crate::brain::provider::TokenUsage) -> bool {
    if usage.output_tokens == 0 {
        return false;
    }
    let visible_tokens = usage.output_tokens.saturating_sub(usage.reasoning_tokens) as u64;
    if visible_tokens == 0 {
        return false;
    }
    let chars = text.trim_end().chars().count() as u64;
    chars < visible_tokens * 2
}

/// Detect a mid-sentence cut-off and inject the one-shot continuation
/// prompt into the context.
///
/// Returns `true` when the text was detected as truncated AND the
/// caller should `continue;` to the next loop iteration. Returns
/// `false` when the text reads as complete — the caller proceeds with
/// normal end-of-turn handling.
///
/// Side effects when returning `true`:
///   * `[TRUNCATION] verdict=retry` warn journal with the signal set
///     (structural, +usage_gap when the billed non-reasoning output
///     dwarfs the arrived text — #36), the last-60-char preview, and
///     the usage numbers
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
    usage: &crate::brain::provider::TokenUsage,
    context: &mut AgentContext,
    session_id: Uuid,
    progress_callback: &Option<ProgressCallback>,
) -> bool {
    let structural = looks_truncated_mid_sentence(iteration_text.trim_end());
    let usage_gap = has_usage_gap(iteration_text, usage);

    if !structural {
        // Text reads as complete. A usage gap alone never triggers a retry
        // — tokenizer variance and terse styles make it ambiguous — but the
        // journal line accumulates evidence for future tuning (#36).
        if usage_gap {
            tracing::info!(
                "[TRUNCATION] verdict=usage-gap-watch text_chars={}, usage_output={}, usage_reasoning={} — complete-looking text with a token deficit; no retry",
                iteration_text.trim_end().chars().count(),
                usage.output_tokens,
                usage.reasoning_tokens
            );
        }
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
        "[TRUNCATION] verdict=retry signals=structural{} tail={:?} text_chars={}, usage_output={}, usage_reasoning={}, usage_gap={} — asking model to continue once",
        if usage_gap { "+usage_gap" } else { "" },
        preview,
        iteration_text.trim_end().chars().count(),
        usage.output_tokens,
        usage.reasoning_tokens,
        usage_gap
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

/// Join a truncated partial with the continuation that was asked for (#859).
///
/// `final_text` is built from the LAST response only, on the assumption that
/// earlier text already reached the user as `IntermediateText`. That holds for
/// the TUI and stopped holding for Telegram once intermediates were gated to
/// deliverable rich reports (#838): a plain-prose partial is emitted, dropped
/// by the gate, and the continuation alone becomes the answer.
///
/// Observed cost: a 551-token answer was replaced by a 60-character provider
/// refusal, because the refusal was the tail of the partial AND the whole of
/// the continuation. The user saw only the refusal.
/// What a continuation attempt actually achieved.
///
/// The two outcomes used to be one `String`, which meant a continuation that
/// recovered nothing was indistinguishable from one that worked — so a turn cut
/// off mid-sentence was delivered as a finished answer (#956).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Continuation {
    /// The continuation carried the answer forward.
    Extended(String),
    /// It added nothing: the model echoed the tail it was asked to continue
    /// from, or returned nothing at all. The text is STILL truncated.
    Echoed(String),
}

/// Note appended to an answer that is still cut off after the continuation
/// failed. Telling the user is the only honest option left: the alternative is
/// presenting a sentence that stops at a colon as a completed reply.
pub(crate) const INCOMPLETE_MARKER: &str =
    "\n\n_(cut off here — the model did not continue. Ask it to finish this.)_";

pub(crate) fn join_continuation(partial: &str, continuation: &str) -> Continuation {
    let p = partial.trim_end();
    let c = continuation.trim();
    if p.is_empty() {
        return Continuation::Extended(c.to_string());
    }
    // Nothing came back, so nothing was recovered.
    if c.is_empty() {
        return Continuation::Echoed(p.to_string());
    }
    // The continuation repeated ground the partial already covers. This is the
    // reported case: the model echoed the tail it was asked to continue from.
    // Not appending it is right — but the answer is still truncated, and
    // saying so is the caller's job.
    if p.contains(c) {
        return Continuation::Echoed(p.to_string());
    }
    // The model restarted and reproduced the partial in full. That IS forward
    // progress: the restart carries the whole answer, not just the tail.
    if c.contains(p) {
        return Continuation::Extended(c.to_string());
    }
    // A genuine continuation. A separator is inserted only when neither side
    // supplies one: the cut can land mid-word, where joining with a space would
    // corrupt the word, but two clauses run together are worse to read than one
    // stray space. Same trade already made for command labels.
    let needs_space = !p.ends_with(char::is_whitespace)
        && !c.starts_with(char::is_whitespace)
        && !p.ends_with(char::is_alphanumeric);
    Continuation::Extended(if needs_space {
        format!("{p} {c}")
    } else {
        format!("{p}{c}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::agent::service::phantom::looks_truncated_mid_sentence;
    use crate::brain::provider::TokenUsage;

    fn usage(output: u32, reasoning: u32) -> TokenUsage {
        TokenUsage {
            output_tokens: output,
            reasoning_tokens: reasoning,
            ..Default::default()
        }
    }

    // ── has_usage_gap ──────────────────────────────────────────────

    #[test]
    fn usage_gap_incident_numbers() {
        // The 2026-08-28 #36 incident: 927 completion tokens billed, 457
        // of them reasoning, but only 316 chars (~80 tokens) arrived.
        let text = "x".repeat(316);
        assert!(has_usage_gap(&text, &usage(927, 457)));
    }

    #[test]
    fn usage_gap_healthy_text() {
        // ~4 chars per visible token is normal English prose — no gap.
        let text = "The deployment completed without issues.".repeat(5); // 190 chars
        assert!(!has_usage_gap(&text, &usage(50, 0)));
    }

    #[test]
    fn usage_gap_reasoning_heavy_no_false_positive() {
        // Thinking-heavy turn: almost all output is reasoning, the small
        // visible remainder matches the text. Must NOT flag.
        let text = "x".repeat(300);
        assert!(!has_usage_gap(&text, &usage(5000, 4900)));
    }

    #[test]
    fn usage_gap_no_usage_no_signal() {
        let text = "x".repeat(10);
        assert!(!has_usage_gap(&text, &usage(0, 0)));
        // All output is reasoning — nothing visible was billed.
        assert!(!has_usage_gap(&text, &usage(100, 100)));
    }

    // ── structural shapes added for #36 ────────────────────────────

    #[test]
    fn backtick_parity_incident_shape() {
        // Stream died on an OPENING backtick: exactly one backtick in the
        // whole reply (odd parity) — the #36 shape, previously read as
        // complete because a lone backtick matched no cut character.
        let truncated = "You can find the full path in the config file under `";
        assert!(looks_truncated_mid_sentence(truncated));
    }

    #[test]
    fn backtick_closed_inline_code_clean() {
        // Balanced backticks: the last one CLOSES inline code — complete.
        let complete = "To check the status of the repository you run `git status`";
        assert!(!looks_truncated_mid_sentence(complete));
    }

    #[test]
    fn unclosed_fence_truncated() {
        let text = "Here is the script you asked for:\n\n```bash\necho hello\nls -la";
        assert!(looks_truncated_mid_sentence(text));
    }

    #[test]
    fn closed_fence_clean() {
        let text = "Here is the script you asked for:\n\n```bash\necho hello\n```\nRun it and tell me what it prints.";
        assert!(!looks_truncated_mid_sentence(text));
    }

    #[test]
    fn em_dash_tail_truncated() {
        let text = "The migration finished cleanly and the only remaining step is the cutover —";
        assert!(looks_truncated_mid_sentence(text));
    }

    #[test]
    fn healthy_period_clean() {
        let text = "The migration finished cleanly and nothing else needs to be done.";
        assert!(!looks_truncated_mid_sentence(text));
    }

    // ── join_continuation semantics (#859/#956 regressions) ────────

    #[test]
    fn join_echoed_tail_recovers_nothing() {
        let partial = "The answer is forty two tokens and the reason is";
        let cont = "and the reason is";
        assert_eq!(
            join_continuation(partial, cont),
            Continuation::Echoed(partial.to_string())
        );
    }

    #[test]
    fn join_genuine_continuation_extends() {
        let partial = "The answer is forty two tokens and the reason is";
        let cont = "that the model counted every token twice.";
        match join_continuation(partial, cont) {
            Continuation::Extended(j) => {
                assert!(j.starts_with(partial));
                assert!(j.ends_with("twice."));
            }
            other => panic!("expected Extended, got {other:?}"),
        }
    }

    #[test]
    fn join_restart_reproducing_partial_is_progress() {
        let partial = "The answer is forty";
        let cont = "The answer is forty two tokens.";
        assert_eq!(
            join_continuation(partial, cont),
            Continuation::Extended(cont.to_string())
        );
    }
}
