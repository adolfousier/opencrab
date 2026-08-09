//! The RSI dedup gate must recognise an unchanged finding (#977, v2 hash).
//!
//! v1 hashed the opportunity-description bodies. #804 stripped the
//! `- session=…, time=…` example lines, but the bodies still carried
//! churning counts ("34% (12 of 35)", "17 successful invocations"),
//! sample invocations and severity-ordered top-N slices, so the hash
//! moved on essentially every busy cycle and the gate never fired: the
//! agent was spawned hourly to look at the same data and say so —
//! "Same data. Stopping." appeared 46 times in nine days, each a full
//! paid turn.
//!
//! v2 hashes one stable identity key per finding (dimension, subsystem,
//! request signature, tool sequence). Counts, samples and ordering are
//! illustration for the agent once it runs; they are not part of
//! whether the finding is new. The builders in `brain::rsi` push the
//! keys alongside the descriptions.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::rsi::hash_opportunities;

/// Realistic key set for a busy cycle: one failing tool, corrections,
/// provider errors and a promoted bash subsystem.
fn key_set() -> Vec<String> {
    vec![
        "tool_failure:hashline_edit".to_string(),
        "user_corrections".to_string(),
        "provider_errors".to_string(),
        "bash_subsystem:gh".to_string(),
    ]
}

#[test]
fn the_same_findings_hash_the_same_whatever_their_numbers() {
    // The bug (#977): descriptions carried counts and samples that moved
    // every busy cycle, so identical findings hashed differently and the
    // gate never suppressed a repeat. Keys carry no numbers — the same
    // findings are the same hash, full stop.
    let cycle_one = key_set();
    let cycle_two = key_set();
    assert_eq!(
        hash_opportunities(&cycle_one),
        hash_opportunities(&cycle_two),
        "a finding set whose only change is counts and samples is not new"
    );
}

#[test]
fn reordering_the_findings_is_not_new() {
    // CONTRACT REVERSED (#977), deliberately. Top-N order reflects this
    // cycle's severity ranking; the SET of findings is what matters.
    // Order churn was one of the three sources (with counts and samples)
    // that kept the v1 hash moving every cycle.
    let a = vec![
        "tool_failure:hashline_edit".to_string(),
        "user_corrections".to_string(),
    ];
    let b = vec![
        "user_corrections".to_string(),
        "tool_failure:hashline_edit".to_string(),
    ];
    assert_eq!(hash_opportunities(&a), hash_opportunities(&b));
}

#[test]
fn a_new_finding_is_still_new() {
    // The gate must stay sensitive to the finding set, or it suppresses
    // real movement.
    let before = key_set();
    let mut after = key_set();
    after.push("command_pattern:/standup".to_string());
    assert_ne!(hash_opportunities(&before), hash_opportunities(&after));
}

#[test]
fn a_disappeared_finding_is_still_new() {
    // A tool that recovered drops out of the set — that is movement too.
    let before = key_set();
    let after: Vec<String> = key_set()
        .into_iter()
        .filter(|k| k != "tool_failure:hashline_edit")
        .collect();
    assert_ne!(hash_opportunities(&before), hash_opportunities(&after));
}

#[test]
fn adjacent_findings_cannot_collapse_into_one() {
    // The join sentinel exists so two keys cannot hash the same as one
    // merged key. Keys are flattened to single lines first, so a key
    // cannot smuggle the sentinel in.
    let split = vec!["alpha".to_string(), "beta".to_string()];
    let merged = vec!["alpha\nbeta".to_string()];
    assert_ne!(hash_opportunities(&split), hash_opportunities(&merged));
}

#[test]
fn a_key_cannot_forge_the_join_sentinel() {
    // Flattening embedded newlines to spaces must keep a single key from
    // impersonating the two-key join.
    let forged = vec!["alpha\n---\nbeta".to_string()];
    let genuine = vec!["alpha".to_string(), "beta".to_string()];
    assert_ne!(hash_opportunities(&forged), hash_opportunities(&genuine));
}

#[test]
fn an_empty_set_is_stable() {
    // Empty and unchanged is the quiet baseline; it must not churn.
    assert_eq!(hash_opportunities(&[]), hash_opportunities(&[]));
}
