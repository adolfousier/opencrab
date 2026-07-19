//! On-demand live runner + baseline drift + repeat-K variance (live-L3, #632).
//!
//! Live evals are non-deterministic, so a single score is noise. [`repeat_k`]
//! runs a scorecard-producing op K times and [`VarianceReport`] quantifies the
//! spread; [`LiveEvalOutcome`] pairs that with a baseline-drift check (Phase 3).
//!
//! Only the aggregation is here — deterministic and offline-tested. The network
//! work (resolving providers, producing artifacts, judging) is supplied by the
//! caller's op, and a live run only happens on demand when `eval_providers` is
//! set. This is never wired into the CI gate.

use std::future::Future;

use super::baseline::{Baseline, Regression};
use super::scorer::Scorecard;

/// Spread of overall scores across K repeated runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarianceReport {
    pub k: usize,
    pub mean: f64,
    /// Median — robust to a single catastrophic outlier, so it reflects the
    /// TYPICAL run where `mean` is skewed by a rare crater.
    pub median: f64,
    pub min: f64,
    pub max: f64,
    /// Population standard deviation.
    pub stddev: f64,
}

impl VarianceReport {
    /// Compute the spread of a set of overall scores. Empty input yields zeros.
    pub fn from_scores(scores: &[f64]) -> Self {
        if scores.is_empty() {
            return Self {
                k: 0,
                mean: 0.0,
                median: 0.0,
                min: 0.0,
                max: 0.0,
                stddev: 0.0,
            };
        }
        let k = scores.len();
        let mean = scores.iter().sum::<f64>() / k as f64;
        let min = scores.iter().copied().fold(f64::INFINITY, f64::min);
        let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / k as f64;
        let mut sorted = scores.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = if k % 2 == 1 {
            sorted[k / 2]
        } else {
            (sorted[k / 2 - 1] + sorted[k / 2]) / 2.0
        };
        Self {
            k,
            mean,
            median,
            min,
            max,
            stddev: variance.sqrt(),
        }
    }

    /// Fraction of runs scoring at or below `threshold` — the catastrophic
    /// failure rate, distinct from the average quality.
    pub fn failure_rate(scores: &[f64], threshold: f64) -> f64 {
        if scores.is_empty() {
            return 0.0;
        }
        let bad = scores.iter().filter(|&&s| s <= threshold).count();
        bad as f64 / scores.len() as f64
    }

    pub fn render(&self) -> String {
        format!(
            "{} runs: mean={:.3} median={:.3} min={:.3} max={:.3} sd={:.3}",
            self.k, self.mean, self.median, self.min, self.max, self.stddev
        )
    }
}

/// Run a scorecard-producing op `k` times, returning every scorecard plus the
/// variance of their overall scores.
pub async fn repeat_k<F, Fut>(k: usize, mut op: F) -> (Vec<Scorecard>, VarianceReport)
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Scorecard>,
{
    let mut cards = Vec::with_capacity(k);
    for i in 0..k {
        cards.push(op(i).await);
    }
    let scores: Vec<f64> = cards.iter().map(|c| c.overall()).collect();
    let report = VarianceReport::from_scores(&scores);
    (cards, report)
}

/// The result of a repeated live eval: its variance, the mean-score baseline it
/// produced, and any regression vs a prior baseline.
#[derive(Debug, Clone)]
pub struct LiveEvalOutcome {
    pub variance: VarianceReport,
    pub current: Baseline,
    pub regressions: Vec<Regression>,
}

impl LiveEvalOutcome {
    /// Build an outcome from K scorecards. When a `prior` baseline is given, the
    /// mean-score baseline is compared against it and any drop beyond
    /// `tolerance` is reported.
    pub fn new(
        label: impl Into<String>,
        cards: &[Scorecard],
        prior: Option<&Baseline>,
        tolerance: f64,
    ) -> Self {
        let scores: Vec<f64> = cards.iter().map(|c| c.overall()).collect();
        let variance = VarianceReport::from_scores(&scores);
        let current = Baseline::from_scorecards_mean(label, cards);
        let regressions = prior
            .map(|p| p.regressions(&current, tolerance))
            .unwrap_or_default();
        Self {
            variance,
            current,
            regressions,
        }
    }

    /// True when no regression was found (or no prior baseline to compare).
    pub fn holds(&self) -> bool {
        self.regressions.is_empty()
    }
}
