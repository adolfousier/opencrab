//! Tests for the Slack reaction mappings (#372).
//!
//! Slack reactions travel by name, not glyph, so both directions go
//! through mapping tables: inbound names feed the shared sentiment
//! classifier via a representative glyph, and the agent's chosen glyph
//! maps back to a valid Slack reaction name.

use crate::channels::slack::reactions::{glyph_for_slack_name, slack_name_for_glyph};
use crate::channels::telegram::reaction_prompt::{ReactionSentiment, classify_reaction};

#[test]
fn positive_and_negative_names_classify_like_telegram() {
    for name in ["+1", "thumbsup", "fire", "100", "muscle", "ok_hand"] {
        assert_eq!(
            classify_reaction(glyph_for_slack_name(name)),
            ReactionSentiment::Positive,
            "{name} should read as approval"
        );
    }
    for name in [
        "-1",
        "thumbsdown",
        "no_entry",
        "octagonal_sign",
        "no_entry_sign",
    ] {
        assert_eq!(
            classify_reaction(glyph_for_slack_name(name)),
            ReactionSentiment::Negative,
            "{name} should read as a stop signal"
        );
    }
    assert_eq!(
        classify_reaction(glyph_for_slack_name("bento")),
        ReactionSentiment::Neutral
    );
}

#[test]
fn agent_glyphs_map_to_valid_slack_names() {
    assert_eq!(slack_name_for_glyph("🔥"), "fire");
    assert_eq!(slack_name_for_glyph("✅"), "white_check_mark");
    assert_eq!(slack_name_for_glyph("🎉"), "tada");
    // Variation selector stripped.
    assert_eq!(slack_name_for_glyph("❤️"), "heart");
    // Unknown glyphs degrade to thumbsup, never an invalid name.
    assert_eq!(slack_name_for_glyph("🦀"), "thumbsup");
}
