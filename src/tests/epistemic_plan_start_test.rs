//! Tests for the epistemic Orient gate on plan `start` (#886).
//!
//! Verifies that `EpistemicStore::list_by_key_prefix` correctly filters
//! beliefs by key prefix, and that the confidence filter used by the
//! plan_tool Start handler (Contradicted + Uncertain only) selects the
//! right subset.

use crate::brain::tools::epistemic::{Confidence, EpistemicStore};

#[test]
fn prefix_query_returns_only_matching_keys() {
    let mut store = EpistemicStore::new();
    store.add_belief(
        "plan:task:1:aaaa",
        "failed clippy",
        Confidence::Contradicted,
        "test",
    );
    store.add_belief("plan:task:2:bbbb", "skipped", Confidence::Uncertain, "test");
    store.add_belief("plan:task:3:cccc", "done", Confidence::Verified, "test");
    store.add_belief("memory:server:ip", "10.0.0.1", Confidence::Inferred, "test");

    let plan_beliefs = store.list_by_key_prefix("plan:task:");
    assert_eq!(plan_beliefs.len(), 3, "should match all plan:task: keys");

    let memory_beliefs = store.list_by_key_prefix("memory:");
    assert_eq!(memory_beliefs.len(), 1);
}

#[test]
fn confidence_filter_selects_contradicted_and_uncertain() {
    let mut store = EpistemicStore::new();
    store.add_belief(
        "plan:task:1:aaaa",
        "failed",
        Confidence::Contradicted,
        "test",
    );
    store.add_belief("plan:task:2:bbbb", "skipped", Confidence::Uncertain, "test");
    store.add_belief("plan:task:3:cccc", "done", Confidence::Verified, "test");
    store.add_belief(
        "plan:task:4:dddd",
        "inferred ok",
        Confidence::Inferred,
        "test",
    );

    // Mirror the filter the plan_tool Start handler applies.
    let actionable: Vec<_> = store
        .list_by_key_prefix("plan:task:")
        .into_iter()
        .filter(|b| {
            matches!(
                b.confidence,
                Confidence::Contradicted | Confidence::Uncertain
            )
        })
        .collect();

    assert_eq!(actionable.len(), 2, "only Contradicted + Uncertain");
    let keys: Vec<&str> = actionable.iter().map(|b| b.key.as_str()).collect();
    assert!(keys.contains(&"plan:task:1:aaaa"));
    assert!(keys.contains(&"plan:task:2:bbbb"));
}

#[test]
fn empty_store_returns_no_flags() {
    let store = EpistemicStore::new();
    let result = store.list_by_key_prefix("plan:task:");
    assert!(result.is_empty(), "empty store should yield no beliefs");
}

#[test]
fn verified_and_inferred_beliefs_are_not_flagged() {
    let mut store = EpistemicStore::new();
    store.add_belief("plan:task:1:aaaa", "done", Confidence::Verified, "test");
    store.add_belief(
        "plan:task:2:bbbb",
        "likely ok",
        Confidence::Inferred,
        "test",
    );

    let actionable: Vec<_> = store
        .list_by_key_prefix("plan:task:")
        .into_iter()
        .filter(|b| {
            matches!(
                b.confidence,
                Confidence::Contradicted | Confidence::Uncertain
            )
        })
        .collect();

    assert!(
        actionable.is_empty(),
        "Verified and Inferred beliefs should not be flagged"
    );
}
