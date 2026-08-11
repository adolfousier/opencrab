//! Tests for the one-shot proactive fallback-chain setup suggestion (#1008).

use crate::brain::agent::service::fallback_suggest::{
    SUGGESTION_NOTE, marker_path, maybe_inject, should_suggest,
};

#[test]
fn suggests_when_chain_missing_and_never_asked() {
    let home = tempfile::tempdir().unwrap();
    assert!(should_suggest(home.path(), false));
}

#[test]
fn silent_when_chain_configured() {
    let home = tempfile::tempdir().unwrap();
    assert!(!should_suggest(home.path(), true));
}

#[test]
fn one_shot_marker_blocks_repeat_nag() {
    let home = tempfile::tempdir().unwrap();
    let msg = maybe_inject(home.path(), false, "hello".to_string());
    assert!(
        msg.starts_with("[System: FALLBACK CHAIN SETUP"),
        "first real turn must carry the note"
    );
    assert!(msg.ends_with("hello"), "user message must be preserved");
    assert!(marker_path(home.path()).exists(), "marker must be written");

    let again = maybe_inject(home.path(), false, "hello again".to_string());
    assert_eq!(again, "hello again", "second turn must stay untouched");
}

#[test]
fn system_messages_never_carry_the_suggestion() {
    let home = tempfile::tempdir().unwrap();
    let msg = maybe_inject(home.path(), false, "[System: resume]".to_string());
    assert_eq!(msg, "[System: resume]");
    assert!(
        !marker_path(home.path()).exists(),
        "skipped turns must not burn the one-shot marker"
    );
}

#[test]
fn note_describes_the_full_guided_flow() {
    assert!(
        SUGGESTION_NOTE.contains("Yes let's setup a fallback provider now"),
        "must offer the tappable accept phrase"
    );
    assert!(
        SUGGESTION_NOTE.contains("/v1/models"),
        "must fetch models live"
    );
    assert!(
        SUGGESTION_NOTE.contains("[providers.fallback]"),
        "must name the config block"
    );
    assert!(
        SUGGESTION_NOTE.contains("keys.toml"),
        "must say where the key goes"
    );
    assert!(
        SUGGESTION_NOTE.contains("ONE step per message"),
        "must pace the guided flow"
    );
}
