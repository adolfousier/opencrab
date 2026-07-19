//! Tests for the memory recall metrics (#623).

use std::collections::HashSet;

use crate::eval::recall::{
    QueryCase, QueryMetrics, RecallReport, mrr, ndcg_at_k, precision_at_k, recall_at_k,
};

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn relset(list: &[&str]) -> HashSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn perfect_ranking_scores_one() {
    let ranked = ids(&["a", "b", "c", "d"]);
    let relevant = relset(&["a", "b"]);
    assert_eq!(precision_at_k(&ranked, &relevant, 2), 1.0);
    assert_eq!(recall_at_k(&ranked, &relevant, 2), 1.0);
    assert_eq!(mrr(&ranked, &relevant), 1.0);
    assert_eq!(ndcg_at_k(&ranked, &relevant, 2), 1.0);
}

#[test]
fn precision_and_recall_at_k_partial() {
    // Relevant: a, c. Ranked top-3: a (hit), b (miss), c (hit).
    let ranked = ids(&["a", "b", "c", "d"]);
    let relevant = relset(&["a", "c"]);
    assert!((precision_at_k(&ranked, &relevant, 3) - 2.0 / 3.0).abs() < 1e-9);
    assert_eq!(recall_at_k(&ranked, &relevant, 3), 1.0);
    // At k=1 only "a" is seen: recall 0.5.
    assert_eq!(recall_at_k(&ranked, &relevant, 1), 0.5);
}

#[test]
fn mrr_uses_first_relevant_rank() {
    let ranked = ids(&["x", "y", "a"]);
    let relevant = relset(&["a"]);
    // First relevant at position 3 -> 1/3.
    assert!((mrr(&ranked, &relevant) - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn ndcg_rewards_higher_placement() {
    let relevant = relset(&["a"]);
    let top = ids(&["a", "x", "y"]);
    let bottom = ids(&["x", "y", "a"]);
    let top_score = ndcg_at_k(&top, &relevant, 3);
    let bottom_score = ndcg_at_k(&bottom, &relevant, 3);
    assert_eq!(top_score, 1.0);
    assert!(bottom_score < top_score);
    assert!(bottom_score > 0.0);
}

#[test]
fn empty_and_no_relevant_are_zero() {
    let relevant = relset(&["a"]);
    assert_eq!(precision_at_k(&[], &relevant, 5), 0.0);
    assert_eq!(recall_at_k(&[], &relevant, 5), 0.0);
    assert_eq!(mrr(&[], &relevant), 0.0);
    // No relevant docs labeled -> recall undefined, reported 0.
    let ranked = ids(&["a", "b"]);
    assert_eq!(recall_at_k(&ranked, &relset(&[]), 2), 0.0);
}

#[test]
fn report_macro_averages_across_queries() {
    let cases = vec![
        QueryCase {
            query: "q1".to_string(),
            relevant: ids(&["a"]),
        },
        QueryCase {
            query: "q2".to_string(),
            relevant: ids(&["b"]),
        },
    ];
    // q1 perfect (a first), q2 misses entirely.
    let results = vec![ids(&["a", "z"]), ids(&["y", "z"])];
    let report = RecallReport::compute(&cases, &results, 2);
    assert_eq!(report.per_query.len(), 2);
    // precision: q1=0.5 (a,z), q2=0.0 -> avg 0.25.
    assert!((report.aggregate.precision - 0.25).abs() < 1e-9);
    // recall: q1=1.0, q2=0.0 -> avg 0.5.
    assert!((report.aggregate.recall - 0.5).abs() < 1e-9);
    assert!(report.render().contains("recall@2"));
}

#[test]
fn query_metrics_compute_bundles_all_four() {
    let m = QueryMetrics::compute(&ids(&["a", "b"]), &relset(&["a"]), 2);
    assert_eq!(m.recall, 1.0);
    assert_eq!(m.mrr, 1.0);
}
