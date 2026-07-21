//! Scoped redaction resolver (#677).
//!
//! `AgentConfig::redact_for(is_dm)` decides whether to scrub secrets for the
//! current chat scope: DMs are owner-private (default show), group/channel chats
//! follow the global flag (default redact), and an explicit per-scope override
//! (set via `/redact`) always wins.

use crate::config::AgentConfig;

#[test]
fn dm_defaults_to_showing_group_follows_global() {
    let cfg = AgentConfig {
        redact_sensitive_data: true,
        redact_dm: None,
        redact_group: None,
        ..Default::default()
    };
    // DM: owner-private, default = do NOT redact.
    assert!(!cfg.redact_for(true));
    // Group: follows the global flag (on here).
    assert!(cfg.redact_for(false));
}

#[test]
fn group_follows_global_flag_both_ways() {
    let off = AgentConfig {
        redact_sensitive_data: false,
        redact_group: None,
        ..Default::default()
    };
    assert!(!off.redact_for(false), "group follows global off");

    let on = AgentConfig {
        redact_sensitive_data: true,
        redact_group: None,
        ..Default::default()
    };
    assert!(on.redact_for(false), "group follows global on");
}

#[test]
fn explicit_scope_overrides_win() {
    // Redact in DM (override the default show), don't redact in groups.
    let cfg = AgentConfig {
        redact_sensitive_data: true,
        redact_dm: Some(true),
        redact_group: Some(false),
        ..Default::default()
    };
    assert!(cfg.redact_for(true), "dm override on wins over the default");
    assert!(
        !cfg.redact_for(false),
        "group override off wins over the global flag"
    );
}
