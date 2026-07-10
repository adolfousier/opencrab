//! Tests for the repeated-user-request -> slash-command detector (#504):
//! groups requests by normalized signature, surfaces ones above threshold,
//! and rejects trivial or prose messages so one-off chatter never proposes.

use crate::brain::rsi_command_patterns::{COMMAND_PATTERN_THRESHOLD, command_candidates};

#[test]
fn recurring_ask_becomes_a_candidate() {
    // Case/punctuation/whitespace/@mention variants of the SAME word
    // sequence normalize together; a different sequence ("... please") is a
    // distinct ask, which is the conservative behavior we want.
    let requests = vec![
        "morning standup".to_string(),
        "Morning standup".to_string(),
        "morning standup!".to_string(),
        "@opencrabsbot morning standup".to_string(),
        "morning  standup".to_string(),
        "unrelated one-off question about foo".to_string(),
    ];
    let out = command_candidates(&requests, 4);
    assert_eq!(out.len(), 1, "one pattern crosses the threshold");
    assert_eq!(out[0].signature, "morning standup");
    assert_eq!(out[0].count, 5, "all five phrasings normalize together");
    assert!(!out[0].samples.is_empty());
}

#[test]
fn below_threshold_is_not_a_candidate() {
    let requests = vec!["run the tests".to_string(), "run the tests".to_string()];
    assert!(command_candidates(&requests, COMMAND_PATTERN_THRESHOLD).is_empty());
}

#[test]
fn mentions_and_punctuation_normalize_together() {
    let requests = vec![
        "@bot deploy staging".to_string(),
        "deploy staging.".to_string(),
        "Deploy   Staging".to_string(),
        "deploy staging?".to_string(),
    ];
    let out = command_candidates(&requests, 4);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].signature, "deploy staging");
    assert_eq!(out[0].count, 4);
}

#[test]
fn trivial_and_prose_are_rejected() {
    // 1-word (too ambiguous), already-a-command, and long prose are all
    // dropped so they never inflate a candidate.
    let requests = vec![
        "yes".to_string(),
        "yes".to_string(),
        "yes".to_string(),
        "yes".to_string(),
        "/standup".to_string(),
        "/standup".to_string(),
        "/standup".to_string(),
        "/standup".to_string(),
        "could you please go and check the whole repository for any lingering issues".to_string(),
        "could you please go and check the whole repository for any lingering issues".to_string(),
        "could you please go and check the whole repository for any lingering issues".to_string(),
        "could you please go and check the whole repository for any lingering issues".to_string(),
    ];
    assert!(
        command_candidates(&requests, 4).is_empty(),
        "single words, slash commands, and prose are not command-shaped"
    );
}

#[test]
fn most_frequent_first_stable_order() {
    let mut requests = Vec::new();
    for _ in 0..6 {
        requests.push("build release".to_string());
    }
    for _ in 0..4 {
        requests.push("check status".to_string());
    }
    let out = command_candidates(&requests, 4);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].signature, "build release", "higher count first");
    assert_eq!(out[1].signature, "check status");
}
