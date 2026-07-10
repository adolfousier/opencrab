//! Tests for the recurring tool-sequence -> skill detector (#504): counts
//! DISTINCT sessions per n-gram, ignores single-tool runs and within-session
//! repetition, and groups flat rows into per-session sequences.

use crate::brain::rsi_skill_sequences::{SEQUENCE_LEN, group_sessions, skill_sequence_candidates};

#[test]
fn sequence_across_enough_sessions_is_a_candidate() {
    // read_file -> edit_file -> bash recurs in 3 distinct sessions.
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let sessions = vec![
        s(&["grep", "read_file", "edit_file", "bash"]),
        s(&["read_file", "edit_file", "bash", "write_file"]),
        s(&["read_file", "edit_file", "bash"]),
        s(&["read_file", "grep"]), // no matching 3-gram
    ];
    let out = skill_sequence_candidates(&sessions, 3, 3);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].sequence, vec!["read_file", "edit_file", "bash"]);
    assert_eq!(out[0].sessions, 3);
}

#[test]
fn within_session_repetition_does_not_inflate() {
    // One session repeats the sequence many times; it still counts as ONE
    // session, so it stays below a 3-session threshold.
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut single = Vec::new();
    for _ in 0..10 {
        single.extend(s(&["read_file", "edit_file", "bash"]));
    }
    let sessions = vec![single];
    assert!(
        skill_sequence_candidates(&sessions, 3, 3).is_empty(),
        "a loop in one session is not a cross-session workflow"
    );
}

#[test]
fn single_tool_runs_are_ignored() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let sessions = vec![
        s(&["bash", "bash", "bash"]),
        s(&["bash", "bash", "bash"]),
        s(&["bash", "bash", "bash"]),
    ];
    assert!(
        skill_sequence_candidates(&sessions, 3, 3).is_empty(),
        "a tight loop of one tool is not a workflow"
    );
}

#[test]
fn most_widespread_first_stable_order() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let common = s(&["read_file", "edit_file", "bash"]);
    let rare = s(&["grep", "read_file", "write_file"]);
    let sessions = vec![
        common.clone(),
        common.clone(),
        common.clone(),
        common.clone(),
        rare.clone(),
        rare.clone(),
        rare,
    ];
    let out = skill_sequence_candidates(&sessions, 3, 3);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].sequence, vec!["read_file", "edit_file", "bash"]);
    assert_eq!(out[0].sessions, 4, "more widespread first");
}

#[test]
fn group_sessions_splits_on_session_id_change() {
    let rows = vec![
        ("s1".to_string(), "a".to_string()),
        ("s1".to_string(), "b".to_string()),
        ("s2".to_string(), "c".to_string()),
        ("s1".to_string(), "d".to_string()), // s1 again after s2 = new group
    ];
    let grouped = group_sessions(&rows);
    assert_eq!(grouped.len(), 3);
    assert_eq!(grouped[0], vec!["a", "b"]);
    assert_eq!(grouped[1], vec!["c"]);
    assert_eq!(grouped[2], vec!["d"]);
}

#[test]
fn default_sequence_len_is_three() {
    assert_eq!(SEQUENCE_LEN, 3);
}
