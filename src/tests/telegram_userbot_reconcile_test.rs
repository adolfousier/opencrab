//! Receive-only userbot lifecycle and pure boundary tests.

use crate::channels::manager::{ChannelAction, userbot_action};
use crate::channels::telegram::userbot::capture::{chat_allowed, message_row_id};

#[test]
fn userbot_lifecycle_requires_opt_in_and_a_session() {
    assert_eq!(userbot_action(false, false, false), ChannelAction::Noop);
    assert_eq!(userbot_action(true, false, false), ChannelAction::Noop);
    assert_eq!(userbot_action(true, true, false), ChannelAction::Start);
    assert_eq!(userbot_action(false, true, true), ChannelAction::Stop);
    assert_eq!(userbot_action(true, true, true), ChannelAction::Noop);
}

#[test]
fn empty_allowlist_is_dry_and_numeric_matches_are_exact() {
    assert!(!chat_allowed(&[], -100123));
    let allowed = vec![" -100123 ".to_owned(), "777".to_owned()];
    assert!(chat_allowed(&allowed, -100123));
    assert!(chat_allowed(&allowed, 777));
    assert!(!chat_allowed(&allowed, 77));
}

#[test]
fn platform_row_id_is_stable_and_chat_scoped() {
    assert_eq!(message_row_id(-100123, 42), message_row_id(-100123, 42));
    assert_ne!(message_row_id(-100123, 42), message_row_id(-100123, 43));
    assert_ne!(message_row_id(-100123, 42), message_row_id(-100124, 42));
}

#[test]
fn credential_validation_rejects_lossy_or_malformed_values() {
    use crate::channels::telegram::userbot::resolve_creds;
    use crate::config::TelegramUserbotConfig;

    let valid = TelegramUserbotConfig {
        api_id: Some(25_625_345),
        api_hash: Some("0123456789abcdef0123456789abcdef".to_owned()),
        phone: Some("+254700000000".to_owned()),
        ..Default::default()
    };
    assert!(resolve_creds(&valid).is_ok());

    let mut bad = valid.clone();
    bad.api_id = Some(i64::from(i32::MAX) + 1);
    assert!(resolve_creds(&bad).is_err());
    bad = valid.clone();
    bad.api_hash = Some("not-hex".to_owned());
    assert!(resolve_creds(&bad).is_err());
    bad = valid;
    bad.phone = Some("254700000000".to_owned());
    assert!(resolve_creds(&bad).is_err());
}
