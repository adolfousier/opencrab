//! Telegram userbot config tests ([channels.telegram.userbot]).
//!
//! The userbot is a read-plane companion to the bot: grammers MTProto user
//! session, secrets in keys.toml, dry by default (empty `allowed_chats`),
//! strictly read-only by default (empty `outbound_allowlist`). These tests
//! pin the two invariants a mis-parse would silently break:
//!
//! 1. A config WITHOUT the table parses to the safe default (disabled, dry,
//!    read-only) — the feature must opt in, never opt out.
//! 2. `merge_channel_keys` overlays api_id/api_hash/phone from keys.toml
//!    onto config.toml values, and the EXISTING_KEY sentinel on api_hash
//!    resolves to the real key (or None) via `real_key` — the #1075 lesson.

use crate::config::{ChannelsConfig, TelegramConfig, TelegramUserbotConfig};

fn channels_from_toml(text: &str) -> ChannelsConfig {
    toml::from_str::<ChannelsConfig>(text).expect("channels config should parse")
}

fn base_channels() -> ChannelsConfig {
    let mut c = ChannelsConfig::default();
    c.telegram = TelegramConfig::default();
    c
}

#[test]
fn absent_userbot_table_is_disabled_dry_and_read_only() {
    let cfg = channels_from_toml("[telegram]\nenabled = true\ntoken = \"x\"\n");
    let ub = &cfg.telegram.userbot;
    assert!(!ub.enabled, "userbot must default to disabled");
    assert!(ub.allowed_chats.is_empty(), "no forwarding by default");
    assert!(
        ub.outbound_allowlist.is_empty(),
        "sending as the user must never be enabled by default"
    );
    assert!(ub.api_hash.is_none() && ub.api_id.is_none() && ub.phone.is_none());
}

#[test]
fn userbot_table_parses_fully() {
    let cfg = channels_from_toml(
        r#"
[telegram]
enabled = true

[telegram.userbot]
enabled = true
api_id = 25625345
phone = "+254700000000"
allowed_chats = ["-1001234567890", "777"]
outbound_allowlist = []
"#,
    );
    let ub = &cfg.telegram.userbot;
    assert!(ub.enabled);
    assert_eq!(ub.api_id, Some(25625345));
    assert_eq!(ub.phone.as_deref(), Some("+254700000000"));
    assert_eq!(ub.allowed_chats, vec!["-1001234567890", "777"]);
    assert!(ub.outbound_allowlist.is_empty());
}

#[test]
fn keys_toml_overlays_userbot_secrets() {
    let mut base = base_channels();
    // config.toml side: enabled but no secrets
    base.telegram.userbot = TelegramUserbotConfig {
        enabled: true,
        ..Default::default()
    };

    let keys = channels_from_toml(
        r#"
[telegram.userbot]
api_id = 25625345
api_hash = "0123456789abcdef0123456789abcdef"
phone = "+254700000000"
"#,
    );

    let merged = crate::config::merge_channel_keys(base, keys);
    let ub = &merged.telegram.userbot;
    assert!(ub.enabled, "merge must not flip enabled");
    assert_eq!(ub.api_id, Some(25625345));
    assert_eq!(
        ub.api_hash.as_deref(),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert_eq!(ub.phone.as_deref(), Some("+254700000000"));
}

#[test]
fn empty_keys_secrets_do_not_clobber_config_values() {
    let mut base = base_channels();
    base.telegram.userbot = TelegramUserbotConfig {
        enabled: true,
        api_id: Some(1),
        api_hash: Some("confighash".into()),
        phone: Some("+254700000001".into()),
        ..Default::default()
    };

    let keys = channels_from_toml("[telegram.userbot]\nenabled = false\n");

    let merged = crate::config::merge_channel_keys(base, keys);
    let ub = &merged.telegram.userbot;
    assert_eq!(ub.api_hash.as_deref(), Some("confighash"));
    assert_eq!(ub.api_id, Some(1));
    assert_eq!(ub.phone.as_deref(), Some("+254700000001"));
}

#[test]
fn sentinel_api_hash_resolves_to_real_key() {
    // A key typed after the #1075 seed marker must surface as the typed key,
    // not the marker; the bare marker means "keep what's on disk" (None here).
    let mut base = base_channels();
    base.telegram.userbot = TelegramUserbotConfig {
        enabled: true,
        ..Default::default()
    };
    let keys = channels_from_toml(
        r#"
[telegram.userbot]
api_hash = "__EXISTING_KEY__abcdef0123456789abcdef0123456789"
"#,
    );
    let merged = crate::config::merge_channel_keys(base.clone(), keys);
    assert_eq!(
        merged.telegram.userbot.api_hash.as_deref(),
        Some("abcdef0123456789abcdef0123456789")
    );

    let keys_bare = channels_from_toml(
        r#"
[telegram.userbot]
api_hash = "__EXISTING_KEY__"
"#,
    );
    let merged_bare = crate::config::merge_channel_keys(base, keys_bare);
    assert!(
        merged_bare.telegram.userbot.api_hash.is_none(),
        "bare sentinel means unchanged, not a literal marker secret"
    );
}
