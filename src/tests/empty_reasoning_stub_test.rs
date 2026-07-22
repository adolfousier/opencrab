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
    let msg = assistant_reasoning_stub(Some("The user wants the pricing table. I have the data."));
    assert_eq!(msg.role, Role::Assistant);
    match msg.content.first() {
        Some(ContentBlock::Thinking { thinking, .. }) => {
            assert!(thinking.contains("pricing table"));
        }
        other => panic!("expected a leading Thinking block, got {other:?}"),
    }
}

#[test]
fn stub_is_empty_when_no_reasoning() {
    // No reasoning to preserve -> a bare empty assistant message.
    for reasoning in [None, Some(""), Some("   \n  ")] {
        let msg = assistant_reasoning_stub(reasoning);
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, ContentBlock::Thinking { .. })),
            "no Thinking block when reasoning is absent/blank: {reasoning:?}"
        );
    }
}
