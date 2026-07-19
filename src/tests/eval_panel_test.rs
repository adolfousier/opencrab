//! Tests for the majority-vote judge panel (live-L1, #630).

use crate::eval::panel::PanelJudge;
use crate::eval::replay::ReplayProvider;
use crate::eval::scorer::{Judge, SequenceJudge};
use std::sync::Arc;

/// A judge that always returns the same verdict, for building panels of a known
/// composition.
struct FixedJudge(bool);

#[async_trait::async_trait]
impl Judge for FixedJudge {
    async fn judge(&self, _q: &str, _a: &str) -> crate::eval::scorer::BinaryVerdict {
        crate::eval::scorer::BinaryVerdict {
            yes: self.0,
            explanation: None,
        }
    }
}

fn panel(votes: &[bool]) -> PanelJudge {
    let members: Vec<Box<dyn Judge>> = votes
        .iter()
        .map(|&v| Box::new(FixedJudge(v)) as Box<dyn Judge>)
        .collect();
    PanelJudge::new(members)
}

#[tokio::test]
async fn unanimous_yes_passes_and_unanimous_no_fails() {
    assert!(panel(&[true, true, true]).judge("q", "a").await.yes);
    assert!(!panel(&[false, false, false]).judge("q", "a").await.yes);
}

#[tokio::test]
async fn strict_majority_passes() {
    // 2/3 YES > 0.5 -> pass.
    let v = panel(&[true, true, false]).judge("q", "a").await;
    assert!(v.yes);
    assert!(v.explanation.unwrap().contains("2/3"));
}

#[tokio::test]
async fn tie_fails_by_default() {
    // 2/4 = 0.5 is NOT > 0.5 -> fail (conservative: a fact needs a majority).
    assert!(!panel(&[true, true, false, false]).judge("q", "a").await.yes);
}

#[tokio::test]
async fn threshold_is_configurable() {
    // With a 2/3 threshold, 2/3 YES (0.666 > 0.666 is false) fails...
    let strict = panel(&[true, true, false]).with_threshold(2.0 / 3.0);
    assert!(!strict.judge("q", "a").await.yes);
    // ...but 3/3 passes.
    let strict_all = panel(&[true, true, true]).with_threshold(2.0 / 3.0);
    assert!(strict_all.judge("q", "a").await.yes);
}

#[tokio::test]
async fn single_member_panel_acts_like_that_member() {
    assert!(panel(&[true]).judge("q", "a").await.yes);
    assert!(!panel(&[false]).judge("q", "a").await.yes);
}

#[tokio::test]
async fn empty_panel_confirms_nothing() {
    let empty = PanelJudge::new(Vec::new());
    assert!(empty.is_empty());
    assert!(!empty.judge("q", "a").await.yes);
}

#[tokio::test]
async fn panel_of_provider_judges_votes_offline_via_replay() {
    // Two "providers": one replays YES, the other NO -> 1/2 = tie -> fails.
    let yes = ReplayProvider::from_json(r#"{"model":"a","turns":[{"text":"YES"}]}"#).unwrap();
    let no = ReplayProvider::from_json(r#"{"model":"b","turns":[{"text":"NO"}]}"#).unwrap();
    let members: Vec<Box<dyn Judge>> = vec![
        Box::new(crate::eval::panel::LiveJudge::new(Arc::new(yes), "a")),
        Box::new(crate::eval::panel::LiveJudge::new(Arc::new(no), "b")),
    ];
    let split = PanelJudge::new(members);
    assert!(!split.judge("does it survive?", "summary").await.yes);
}

#[tokio::test]
async fn sequence_judge_members_are_independent_per_call() {
    // Sanity: a panel routes each question to each member once.
    let members: Vec<Box<dyn Judge>> = vec![
        Box::new(SequenceJudge::new([true])),
        Box::new(SequenceJudge::new([true])),
    ];
    let p = PanelJudge::new(members);
    assert!(p.judge("q", "a").await.yes);
}
