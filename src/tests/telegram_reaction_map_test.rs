//! Tests for `map_to_allowed_reaction` (#353): Telegram message reactions
//! only accept a fixed emoji set, and a rejected reaction on a reaction-only
//! turn meant the user got NOTHING (four silent turns in one day). Every
//! outgoing reaction is mapped onto the allowed set first.

use crate::channels::telegram::handler::map_to_allowed_reaction;

#[test]
fn allowed_emojis_pass_through() {
    for e in ["👍", "🔥", "👀", "🎉", "💯", "🤝", "🤣", "🏆", "⚡"] {
        assert_eq!(map_to_allowed_reaction(e), e, "{e} must pass through");
    }
}

#[test]
fn variation_selector_is_normalized() {
    // Models emit the VS16 form (❤️); the API list uses the bare char (❤).
    assert_eq!(map_to_allowed_reaction("❤️"), "❤");
    assert_eq!(map_to_allowed_reaction("✍️"), "✍");
}

#[test]
fn common_out_of_set_picks_are_aliased() {
    assert_eq!(map_to_allowed_reaction("😂"), "🤣");
    assert_eq!(map_to_allowed_reaction("😊"), "😁");
    assert_eq!(map_to_allowed_reaction("🚀"), "🔥");
    assert_eq!(map_to_allowed_reaction("🙌"), "👏");
    assert_eq!(map_to_allowed_reaction("⭐"), "🤩");
}

#[test]
fn rejected_real_world_picks_fall_back_to_thumbs_up() {
    // The two emojis that produced today's REACTION_INVALID silent turns.
    assert_eq!(map_to_allowed_reaction("🦀"), "👍");
    assert_eq!(map_to_allowed_reaction("✅"), "👍");
}

#[test]
fn unknown_and_whitespace_wrapped_input_falls_back() {
    assert_eq!(map_to_allowed_reaction("🧿"), "👍");
    assert_eq!(map_to_allowed_reaction("  🔥  "), "🔥");
}
