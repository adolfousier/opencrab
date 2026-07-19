//! Tests for the live runner: variance + baseline drift (live-L3, #632).

use crate::eval::baseline::Baseline;
use crate::eval::runner::{LiveEvalOutcome, VarianceReport, repeat_k};
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
fn variance_of_constant_scores_is_zero() {
    let r = VarianceReport::from_scores(&[0.8, 0.8, 0.8]);
    assert_eq!(r.k, 3);
    assert!((r.mean - 0.8).abs() < 1e-9);
    assert!(r.stddev < 1e-9);
    assert_eq!((r.min, r.max), (0.8, 0.8));
}

#[test]
fn variance_reports_spread() {
    // scores 0.0, 1.0 -> mean 0.5, population sd 0.5.
    let r = VarianceReport::from_scores(&[0.0, 1.0]);
    assert_eq!(r.mean, 0.5);
    assert_eq!(r.min, 0.0);
    assert_eq!(r.max, 1.0);
    assert!((r.stddev - 0.5).abs() < 1e-9);
    assert!(r.render().contains("2 runs"));
}

#[test]
fn empty_and_single_scores() {
    let empty = VarianceReport::from_scores(&[]);
    assert_eq!(empty.k, 0);
    assert_eq!(empty.mean, 0.0);
    let single = VarianceReport::from_scores(&[0.42]);
    assert_eq!(single.mean, 0.42);
    assert_eq!(single.stddev, 0.0);
}

#[tokio::test]
async fn repeat_k_runs_the_op_k_times() {
    // Op returns a card scoring i/4 on run i.
    let (cards, report) = repeat_k(3, |i| async move { card(i, 4) }).await;
    assert_eq!(cards.len(), 3);
    // overalls: 0/4, 1/4, 2/4 -> mean 0.25.
    assert!((report.mean - 0.25).abs() < 1e-9);
    assert_eq!(report.k, 3);
}

#[test]
fn outcome_flags_drift_against_a_prior_baseline() {
    // Prior baseline overall 0.9; current runs average ~0.5 -> regression.
    let prior = Baseline {
        label: "prior".to_string(),
        overall: 0.9,
        dimensions: std::iter::once(("d".to_string(), 0.9)).collect(),
    };
    let cards = vec![card(2, 4), card(2, 4)]; // overall 0.5 each
    let outcome = LiveEvalOutcome::new("run", &cards, Some(&prior), 0.05);
    assert!(!outcome.holds());
    assert_eq!(outcome.current.overall, 0.5);
    assert!((outcome.variance.mean - 0.5).abs() < 1e-9);
}

#[test]
fn outcome_with_no_prior_baseline_holds() {
    let cards = vec![card(4, 4)];
    let outcome = LiveEvalOutcome::new("run", &cards, None, 0.05);
    assert!(outcome.holds());
    assert_eq!(outcome.current.overall, 1.0);
}
