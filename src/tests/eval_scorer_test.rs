//! Tests for the BinEval-style scorer + scorecard (#622).

use crate::eval::replay::ReplayProvider;
use crate::eval::scorer::{BinaryQuestion, ProviderJudge, SequenceJudge, parse_verdict, score};

fn questions() -> Vec<BinaryQuestion> {
    vec![
        BinaryQuestion::new("prefs", "Does it preserve the user preference?"),
        BinaryQuestion::new("prefs", "Does it preserve the second preference?"),
        BinaryQuestion::new("tasks", "Does it keep the pending task list?"),
    ]
}

#[test]
fn parse_verdict_reads_leading_word() {
    assert!(parse_verdict("YES, it does."));
    assert!(!parse_verdict("NO — the fact is absent"));
    assert!(parse_verdict("yes\nbecause ..."));
    // Ambiguous defaults to NO (fact unverified).
    assert!(!parse_verdict("hmm, unclear"));
}

#[tokio::test]
async fn aggregates_per_dimension_and_overall() {
    // prefs: YES, NO ; tasks: YES  -> overall 2/3.
    let judge = SequenceJudge::new([true, false, true]);
    let card = score(&judge, questions(), "some summary").await;

    assert_eq!(card.total, 3);
    assert_eq!(card.passed, 2);
    assert!((card.overall() - 2.0 / 3.0).abs() < 1e-9);

    let prefs = &card.per_dimension["prefs"];
    assert_eq!((prefs.passed, prefs.total), (1, 2));
    let tasks = &card.per_dimension["tasks"];
    assert_eq!((tasks.passed, tasks.total), (1, 1));

    assert!(card.passes(0.66));
    assert!(!card.passes(0.9));
    assert!(card.render().contains("prefs"));
}

#[tokio::test]
async fn all_pass_and_all_fail_thresholds() {
    let all_yes = SequenceJudge::new([true, true, true]);
    assert!(score(&all_yes, questions(), "x").await.passes(1.0));

    let all_no = SequenceJudge::new([false, false, false]);
    let card = score(&all_no, questions(), "x").await;
    assert_eq!(card.overall(), 0.0);
    assert!(!card.passes(0.01));
}

#[tokio::test]
async fn provider_judge_parses_replayed_verdicts_offline() {
    // The judge is driven by the offline replay provider: YES, NO, YES.
    let fixture = r#"{
        "model": "judge",
        "turns": [
            { "text": "YES" },
            { "text": "NO, that fact is not present" },
            { "text": "YES it is preserved" }
        ]
    }"#;
    let provider = ReplayProvider::from_json(fixture).unwrap();
    let judge = ProviderJudge::new(&provider, "judge");
    let card = score(&judge, questions(), "the summary under test").await;

    assert_eq!((card.passed, card.total), (2, 3));
    assert_eq!(provider.turns_consumed(), 3);
    // Explanations captured from the judge replies.
    assert!(
        card.results[1]
            .1
            .explanation
            .as_deref()
            .unwrap()
            .contains("NO")
    );
}
