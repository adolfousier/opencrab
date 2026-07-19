//! Tests for the RSI before/after regression guard (#625).

use crate::eval::before_after::BeforeAfter;
use crate::eval::scorer::{BinaryQuestion, BinaryVerdict, Scorecard};

fn card(yes: usize, total: usize) -> Scorecard {
    let results = (0..total)
        .map(|i| {
            (
                BinaryQuestion::new("d", format!("q{i}")),
                BinaryVerdict {
                    yes: i < yes,
                    explanation: None,
                },
            )
        })
        .collect();
    Scorecard::from_verdicts(results)
}

#[test]
fn genuine_improvement_with_control_held_is_accepted() {
    let ba = BeforeAfter {
        targeted_before: 0.4,
        targeted_after: 0.9,
        control_before: 0.95,
        control_after: 0.95,
    };
    let v = ba.evaluate(0.1, 0.05);
    assert!(v.improved);
    assert!(v.control_held);
    assert!(v.accepted);
    assert!((v.targeted_delta - 0.5).abs() < 1e-9);
}

#[test]
fn improvement_that_regresses_control_is_rejected() {
    // Targeted jumps, but the unrelated control drops 0.20 — the capability-vs-
    // guidance failure the guard exists to catch.
    let ba = BeforeAfter {
        targeted_before: 0.4,
        targeted_after: 0.9,
        control_before: 0.90,
        control_after: 0.70,
    };
    let v = ba.evaluate(0.1, 0.05);
    assert!(v.improved);
    assert!(!v.control_held);
    assert!(!v.accepted);
}

#[test]
fn no_op_change_is_rejected() {
    let ba = BeforeAfter {
        targeted_before: 0.8,
        targeted_after: 0.8,
        control_before: 0.9,
        control_after: 0.9,
    };
    let v = ba.evaluate(0.1, 0.05);
    assert!(!v.improved);
    assert!(!v.accepted);
}

#[test]
fn small_control_dip_within_tolerance_still_accepts() {
    let ba = BeforeAfter {
        targeted_before: 0.5,
        targeted_after: 0.8,
        control_before: 0.90,
        control_after: 0.88, // 0.02 dip, within 0.05 tolerance
    };
    let v = ba.evaluate(0.1, 0.05);
    assert!(v.accepted);
}

#[test]
fn from_scorecards_reads_overall() {
    // targeted: 1/4 -> 4/4 ; control: 4/4 -> 4/4.
    let ba = BeforeAfter::from_scorecards(&card(1, 4), &card(4, 4), &card(4, 4), &card(4, 4));
    assert_eq!(ba.targeted_before, 0.25);
    assert_eq!(ba.targeted_after, 1.0);
    let v = ba.evaluate(0.1, 0.05);
    assert!(v.accepted);
}
