//! Plan tool.
//!
//! Moved out of `src/brain/tools/plan_tool.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::brain::tools::epistemic::Confidence;
use crate::brain::tools::plan_tool::*;

#[test]
fn outcome_confidence_mapping() {
    assert_eq!(task_outcome_confidence("completed"), Confidence::Verified);
    assert_eq!(task_outcome_confidence("failed"), Confidence::Contradicted);
    assert_eq!(task_outcome_confidence("skipped"), Confidence::Uncertain);
}

#[test]
fn outcome_key_stable_across_retries() {
    let k1 = task_outcome_key(3, "Fix the parser");
    let k2 = task_outcome_key(3, "Fix the parser");
    assert_eq!(k1, k2);
}

#[test]
fn outcome_key_differs_by_task() {
    assert_ne!(
        task_outcome_key(3, "Fix the parser"),
        task_outcome_key(4, "Fix the parser")
    );
    assert_ne!(
        task_outcome_key(3, "Fix the parser"),
        task_outcome_key(3, "Write the docs")
    );
}
