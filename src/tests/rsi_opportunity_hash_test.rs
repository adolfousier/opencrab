//! The RSI dedup gate must recognise an unchanged finding (#804).
//!
//! The hash covered the full opportunity description, including the
//! `- session=…, time=…` lines that illustrate it. Those carry a fresh
//! session id and timestamp from whatever the latest events happened to be,
//! so the hash differed on essentially every cycle even when the finding was
//! word-for-word identical.
//!
//! The gate could therefore never suppress a repeat. The agent was spawned
//! hourly to look at the same data and say so: "Same data. Stopping." appeared
//! 46 times in nine days, each a full paid turn.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::rsi::hash_opportunities;

/// One finding, rendered with the per-event examples the builder appends.
fn opportunity(example_session: &str, example_time: &str) -> String {
    format!(
        "tool `hashline_edit` failing 34% (12 of 35 calls)\n  \
         Recent failures:\n  \
         - session={example_session}, time={example_time}, meta=stale hash"
    )
}

#[test]
fn the_same_finding_hashes_the_same_despite_new_examples() {
    // The bug: only the illustration moved, and the gate treated it as new.
    let cycle_one = vec![opportunity("a1b2c3d4", "2026-07-26 21:00")];
    let cycle_two = vec![opportunity("99887766", "2026-07-26 22:00")];

    assert_eq!(
        hash_opportunities(&cycle_one),
        hash_opportunities(&cycle_two),
        "a finding whose only change is which events illustrate it is not new"
    );
}

#[test]
fn a_changed_count_is_still_new() {
    // The gate must stay sensitive to the finding itself, or it suppresses
    // real movement.
    let before = vec!["tool `hashline_edit` failing 34% (12 of 35 calls)".to_string()];
    let after = vec!["tool `hashline_edit` failing 51% (18 of 35 calls)".to_string()];
    assert_ne!(hash_opportunities(&before), hash_opportunities(&after));
}

#[test]
fn a_new_entry_is_still_new() {
    let one = vec![opportunity("a1b2c3d4", "2026-07-26 21:00")];
    let two = vec![
        opportunity("a1b2c3d4", "2026-07-26 21:00"),
        "3 provider errors recorded.".to_string(),
    ];
    assert_ne!(hash_opportunities(&one), hash_opportunities(&two));
}

#[test]
fn reordering_the_findings_is_still_new() {
    // Top-N order reflects severity ranking, so a reshuffle is a real change.
    let a = vec!["first".to_string(), "second".to_string()];
    let b = vec!["second".to_string(), "first".to_string()];
    assert_ne!(hash_opportunities(&a), hash_opportunities(&b));
}

#[test]
fn adjacent_findings_cannot_collapse_into_one() {
    // The join sentinel exists so two descriptions cannot hash the same as
    // one merged description.
    let split = vec!["alpha".to_string(), "beta".to_string()];
    let merged = vec!["alpha\nbeta".to_string()];
    assert_ne!(hash_opportunities(&split), hash_opportunities(&merged));
}

#[test]
fn an_empty_set_is_stable() {
    // Empty and unchanged is the quiet baseline; it must not churn.
    assert_eq!(hash_opportunities(&[]), hash_opportunities(&[]));
}

#[test]
fn only_the_event_lines_are_ignored() {
    // Stripping too much would blind the gate. The finding text and its
    // headers must still count.
    let with_header = vec!["finding\n  Recent failures:".to_string()];
    let without_header = vec!["finding".to_string()];
    assert_ne!(
        hash_opportunities(&with_header),
        hash_opportunities(&without_header)
    );
}
