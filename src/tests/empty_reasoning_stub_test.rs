//! Regression for #692: the empty-reasoning nudge must PRESERVE the model's
//! reasoning_content, not drop it.
//!
//! qwen3.8-max-preview keeps thinking always on and requires the complete
//! reasoning_content echoed back in history. When it produced a reasoning-only
//! turn, the old code added an empty assistant message and nudged — throwing the
//! reasoning away, so the model re-reasoned ~20k tokens per nudge (up to 5), the
//! 200s runaway loop. The stub now carries the reasoning as a leading Thinking
//! block (encoded back as reasoning_content).

use crate::brain::agent::service::helpers::assistant_reasoning_stub;
use crate::brain::provider::{ContentBlock, Role};

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
