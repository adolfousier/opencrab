//! Tests for the compaction-fidelity harness + dataset format (#621).

use crate::eval::compaction::CompactionDataset;
use crate::eval::replay::ReplayProvider;
use crate::eval::scorer::ProviderJudge;

#[test]
fn seed_dataset_loads_with_probes_and_messages() {
    let ds = CompactionDataset::seed();
    assert_eq!(ds.name, "seed-coding-session");
    assert_eq!(ds.probes.len(), 4);
    assert_eq!(ds.messages().len(), 5);
    // Every probe carries offline keywords.
    assert!(ds.probes.iter().all(|p| !p.expect_keywords.is_empty()));
}

#[test]
fn faithful_summary_scores_full_marks() {
    let ds = CompactionDataset::seed();
    // A summary that mentions every required fact keyword.
    let good = "Preferences: tabs indentation. Edited net/client.rs (retry refactor). \
                Hit error E0433 on the import. Pending: update the CHANGELOG.";
    let card = ds.keyword_scorecard(good);
    assert_eq!(card.overall(), 1.0);
    assert!(card.passes(1.0));
    assert_eq!(card.per_dimension.len(), 4);
}

#[test]
fn lossy_summary_is_penalized_per_dimension() {
    let ds = CompactionDataset::seed();
    // Drops the pending CHANGELOG task and the E0433 error.
    let lossy = "Preferences: tabs. Edited net/client.rs.";
    let card = ds.keyword_scorecard(lossy);
    assert_eq!(card.passed, 2);
    assert_eq!(card.total, 4);
    assert_eq!(card.per_dimension["pending_tasks"].passed, 0);
    assert_eq!(card.per_dimension["errors"].passed, 0);
    assert_eq!(card.per_dimension["user_prefs"].passed, 1);
    // The failing probe explains what was missing.
    let changelog = card
        .results
        .iter()
        .find(|(q, _)| q.dimension == "pending_tasks")
        .unwrap();
    assert!(
        changelog
            .1
            .explanation
            .as_deref()
            .unwrap()
            .contains("CHANGELOG")
    );
}

#[test]
fn empty_summary_survives_nothing() {
    let ds = CompactionDataset::seed();
    let card = ds.keyword_scorecard("");
    assert_eq!(card.overall(), 0.0);
}

#[tokio::test]
async fn judge_path_grades_summary_offline_via_replay() {
    let ds = CompactionDataset::seed();
    // Scripted judge verdicts (one per probe): YES, NO, YES, YES.
    let fixture = r#"{
        "model": "judge",
        "turns": [
            { "text": "YES" },
            { "text": "NO" },
            { "text": "YES" },
            { "text": "YES" }
        ]
    }"#;
    let provider = ReplayProvider::from_json(fixture).unwrap();
    let judge = ProviderJudge::new(&provider, "judge");
    let card = ds.judge_scorecard(&judge, "some produced summary").await;
    assert_eq!((card.passed, card.total), (3, 4));
    assert_eq!(provider.turns_consumed(), 4);
}
