//! Regression for #692: the empty-reasoning nudge must PRESERVE the model's
//! reasoning_content, not drop it.
//!
//! qwen3.8-max-preview keeps thinking always on and requires the complete
//! reasoning_content echoed back in history. When it produced a reasoning-only
//! turn, the old code added an empty assistant message and nudged — throwing the
//! reasoning away, so the model re-reasoned ~20k tokens per nudge (up to 5), the
//! 200s runaway loop. The stub now carries the reasoning as a leading Thinking
//! block (encoded back as reasoning_content).

use crate::brain::agent::service::helpers::{assistant_reasoning_stub, empty_reasoning_nudge};
use crate::brain::provider::{ContentBlock, Role};

#[test]
fn no_tools_yet_nudge_encourages_a_tool_call_never_suppresses_it() {
    // #692 follow-up: when no tool has run this turn, the nudge must push the
    // model to CALL the tool it needs — never tell it to avoid tools (which left
    // qwen3.8-max-preview narrating with no way to act).
    for attempt in 1..=5 {
        let n = empty_reasoning_nudge(true, attempt).to_lowercase();
        assert!(
            n.contains("tool"),
            "no-tools nudge must reference calling a tool (attempt {attempt}): {n}"
        );
        assert!(
            !n.contains("do not call more tools")
                && !n.contains("no tool calls")
                && !n.contains("tool results above are sufficient"),
            "no-tools nudge must NOT suppress tool calls (attempt {attempt}): {n}"
        );
    }
}

#[test]
fn tools_ran_nudge_steers_to_writing_the_answer() {
    // Once a tool HAS run, steering toward the answer is correct.
    let n = empty_reasoning_nudge(false, 1).to_lowercase();
    assert!(n.contains("tool results above are sufficient") || n.contains("write the answer"));
}

#[test]
fn stub_carries_reasoning_as_thinking_block() {
    let msg = assistant_reasoning_stub(Some("The user wants the pricing table. I have the data."))
        .expect("reasoning present, so a stub must be produced");
    assert_eq!(msg.role, Role::Assistant);
    match msg.content.first() {
        Some(ContentBlock::Thinking { thinking, .. }) => {
            assert!(thinking.contains("pricing table"));
        }
        other => panic!("expected a leading Thinking block, got {other:?}"),
    }
}

#[test]
fn nothing_is_appended_when_there_is_no_reasoning() {
    // Was a bare empty assistant message. Harmless while only one nudge could
    // fire; destructive once the escalation reached 5/5, because five
    // `[empty assistant] [nudge]` pairs accumulated on the context and that
    // same context was handed to every fallback, so all of them returned
    // nothing (#979). With no reasoning there is nothing to preserve, so the
    // caller must append no message at all.
    for reasoning in [None, Some(""), Some("   \n  ")] {
        assert!(
            assistant_reasoning_stub(reasoning).is_none(),
            "must append nothing when reasoning is absent/blank: {reasoning:?}"
        );
    }
}

// ── Fallback context (#979) ──────────────────────────────────────────────────

use crate::brain::agent::service::helpers::fallback_messages;
use crate::brain::provider::Message;

fn convo(n: usize) -> Vec<Message> {
    (0..n).map(|i| Message::user(format!("m{i}"))).collect()
}

#[test]
fn a_fallback_gets_the_conversation_from_before_the_nudging() {
    // 3 real messages, then 4 appended by the nudge escalation.
    let messages = convo(7);
    let trimmed = fallback_messages(Some(3), &messages);
    assert_eq!(trimmed.len(), 3, "scaffolding must be dropped");
}

#[test]
fn no_boundary_means_no_trimming() {
    // No nudge ever fired, so there is nothing to strip and the full
    // conversation must survive.
    let messages = convo(5);
    assert_eq!(fallback_messages(None, &messages).len(), 5);
}

#[test]
fn an_out_of_range_boundary_never_loses_history() {
    // Defensive: a stale or impossible marker must not truncate to garbage.
    let messages = convo(4);
    assert_eq!(fallback_messages(Some(99), &messages).len(), 4);
}

#[test]
fn a_boundary_at_the_end_is_a_no_op() {
    let messages = convo(4);
    assert_eq!(fallback_messages(Some(4), &messages).len(), 4);
}

#[test]
fn a_zero_boundary_yields_an_empty_conversation() {
    // Only reachable if nudging began before any message existed, which is not
    // a real state, but the arithmetic must still be exact rather than clamped.
    let messages = convo(3);
    assert!(fallback_messages(Some(0), &messages).is_empty());
}
