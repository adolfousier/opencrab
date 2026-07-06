//! Per-chat ACL for Telegram: global admins + owner act anywhere, per-group
//! `allowed_users` grant access to that group only (never DMs), and the DM gate
//! closes the "DM the bot privately to escape group oversight" bypass. Also
//! covers per-group `respond_to` override resolution.

use crate::config::types::{RespondTo, TelegramConfig, TelegramGroupConfig};
use std::collections::HashMap;

fn cfg(allowed: &[&str], owner: &[&str], groups: &[(&str, &[&str])]) -> TelegramConfig {
    let mut g = HashMap::new();
    for (id, users) in groups {
        g.insert(
            (*id).to_string(),
            TelegramGroupConfig {
                allowed_users: users.iter().map(|s| s.to_string()).collect(),
                respond_to: None,
            },
        );
    }
    TelegramConfig {
        allowed_users: allowed.iter().map(|s| s.to_string()).collect(),
        bot_owner: owner.iter().map(|s| s.to_string()).collect(),
        groups: g,
        ..Default::default()
    }
}

#[test]
fn admin_allowed_in_dm_and_group() {
    let c = cfg(&["111"], &["111"], &[("-100g", &["222"])]);
    assert!(c.user_allowed("111", "111", true), "admin may DM");
    assert!(
        c.user_allowed("111", "-100g", false),
        "admin may act in groups"
    );
}

#[test]
fn group_user_allowed_only_in_that_group_never_dm() {
    let c = cfg(&["111"], &["111"], &[("-100g", &["222"])]);
    assert!(
        c.user_allowed("222", "-100g", false),
        "group member allowed in their group"
    );
    assert!(
        !c.user_allowed("222", "222", true),
        "group member refused in DM (the bypass fix)"
    );
    assert!(
        !c.user_allowed("222", "-100other", false),
        "group member refused in a different group"
    );
}

#[test]
fn unknown_user_refused_everywhere() {
    let c = cfg(&["111"], &["111"], &[("-100g", &["222"])]);
    assert!(!c.user_allowed("999", "999", true));
    assert!(!c.user_allowed("999", "-100g", false));
}

#[test]
fn owner_only_dm_when_no_admins() {
    // allowed_users empty but owner set => only the owner can DM.
    let c = cfg(&[], &["111"], &[]);
    assert!(c.user_allowed("111", "111", true), "owner may DM");
    assert!(
        !c.user_allowed("222", "222", true),
        "non-owner refused in DM when only the owner is configured"
    );
}

#[test]
fn fully_unconfigured_denies_access() {
    // No admins and no owner => unconfigured, denies access (secure by default).
    let c = cfg(&[], &[], &[]);
    assert!(!c.user_allowed("anyone", "x", true));
    assert!(!c.user_allowed("anyone", "-100g", false));
}

#[test]
fn respond_to_group_override_wins_else_global() {
    let mut c = cfg(&["111"], &["111"], &[]);
    c.respond_to = RespondTo::Mention;
    c.groups.insert(
        "-100g".to_string(),
        TelegramGroupConfig {
            allowed_users: vec![],
            respond_to: Some(RespondTo::All),
        },
    );
    assert_eq!(
        c.respond_to_for("-100g"),
        RespondTo::All,
        "group override wins"
    );
    assert_eq!(
        c.respond_to_for("-100other"),
        RespondTo::Mention,
        "falls back to global when no override"
    );
}

#[test]
fn id_normalizes_leading_plus() {
    let c = cfg(&["+111"], &["+111"], &[]);
    assert!(c.user_allowed("111", "111", true));
}

#[test]
fn respond_to_group_with_mention_override() {
    let mut c = cfg(&["111"], &["111"], &[]);
    c.respond_to = RespondTo::All;
    c.groups.insert(
        "-100g".to_string(),
        TelegramGroupConfig {
            allowed_users: vec![],
            respond_to: Some(RespondTo::Mention),
        },
    );
    assert_eq!(
        c.respond_to_for("-100g"),
        RespondTo::Mention,
        "group can restrict to mention-only even when global is all"
    );
    assert_eq!(
        c.respond_to_for("-100other"),
        RespondTo::All,
        "other groups fall back to global"
    );
}

#[test]
fn respond_to_group_with_auto_override() {
    let mut c = cfg(&["111"], &["111"], &[]);
    c.respond_to = RespondTo::Mention;
    c.groups.insert(
        "-100g".to_string(),
        TelegramGroupConfig {
            allowed_users: vec![],
            respond_to: Some(RespondTo::Auto),
        },
    );
    assert_eq!(
        c.respond_to_for("-100g"),
        RespondTo::Auto,
        "group can set auto mode"
    );
    assert_eq!(
        c.respond_to_for("-100other"),
        RespondTo::Mention,
        "other groups get global mention"
    );
}

#[test]
fn respond_to_no_group_override_uses_global() {
    let mut c = cfg(&["111"], &["111"], &[("-100g", &["222"])]);
    c.respond_to = RespondTo::Auto;
    // No respond_to set on the group => should fall back to global
    assert_eq!(
        c.respond_to_for("-100g"),
        RespondTo::Auto,
        "group without override falls back to global auto"
    );
}
