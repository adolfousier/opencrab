//! Group-name recording for `[channels.telegram.groups.<chat_id>]` (#984).
//!
//! Covers the two pure decisions: what an observed Telegram title normalises
//! to before it reaches config, and when a stored name is stale enough to
//! rewrite. The write itself goes through `Config::write_key_string`, which
//! owns the file lock and the parse guard.

use crate::channels::telegram::group_name::{needs_update, sanitize};
use crate::config::types::{TelegramConfig, TelegramGroupConfig};

#[test]
fn keeps_an_ordinary_title() {
    assert_eq!(sanitize("Release Crew"), Some("Release Crew".to_string()));
}

#[test]
fn collapses_whitespace_and_newlines() {
    // Titles carry decorative spacing, and a newline would otherwise sit
    // inside the TOML string.
    assert_eq!(
        sanitize("  Release\n\tCrew   2  "),
        Some("Release Crew 2".to_string())
    );
}

#[test]
fn strips_control_characters() {
    assert_eq!(
        sanitize("Release\u{0}Crew\u{7}"),
        Some("Release Crew".to_string())
    );
}

#[test]
fn rejects_a_title_with_nothing_printable() {
    assert_eq!(sanitize(""), None);
    assert_eq!(sanitize("   \n\t  "), None);
    assert_eq!(sanitize("\u{0}\u{1}"), None);
}

#[test]
fn bounds_the_length() {
    let long = "x".repeat(400);
    let out = sanitize(&long).expect("printable");
    assert_eq!(out.chars().count(), 128);
}

#[test]
fn truncates_on_a_character_boundary() {
    // Every char is multi-byte, so a byte-wise cut would panic or corrupt.
    let long = "\u{1F980}".repeat(400);
    let out = sanitize(&long).expect("printable");
    assert_eq!(out.chars().count(), 128);
    assert!(out.chars().all(|c| c == '\u{1F980}'));
}

#[test]
fn writes_when_no_name_is_stored_yet() {
    assert!(needs_update(None, "Release Crew"));
}

#[test]
fn writes_when_the_group_was_renamed() {
    assert!(needs_update(Some("Release Crew"), "Shipping Crew"));
}

#[test]
fn stays_quiet_when_the_name_already_matches() {
    // This is the every-message path: an unchanged title must not write.
    assert!(!needs_update(Some("Release Crew"), "Release Crew"));
}

#[test]
fn loads_a_hand_written_name_that_reads_as_a_number() {
    // A group can legitimately be called "2026", and a hand-edited config
    // writes it unquoted. Failing the whole config load over a display-only
    // field would be a bad trade.
    let g: TelegramGroupConfig = toml::from_str("name = 2026").expect("loads");
    assert_eq!(g.name.as_deref(), Some("2026"));

    let g: TelegramGroupConfig = toml::from_str("name = true").expect("loads");
    assert_eq!(g.name.as_deref(), Some("true"));
}

#[test]
fn an_absent_name_stays_none() {
    let g: TelegramGroupConfig = toml::from_str("open = true").expect("loads");
    assert_eq!(g.name, None);
    assert!(g.open);
}

#[test]
fn name_never_affects_access_control() {
    // The stored title is attacker-controlled in any group the bot can be
    // invited to, so naming a group must not move the ACL by itself.
    let mut tg = TelegramConfig {
        allowed_users: vec!["1".to_string()],
        ..Default::default()
    };
    tg.groups.insert(
        "-100".to_string(),
        TelegramGroupConfig {
            name: Some("Release Crew".to_string()),
            ..Default::default()
        },
    );
    assert!(!tg.user_allowed("2", "-100", false));
    assert!(!tg.user_allowed("2", "-100", true));
    // The owner is unaffected either way.
    assert!(tg.user_allowed("1", "-100", false));
}
