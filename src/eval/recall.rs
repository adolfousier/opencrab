//! Memory recall metrics + labeled query dataset (#623).
//!
//! The retrieval metrics are pure functions over a ranked list of document ids
//! plus a relevant set, so they run deterministically offline. A live run feeds
//! real `memory_search` output through the same functions; tests feed synthetic
//! ranked lists. Relevance is binary.

use std::collections::HashSet;

use serde::Deserialize;

/// Fraction of the top-`k` results that are relevant.
pub fn precision_at_k(ranked: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let top = &ranked[..ranked.len().min(k)];
    if top.is_empty() {
        return 0.0;
    }
    let hits = top.iter().filter(|id| relevant.contains(*id)).count();
    hits as f64 / top.len() as f64
}

/// Fraction of all relevant documents found in the top-`k`.
pub fn recall_at_k(ranked: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let top = &ranked[..ranked.len().min(k)];
    let hits = top.iter().filter(|id| relevant.contains(*id)).count();
    hits as f64 / relevant.len() as f64
}

/// Reciprocal rank of the first relevant result (0.0 if none).
pub fn mrr(ranked: &[String], relevant: &HashSet<String>) -> f64 {
    for (i, id) in ranked.iter().enumerate() {
        if relevant.contains(id) {
            return 1.0 / (i as f64 + 1.0);
        }
    }
    0.0
}

/// Normalized discounted cumulative gain at `k` with binary relevance.
pub fn ndcg_at_k(ranked: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if k == 0 || relevant.is_empty() {
        return 0.0;
    }
    let discount = |i: usize| 1.0 / ((i as f64 + 2.0).log2());
    let dcg: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| relevant.contains(*id))
        .map(|(i, _)| discount(i))
        .sum();
    let ideal_hits = relevant.len().min(k);
    let idcg: f64 = (0..ideal_hits).map(discount).sum();
    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

/// The four metrics for one query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryMetrics {
    pub precision: f64,
    pub recall: f64,
    pub mrr: f64,
    pub ndcg: f64,
}

impl QueryMetrics {
    /// Compute all metrics for a ranked result list against a relevant set.
    pub fn compute(ranked: &[String], relevant: &HashSet<String>, k: usize) -> Self {
        Self {
            precision: precision_at_k(ranked, relevant, k),
            recall: recall_at_k(ranked, relevant, k),
            mrr: mrr(ranked, relevant),
            ndcg: ndcg_at_k(ranked, relevant, k),
        }
    }
}

/// One labeled query: the relevant document ids for it.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryCase {
    pub query: String,
    pub relevant: Vec<String>,
}

/// A labeled recall dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallDataset {
    pub name: String,
    pub cases: Vec<QueryCase>,
}

impl RecallDataset {
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

/// Per-query metrics plus the macro-average across queries.
#[derive(Debug, Clone)]
pub struct RecallReport {
    pub per_query: Vec<(String, QueryMetrics)>,
    pub aggregate: QueryMetrics,
    pub k: usize,
}

impl RecallReport {
    /// Compute a report from each query's relevant set and its ranked results.
    /// `results` is paired positionally with `cases`.
    pub fn compute(cases: &[QueryCase], results: &[Vec<String>], k: usize) -> Self {
        let mut per_query = Vec::with_capacity(cases.len());
        let (mut sp, mut sr, mut sm, mut sn) = (0.0, 0.0, 0.0, 0.0);
        for (case, ranked) in cases.iter().zip(results.iter()) {
            let relevant: HashSet<String> = case.relevant.iter().cloned().collect();
            let m = QueryMetrics::compute(ranked, &relevant, k);
            sp += m.precision;
            sr += m.recall;
            sm += m.mrr;
            sn += m.ndcg;
            per_query.push((case.query.clone(), m));
        }
        let n = per_query.len().max(1) as f64;
        RecallReport {
            aggregate: QueryMetrics {
                precision: sp / n,
                recall: sr / n,
                mrr: sm / n,
                ndcg: sn / n,
            },
            per_query,
            k,
        }
    }

    /// Stable human-readable summary of the aggregate metrics.
    pub fn render(&self) -> String {
        format!(
            "recall@{k}: P={:.3} R={:.3} MRR={:.3} nDCG={:.3} ({n} queries)\n",
            self.aggregate.precision,
            self.aggregate.recall,
            self.aggregate.mrr,
            self.aggregate.ndcg,
            k = self.k,
            n = self.per_query.len()
        )
    }
}
