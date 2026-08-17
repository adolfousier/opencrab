//! write_opencrabs_file tool.
//!
//! Moved out of `src/brain/tools/write_opencrabs_file.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::brain::tools::write_opencrabs_file::*;

#[test]
fn memory_belief_skips_noise() {
    assert!(memory_belief_key_value("").is_none());
    assert!(memory_belief_key_value("## Header line here").is_none());
    assert!(memory_belief_key_value("---").is_none());
    assert!(memory_belief_key_value("*italic note line*").is_none());
    assert!(memory_belief_key_value("too short").is_none());
}

#[test]
fn memory_belief_tracks_rule_line() {
    let line = "- NEVER push without explicit user approval";
    let (key, value) = memory_belief_key_value(line).expect("rule line should track");
    assert!(key.starts_with("memory:"));
    assert_eq!(value, line);
}

#[test]
fn memory_belief_same_topic_same_key() {
    let (k1, _) = memory_belief_key_value("- NEVER push without explicit user approval").unwrap();
    let (k2, _) =
        memory_belief_key_value("- NEVER push without explicit user approval ever").unwrap();
    assert_eq!(k1, k2);
}

#[test]
fn memory_belief_different_topic_different_key() {
    let (k1, _) = memory_belief_key_value("- NEVER push without explicit user approval").unwrap();
    let (k2, _) = memory_belief_key_value("- ALWAYS run clippy before every commit").unwrap();
    assert_ne!(k1, k2);
}
