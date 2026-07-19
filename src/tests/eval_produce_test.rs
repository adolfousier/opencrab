//! Tests for live artifact producers (live-L2, #631).

use crate::eval::compaction::CompactionDataset;
use crate::eval::produce::{produce_compaction_summary, run_compaction_eval};
use crate::eval::replay::ReplayProvider;
use crate::eval::scorer::ProviderJudge;

// A summary that preserves every probed fact from the seed dataset.
const FAITHFUL_SUMMARY: &str = "Preferences: tabs indentation. Edited net/client.rs (retry refactor). \
     Hit error E0433 on the import. Pending: update the CHANGELOG.";

fn summary_provider(text: &str) -> ReplayProvider {
    let fixture = format!(r#"{{"model":"producer","turns":[{{"text":{text:?}}}]}}"#);
    ReplayProvider::from_json(&fixture).unwrap()
}

#[tokio::test]
async fn producer_returns_the_models_summary() {
    let ds = CompactionDataset::seed();
    let provider = summary_provider(FAITHFUL_SUMMARY);
    let summary = produce_compaction_summary(&provider, "producer", &ds.messages(), None).await;
    assert_eq!(summary, FAITHFUL_SUMMARY);
    // One producer turn consumed.
    assert_eq!(provider.turns_consumed(), 1);
}

#[tokio::test]
async fn faithful_summary_grades_full_via_keywords() {
    let ds = CompactionDataset::seed();
    let provider = summary_provider(FAITHFUL_SUMMARY);
    let summary = produce_compaction_summary(&provider, "producer", &ds.messages(), None).await;
    assert_eq!(ds.keyword_scorecard(&summary).overall(), 1.0);
}

#[tokio::test]
async fn lossy_summary_loses_dimensions() {
    let ds = CompactionDataset::seed();
    // Drops CHANGELOG and E0433.
    let provider = summary_provider("tabs; edited net/client.rs");
    let summary = produce_compaction_summary(&provider, "producer", &ds.messages(), None).await;
    let card = ds.keyword_scorecard(&summary);
    assert_eq!(card.passed, 2);
    assert_eq!(card.total, 4);
}

#[tokio::test]
async fn end_to_end_produce_then_judge_offline() {
    let ds = CompactionDataset::seed();
    let producer = summary_provider(FAITHFUL_SUMMARY);
    // Judge replays YES for each of the 4 probes.
    let judge_provider =
        ReplayProvider::from_json(r#"{"model":"judge","turns":[{"text":"YES"},{"text":"YES"},{"text":"YES"},{"text":"YES"}]}"#)
            .unwrap();
    let judge = ProviderJudge::new(&judge_provider, "judge");

    let card = run_compaction_eval(&producer, "producer", &judge, &ds, None).await;
    assert_eq!((card.passed, card.total), (4, 4));
    assert_eq!(producer.turns_consumed(), 1);
    assert_eq!(judge_provider.turns_consumed(), 4);
}
