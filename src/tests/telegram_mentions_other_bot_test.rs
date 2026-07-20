//! Tests for `mentions_other_bot` — the group-addressing guard that stops us
//! answering a reply-to-us that explicitly tags a DIFFERENT bot (#648).
//!
//! No real handles: synthetic @ourbot / @otherbot examples only.

use crate::channels::telegram::handler::mentions_other_bot;

#[test]
fn tags_a_different_bot() {
    // The reported case: a reply to us that asks another bot to act.
    assert!(mentions_other_bot(
        "Verified. @otherservice_admin_bot can you update the issue?",
        Some("ourbot"),
    ));
}

#[test]
fn only_our_bot_tagged_is_not_another_bot() {
    assert!(!mentions_other_bot(
        "hey @ourbot please update",
        Some("ourbot")
    ));
    // Leading @ on our username is tolerated.
    assert!(!mentions_other_bot("hey @ourbot", Some("@ourbot")));
}

#[test]
fn both_us_and_another_bot_still_flags_the_other() {
    // Helper reports the other bot; the caller's own-mention check decides to
    // answer anyway when we are also tagged.
    assert!(mentions_other_bot("@ourbot and @otherbot", Some("ourbot")));
}

#[test]
fn human_mentions_do_not_count() {
    assert!(!mentions_other_bot(
        "@ourbot ask @john about it",
        Some("ourbot")
    ));
    assert!(!mentions_other_bot("thanks @alice_dev", Some("ourbot")));
}

#[test]
fn case_insensitive_and_underscored() {
    assert!(mentions_other_bot(
        "ping @Some_Admin_Tools_Bot",
        Some("ourbot")
    ));
    assert!(mentions_other_bot("ping @OTHERBOT", Some("ourbot")));
}

#[test]
fn no_mentions_or_unknown_self() {
    assert!(!mentions_other_bot("no tags here at all", Some("ourbot")));
    assert!(!mentions_other_bot("plain text", None));
    // A bare @bot is too short to be a real bot username.
    assert!(!mentions_other_bot("@bot", Some("ourbot")));
}
