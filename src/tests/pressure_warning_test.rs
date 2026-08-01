//! Tests for the pre-compaction context-pressure warning (#909).
//!
//! Covers the pure decision logic in `nudge.rs` (band boundaries, once-per-entry
//! throttle, re-arm below floor) and the end-to-end wiring on `AgentContext`
//! (nudge appends to system_brain, token count rises).

use crate::brain::agent::context::AgentContext;
use crate::brain::agent::service::nudge::{
    PRESSURE_WARN_CEILING, PRESSURE_WARN_FLOOR, context_pressure_warning, in_pressure_warning_band,
    should_emit_pressure_warning,
};
use uuid::Uuid;

// ── Band boundary tests ──

#[test]
fn just_below_floor_is_not_in_band() {
    assert!(!in_pressure_warning_band(54.9));
    assert!(!in_pressure_warning_band(54.0));
    assert!(!in_pressure_warning_band(0.0));
}

#[test]
fn at_floor_is_in_band() {
    // 55.0 is the inclusive lower edge.
    assert!(in_pressure_warning_band(55.0));
}

#[test]
fn mid_band_is_in_band() {
    assert!(in_pressure_warning_band(60.0));
    assert!(in_pressure_warning_band(63.9));
}

#[test]
fn just_below_ceiling_is_in_band() {
    // 64.9 is still inside; 65.0 is where compaction fires (excluded).
    assert!(in_pressure_warning_band(64.9));
    assert!(!in_pressure_warning_band(65.0));
    assert!(!in_pressure_warning_band(90.0));
    assert!(!in_pressure_warning_band(100.0));
}

#[test]
fn band_edges_match_constants() {
    assert_eq!(PRESSURE_WARN_FLOOR, 55.0);
    assert_eq!(PRESSURE_WARN_CEILING, 65.0);
}

// ── should_emit_pressure_warning: emission + throttle ──

#[test]
fn emits_on_first_entry_into_band() {
    assert_eq!(
        should_emit_pressure_warning(60.0, false),
        Some(context_pressure_warning())
    );
}

#[test]
fn does_not_emit_when_already_emitted() {
    // Once-per-entry: second turn in the band is suppressed.
    assert_eq!(should_emit_pressure_warning(60.0, true), None);
    assert_eq!(should_emit_pressure_warning(64.0, true), None);
}

#[test]
fn does_not_emit_outside_band() {
    assert_eq!(should_emit_pressure_warning(40.0, false), None);
    assert_eq!(should_emit_pressure_warning(54.9, false), None);
    assert_eq!(should_emit_pressure_warning(65.0, false), None);
    assert_eq!(should_emit_pressure_warning(80.0, false), None);
}

#[test]
fn re_arms_when_usage_drops_below_floor() {
    // Simulate the once-per-entry lifecycle across turns.
    // Turn 1: enter band at 58%, not yet emitted -> emits.
    assert!(should_emit_pressure_warning(58.0, false).is_some());

    // Turn 2: still in band, already emitted -> suppressed.
    assert!(should_emit_pressure_warning(58.0, true).is_none());

    // Usage drops to 40% (below floor). Caller clears the flag.
    // Turn 3: re-enters band at 61%, flag cleared -> emits again.
    assert!(should_emit_pressure_warning(61.0, false).is_some());
}

// ── Warning text content ──

#[test]
fn warning_text_mentions_persist_and_compaction() {
    let w = context_pressure_warning();
    assert!(w.contains("compaction"));
    assert!(w.contains("persist"));
    assert!(w.contains("disk"));
    // Must not contain a numeric percentage (nudge-not-number design, #896 guard).
    assert!(!w.contains('%'));
}

// ── AgentContext wiring (nudge appends to system_brain + counts tokens) ──

#[test]
fn appending_warning_to_brain_raises_token_count() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 100_000);
    let brain = "You are a helpful agent.".to_string();
    context.system_brain = Some(brain.clone());
    context.token_count = AgentContext::estimate_tokens(&brain);

    let tokens_before = context.token_count;
    let warning = context_pressure_warning();
    context.system_brain.as_mut().unwrap().push_str(warning);
    context.token_count += AgentContext::estimate_tokens(warning);

    assert!(context.token_count > tokens_before);
    assert!(
        context
            .system_brain
            .as_ref()
            .unwrap()
            .contains("SYSTEM WARNING")
    );
}
