//! Context-budget enforcement (Tier 1 + Tier 2 compaction).
//!
//! Extracted from `tool_loop.rs` (was lines 134-330) as part of the
//! 2026-05-04 Linor-flagged refactor: `tool_loop.rs` was 4,047 lines.
//! Compaction logic is cohesive — one async method that decides between
//! the soft 65 % LLM-summarisation tier, the 90 % hard-truncate floor,
//! and the safety-net truncation when all attempts fail. Lives next to
//! the rest of `impl AgentService` in the same crate; callers still
//! invoke it as `self.enforce_context_budget(...)` exactly as before.
//!
//! Behaviour is unchanged from the pre-extraction version. The
//! exhaustive comments inside the function were preserved verbatim
//! because they document the regression history (pre-0f052250 shape,
//! cancellation race details, the failed async-spawn variant).

use super::builder::AgentService;
use super::types::{ProgressCallback, ProgressEvent};
use crate::brain::agent::context::AgentContext;
use uuid::Uuid;

impl AgentService {
    /// Enforce context budget with non-blocking compaction.
    ///
    /// Tier 1 — soft trigger at 65%: spawns an async LLM compaction task in
    /// the background and returns immediately. The agent keeps processing
    /// turns. Subsequent visits to this function check whether the spawned
    /// task has finished and atomically swap the summary in when it has.
    ///
    /// Tier 2 — hard floor at 90%: if context grows past 90% (because growth
    /// outran compaction or compaction failed), emergency truncation cuts
    /// older messages back to 80%. This path NEVER fails. It also cancels
    /// any in-flight async compaction so a stale snapshot summary cannot
    /// later overwrite the now-truncated context.
    ///
    /// NOTE: 65% (~130k of 200k) is chosen because MiniMax (and likely other
    /// providers) start returning `400 Prompt exceeds max length` well below
    /// the documented limit, around 75-80% in practice. 65% gives enough
    /// headroom to summarise without bumping into the actual ceiling.
    ///
    /// Returns `Some(summary)` only on the visit where a previously-spawned
    /// async task finished AND its summary was applied; otherwise `None`.
    pub(super) async fn enforce_context_budget(
        &self,
        session_id: Uuid,
        context: &mut AgentContext,
        model_name: &str,
        cancel_token: Option<&tokio_util::sync::CancellationToken>,
        progress_callback: &Option<ProgressCallback>,
    ) -> Option<String> {
        // Restored to the pre-0f052250 shape (the version that ran fine for
        // months before the async-compaction refactor). Logic, in order:
        //
        //   Tier 2 (90% hard floor): truncate to 80% first, then FALL THROUGH
        //     to Tier 1. Doing the truncation first means the compaction
        //     summarizer below sees ≤80% of the window — well within tokenizer
        //     headroom — so it doesn't hit `400 Prompt exceeds max length`
        //     and there's no failed-summarizer-then-truncate cascade.
        //
        //   Tier 1 (65% soft trigger): up to 3 sync compact_context attempts.
        //     If any succeed, summary lands and the marker gets persisted by
        //     the caller. If still over 65% target after success, re-compact
        //     once more with the now-tighter budget.
        //
        //   Safety net: only if all 3 attempts totally failed AND we're still
        //     above 80%, hard-truncate to 80%. This is the LAST RESORT — it
        //     drops messages without a summary marker, but only fires when
        //     the LLM compaction path is entirely unavailable.
        //
        // No async spawn/swap, no cancel-pending-on-90%, no per-call hard-
        // truncate fallback in the error arm — those were the additions that
        // produced the cascade-and-loop behaviour.
        let effective_max = context.max_tokens;
        let usage_pct = if effective_max > 0 {
            (context.token_count as f64 / effective_max as f64) * 100.0
        } else {
            100.0
        };

        tracing::debug!(
            "Context budget: {} tokens / {} max = {:.1}%",
            context.token_count,
            effective_max,
            usage_pct,
        );

        // ── Tier 2: 90% hard floor — truncate to 80%, then fall through to Tier 1 compaction ──
        if usage_pct >= 90.0 {
            tracing::warn!(
                "Context at {:.0}% ({} tokens) — hard truncating to 80%",
                usage_pct,
                context.token_count,
            );

            let target = (effective_max as f64 * 0.80) as usize;
            context.hard_truncate_to(target);
            context.trim_to_fit(0);

            if let Some(cb) = progress_callback {
                cb(session_id, ProgressEvent::TokenCount(context.token_count));
            }

            tracing::info!(
                "Hard truncation complete: {} messages, {} tokens ({:.0}%)",
                context.messages.len(),
                context.token_count,
                context.token_count as f64 / effective_max as f64 * 100.0,
            );

            let usage_pct_now = if effective_max > 0 {
                (context.token_count as f64 / effective_max as f64) * 100.0
            } else {
                100.0
            };
            tracing::debug!(
                "Post-truncation: {:.0}% — falling through to auto-compaction",
                usage_pct_now,
            );
        }

        // ── Tier 1: soft trigger at 65% — LLM compaction ──
        let usage_pct = if effective_max > 0 {
            (context.token_count as f64 / effective_max as f64) * 100.0
        } else {
            100.0
        };
        if usage_pct <= 65.0 {
            return None;
        }

        tracing::warn!(
            "Context at {:.0}% (>65%) — triggering LLM compaction",
            usage_pct
        );
        self.record_provider_feedback(
            session_id,
            "context_compaction",
            model_name,
            Some(&format!("proactive_65pct tokens={}", context.token_count)),
        );

        // Signal channels (Telegram especially — no continuous typing
        // indicator like the TUI spinner) that we're still working. The
        // LLM compact_context call below can run 10-60s with zero
        // streaming chunks; without this event the user sees nothing
        // happening and assumes the request was dropped (2026-05-19
        // Telegram report on a long heyiolo session — request finished
        // fine, just no signal during the silent window).
        if let Some(cb) = progress_callback {
            cb(session_id, ProgressEvent::Compacting);
        }

        // Up to 3 attempts — transient summarizer errors (network blip,
        // tokenizer-edge 400) usually self-resolve on retry.
        let mut summary_result = None;
        const MAX_ATTEMPTS: u32 = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            match self
                .compact_context(session_id, context, model_name, cancel_token)
                .await
            {
                Ok(summary) => {
                    summary_result = Some(summary);
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        "LLM compaction failed (attempt {}/{}): {}",
                        attempt,
                        MAX_ATTEMPTS,
                        e
                    );
                }
            }
        }

        // If still over the 65% target after a successful compaction, run one
        // more pass with the tighter post-summary budget.
        let target_tokens = (effective_max as f64 * 0.65) as usize;
        if context.token_count > target_tokens && summary_result.is_some() {
            tracing::warn!(
                "Still at {} tokens after compaction (target {}), re-compacting",
                context.token_count,
                target_tokens,
            );
            if let Ok(summary) = self
                .compact_context(session_id, context, model_name, cancel_token)
                .await
            {
                summary_result = Some(summary);
            }
        }

        // Last resort: every compaction attempt failed AND we're still over
        // 80%. Truncate to keep the next request from going out at 200%+. No
        // marker is persisted in this branch; the caller sees None back.
        if summary_result.is_none() {
            let safety_target = (effective_max as f64 * 0.80) as usize;
            if context.token_count > safety_target {
                tracing::warn!(
                    "Compaction exhausted, context at {} tokens (>{:.0}%) — safety truncation to 80%",
                    context.token_count,
                    usage_pct,
                );
                context.hard_truncate_to(safety_target);
                context.trim_to_fit(0);
            }
        }

        // Emit the token count the NEXT request will start with.
        if let Some(cb) = progress_callback {
            if let Some(ref summary) = summary_result {
                let marker_tokens = AgentContext::estimate_tokens(summary) + 100;
                let brain_tokens = self
                    .default_system_brain
                    .as_deref()
                    .map(AgentContext::estimate_tokens)
                    .unwrap_or(0);
                cb(
                    session_id,
                    ProgressEvent::TokenCount(marker_tokens + brain_tokens),
                );
            } else {
                cb(session_id, ProgressEvent::TokenCount(context.token_count));
            }
        }

        summary_result
    }
}
