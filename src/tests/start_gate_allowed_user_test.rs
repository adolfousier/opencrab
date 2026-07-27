//! `/start` in a closed group must recognise an existing member (#776).
//!
//! The gate tested only the group's `open` flag, so an already-allowed user
//! running `/start` was told "This group isn't open yet. Ask the owner to run
//! /cowork here, or to add your ID." Their id was already on the list, so the
//! instruction was to request access they had.
//!
//! `open` gates SELF-REGISTRATION by strangers. It says nothing about whether
//! an existing member may chat, and conflating the two treated a member like
//! an outsider.
//!
//! These pin the decision the gate makes, via the same `user_allowed` the rest
//! of the ACL uses.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::config::types::{TelegramConfig, TelegramGroupConfig};
use std::collections::HashMap;

const CHAT: &str = "-1001234567890";
const MEMBER: &str = "111111";
const STRANGER: &str = "222222";
const OWNER: &str = "999999";

/// A closed group (`open = false`) with one allow-listed member.
fn closed_group_with_member() -> TelegramConfig {
    let mut groups = HashMap::new();
    groups.insert(
        CHAT.to_string(),
        TelegramGroupConfig {
            open: false,
            allowed_users: vec![MEMBER.to_string()],
            ..Default::default()
        },
    );
    TelegramConfig {
        bot_owner: vec![OWNER.to_string()],
        groups,
        ..Default::default()
    }
}

#[test]
fn an_allow_listed_member_is_recognised_in_a_closed_group() {
    // The reported bug: this returned false, so the gate told a member to ask
    // for access they already had.
    let cfg = closed_group_with_member();
    assert!(
        cfg.user_allowed(MEMBER, CHAT, false),
        "an allow-listed member must be recognised even when the group is closed"
    );
}

#[test]
fn a_stranger_is_still_refused_in_a_closed_group() {
    // The security property the gate exists for must survive the fix.
    let cfg = closed_group_with_member();
    assert!(!cfg.user_allowed(STRANGER, CHAT, false));
}

#[test]
fn the_owner_is_allowed_anywhere() {
    let cfg = closed_group_with_member();
    assert!(cfg.user_allowed(OWNER, CHAT, false));
}

#[test]
fn an_open_group_admits_anyone() {
    // `open` is what self-registration keys off, and it still does.
    let mut cfg = closed_group_with_member();
    if let Some(g) = cfg.groups.get_mut(CHAT) {
        g.open = true;
    }
    assert!(cfg.user_allowed(STRANGER, CHAT, false));
}

#[test]
fn group_membership_does_not_leak_into_dms() {
    // Being allowed in a group must not admit the same user in a DM, or the
    // bot could be moved to a private chat to escape group oversight.
    let cfg = closed_group_with_member();
    assert!(!cfg.user_allowed(MEMBER, CHAT, true));
}

#[test]
fn an_unconfigured_bot_denies_everyone() {
    // Secure by default: no allow-list and no owner means no access.
    let cfg = TelegramConfig::default();
    assert!(!cfg.user_allowed(MEMBER, CHAT, false));
}

#[test]
fn an_unknown_chat_admits_nobody() {
    // A group with no config entry is not implicitly open.
    let cfg = closed_group_with_member();
    assert!(!cfg.user_allowed(MEMBER, "-1009999999999", false));
}
