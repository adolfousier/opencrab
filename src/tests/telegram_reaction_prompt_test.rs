//! Tests for reaction sentiment classification and prompt framing (#302 Stage 1).

use crate::channels::telegram::reaction_prompt::{
    ReactionSentiment, build_midturn_reaction_message, build_reaction_prompt, classify_reaction,
};

#[test]
fn positive_emojis_classify_positive() {
    for e in ["👍", "👌", "💪", "🫡", "🆗", "🔥", "💯"] {
        assert_eq!(
            classify_reaction(e),
            ReactionSentiment::Positive,
            "{e} should be positive"
        );
    }
}

#[test]
fn negative_emojis_classify_negative() {
    for e in ["⛔️", "⛔", "🚫", "🛑", "👎"] {
        assert_eq!(
            classify_reaction(e),
            ReactionSentiment::Negative,
            "{e} should be negative"
        );
    }
}

#[test]
fn unknown_emoji_classifies_neutral() {
    for e in ["🎉", "❤️", "🤔", "🍕"] {
        assert_eq!(classify_reaction(e), ReactionSentiment::Neutral);
    }
}

#[test]
fn classification_ignores_surrounding_whitespace() {
    assert_eq!(classify_reaction(" 👍 "), ReactionSentiment::Positive);
}

#[test]
fn prompt_addresses_user_by_first_name() {
    let out = build_reaction_prompt("Adolfo", "🔥", "the fold fix is committed", false);
    // First name appears, not the generic User "..." framing.
    assert!(out.contains("Adolfo"));
    assert!(!out.contains("User \""));
    // Preview and emoji are carried through.
    assert!(out.contains("the fold fix is committed"));
    assert!(out.contains("🔥"));
    // The react-back affordance survives for silent acks.
    assert!(out.contains("<<react:🔥>>"));
}

#[test]
fn positive_reaction_defaults_to_react_only() {
    // A positive reaction is an ack, not a request: default to reacting back
    // with no text, not writing a paragraph (the group-spam bug, #309).
    let out = build_reaction_prompt("Carlos", "💯", "shipping it now", false);
    let lower = out.to_lowercase();
    assert!(lower.contains("no text"));
    assert!(lower.contains("react"));
    // Must not carry the old "keep the momentum / proceed" text bias.
    assert!(!lower.contains("keep the momentum"));
}

#[test]
fn group_reaction_hardens_react_only_but_dm_does_not() {
    // In a group a text reply per reaction is noise, so the steer says so;
    // a DM keeps the plain react-only default without the group note.
    let group = build_reaction_prompt("Carlos", "👍", "done", true);
    let dm = build_reaction_prompt("Carlos", "👍", "done", false);
    assert!(group.to_lowercase().contains("group"));
    assert!(!dm.to_lowercase().contains("this is a group"));
}

#[test]
fn negative_prompt_frames_pause_and_ask() {
    // The one sentiment that DOES warrant text: pause and ask what to change.
    let out = build_reaction_prompt("Felipe", "👎", "here is the plan", true);
    assert!(out.to_lowercase().contains("pause"));
    assert!(out.to_lowercase().contains("ask"));
}

#[test]
fn midturn_positive_frames_keep_going() {
    let out = build_midturn_reaction_message("Adolfo", "🔥");
    assert!(out.contains("Adolfo"));
    assert!(out.contains("Live feedback"));
    assert!(out.to_lowercase().contains("keep going"));
}

#[test]
fn midturn_negative_frames_pause() {
    let out = build_midturn_reaction_message("Carlos", "🛑");
    assert!(out.contains("Carlos"));
    assert!(out.contains("PAUSE"));
    assert!(out.to_lowercase().contains("ask"));
}

#[test]
fn midturn_neutral_still_addresses_by_name() {
    let out = build_midturn_reaction_message("Ruhul", "🎉");
    assert!(out.contains("Ruhul"));
    assert!(out.contains("Live feedback"));
}
