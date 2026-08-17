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

/// #1083 regression: a failure recorded for plan A's task #1 must NOT surface
/// when plan B's task #1 starts. Before the fix both wrote `plan:task:1:<hash>`
/// and the Orient gate filtered on that bare prefix, so every historical plan's
/// task #1 leaked into every new plan's brief.
#[test]
fn foreign_plan_failures_do_not_match_this_plans_prefix() {
    use crate::brain::tools::plan_tool::{plan_belief_prefix, task_outcome_key};
    let (plan_a, plan_b) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());

    let mut store = EpistemicStore::new();
    store.add_belief(
        &task_outcome_key(plan_a, 1, "Run tests"),
        "failed clippy",
        Confidence::Contradicted,
        "test",
    );
    store.add_belief(
        &task_outcome_key(plan_b, 1, "Run tests"),
        "skipped",
        Confidence::Uncertain,
        "test",
    );
    // A pre-#1083 key left in a long-lived store must not match either plan.
    store.add_belief(
        "plan:task:1:deadbeefdeadbeef",
        "failed long ago",
        Confidence::Contradicted,
        "test",
    );

    let for_b = store.list_by_key_prefix(&plan_belief_prefix(plan_b));
    assert_eq!(for_b.len(), 1, "only plan B's own belief: {for_b:?}");
    assert_eq!(for_b[0].value, "skipped");

    let for_a = store.list_by_key_prefix(&plan_belief_prefix(plan_a));
    assert_eq!(for_a.len(), 1, "only plan A's own belief: {for_a:?}");
    assert_eq!(for_a[0].value, "failed clippy");
}

/// #1083 landmine: the superseded copy kept by contradiction detection used to
/// be stored under `{key}:contradicted:{ts}`, which still starts with the
/// original key, so every archived copy re-matched prefix queries forever.
#[test]
fn superseded_copies_leave_the_original_prefix() {
    use crate::brain::tools::plan_tool::{plan_belief_prefix, task_outcome_key};
    let plan = uuid::Uuid::new_v4();
    let key = task_outcome_key(plan, 1, "Run tests");

    // A skipped task (Uncertain) that later completes (Verified) is the
    // transition that archives. A task recorded as `failed` is already
    // Contradicted, and `add_belief` deliberately overwrites those without
    // archiving a second copy, so it never reaches this path.
    let mut store = EpistemicStore::new();
    store.add_belief(&key, "skipped", Confidence::Uncertain, "test");
    store.add_belief(&key, "completed", Confidence::Verified, "test");

    let live = store.list_by_key_prefix(&plan_belief_prefix(plan));
    assert_eq!(live.len(), 1, "the archived copy is out of scope: {live:?}");
    assert_eq!(live[0].value, "completed", "the retry supersedes (#862)");

    // The copy is still retrievable under its own namespace, and its `key`
    // field agrees with the map key it is stored under.
    let archived = store.list_by_key_prefix("contradicted:");
    assert_eq!(archived.len(), 1, "history is kept, not dropped");
    assert!(archived[0].key.starts_with("contradicted:"));
    assert_eq!(archived[0].value, "skipped");
}
