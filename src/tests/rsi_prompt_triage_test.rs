//! Pins the BinEval-informed tool-failure triage guidance in the RSI prompt
//! (#236). These guard against the prompt silently losing the rules that stop
//! the RSI from banning working tools or bloating the brain.

use crate::brain::rsi::RSI_AGENT_PROMPT;

#[test]
fn prompt_has_failure_triage_section() {
    assert!(RSI_AGENT_PROMPT.contains("Tool-Failure Triage"));
    // Minimum-sample guard against acting on n=1 noise.
    assert!(RSI_AGENT_PROMPT.contains("fewer than 5"));
    // Recoverable-vs-defect distinction.
    assert!(
        RSI_AGENT_PROMPT.contains("recoverable")
            && RSI_AGENT_PROMPT.to_lowercase().contains("environmental")
    );
    // Capability-vs-guidance (the paper's central iterative-update finding).
    assert!(RSI_AGENT_PROMPT.contains("Capability, or guidance"));
}

#[test]
fn prompt_forbids_banning_builtin_tools() {
    assert!(RSI_AGENT_PROMPT.contains("BUILT-IN tool"));
    assert!(RSI_AGENT_PROMPT.contains("routing guidance"));
    // Disabling is only allowed for user-defined tools.
    assert!(RSI_AGENT_PROMPT.contains("tools.toml"));
}

#[test]
fn prompt_warns_against_brain_bloat() {
    assert!(RSI_AGENT_PROMPT.contains("Avoid prompt bloat"));
    assert!(RSI_AGENT_PROMPT.contains("violation counters"));
    assert!(RSI_AGENT_PROMPT.contains("competing instructions"));
}
