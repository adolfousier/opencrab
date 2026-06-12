//! The Xiaomi MiMo system-prompt nudge: mimo narrates tool calls in prose or
//! emits them as `<tool_call_list>` text instead of using the structured field,
//! so we append a reminder up front (the phantom self-heal and the
//! `<tool_call_list>` parser are the after-the-fact safety nets).

use crate::brain::agent::service::helpers::{MIMO_TOOL_CALL_HINT, is_mimo_model};

#[test]
fn detects_mimo_models_case_insensitively() {
    assert!(is_mimo_model("mimo-v2.5-pro"));
    assert!(is_mimo_model("mimo-v2-flash"));
    assert!(is_mimo_model("Xiaomi-MiMo/MiMo-V2-Pro"));
}

#[test]
fn does_not_flag_other_models() {
    assert!(!is_mimo_model("gpt-4"));
    assert!(!is_mimo_model("claude-opus-4-8"));
    assert!(!is_mimo_model("Qwen3.7-Plus"));
    assert!(!is_mimo_model("glm-4.6"));
}

#[test]
fn hint_targets_the_actual_failure_modes() {
    // Names the exact text-emission shapes we've seen mimo leak.
    assert!(MIMO_TOOL_CALL_HINT.contains("tool_call_list"));
    assert!(MIMO_TOOL_CALL_HINT.contains("structured tool call"));
}
