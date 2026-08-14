//! Owner notification when a bot joins a Telegram chat (#1041).
//!
//! Two events, two notices. Being added somewhere needs different wording and
//! different advice from watching another bot arrive where we already are; one
//! notice for both made them indistinguishable in the owner's DM and told the
//! owner to allowlist OpenCrabs' own id.

use crate::channels::telegram::handler::{BotJoin, format_bot_join_notification};

const OTHER: BotJoin<'static> = BotJoin::Other {
    username: "atlas_bot",
    user_id: 8365623776,
};

#[test]
fn being_added_says_so_and_names_who_did_it() {
    let notify = format_bot_join_notification(
        BotJoin::Ourselves,
        "Test Group",
        -1001234567890,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("I was added"));
    assert!(notify.contains("Test Group"));
    assert!(notify.contains("-1001234567890"));
    assert!(notify.contains("adolfo"), "names the adder");
    assert!(notify.contains("5248691558"), "and their id");
}

#[test]
fn being_added_never_advises_allowlisting_ourselves() {
    // The original bug: the owner was told to add the bot's own id to
    // allowed_users, which is not an action anyone should take.
    let notify = format_bot_join_notification(
        BotJoin::Ourselves,
        "Test Group",
        -1001234567890,
        None,
        "adolfo",
        5248691558,
    );
    assert!(!notify.contains("allowed_users"));
}

#[test]
fn another_bot_arriving_is_worded_differently() {
    let notify = format_bot_join_notification(
        OTHER,
        "Test Group",
        -1001234567890,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("Another bot joined"));
    assert!(
        notify.contains("already in"),
        "says why the owner is hearing about this chat at all"
    );
    assert!(notify.contains("@atlas_bot"));
    assert!(notify.contains("8365623776"));
}

#[test]
fn another_bot_arriving_keeps_the_allowlist_advice() {
    let notify = format_bot_join_notification(
        OTHER,
        "Test Group",
        -1001234567890,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("allowed_users"));
    assert!(notify.contains("8365623776"), "the id to actually add");
}

#[test]
fn the_two_notices_are_never_confusable() {
    let mine =
        format_bot_join_notification(BotJoin::Ourselves, "G", -1, None, "adolfo", 5248691558);
    let theirs = format_bot_join_notification(OTHER, "G", -1, None, "adolfo", 5248691558);
    assert_ne!(mine, theirs);
}

#[test]
fn a_public_chat_carries_a_reachable_link() {
    // A numeric chat_id alone leaves the owner unable to find the group.
    let notify = format_bot_join_notification(
        OTHER,
        "Test Group",
        -1001234567890,
        Some("testgroup"),
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("https://t.me/testgroup"));
}

#[test]
fn a_private_chat_says_it_is_private_rather_than_going_quiet() {
    let notify = format_bot_join_notification(
        OTHER,
        "Test Group",
        -1001234567890,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("private chat with no public link"));
    assert!(!notify.contains("https://t.me/"));
}

#[test]
fn an_empty_username_is_treated_as_private() {
    let notify =
        format_bot_join_notification(OTHER, "Test Group", -1, Some(""), "adolfo", 5248691558);
    assert!(notify.contains("private chat"));
    assert!(!notify.contains("https://t.me/"));
}

#[test]
fn special_characters_in_the_title_survive() {
    let notify = format_bot_join_notification(
        OTHER,
        "Crabs & Claws <Group>",
        -1234,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("Crabs & Claws <Group>"));
    assert!(notify.contains("-1234"));
}

#[test]
fn large_ids_keep_their_precision() {
    let notify = format_bot_join_notification(
        BotJoin::Other {
            username: "bot",
            user_id: 1234567890123,
        },
        "G",
        9999999999999,
        None,
        "adolfo",
        5248691558,
    );
    assert!(notify.contains("9999999999999"));
    assert!(notify.contains("1234567890123"));
}
