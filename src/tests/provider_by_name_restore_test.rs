//! Regression for #270: `create_provider_by_name` promises to ignore the
//! `enabled` flag (it restores session-pinned providers), but the CLI
//! factories gate on `cfg.enabled` internally, so a session pinned to
//! claude-cli whose config section was disabled (or absent) could never be
//! restored after a restart and silently landed on another provider.
//!
//! `force_enable_section` is the retry hook: it flips the named provider's
//! section to enabled in a CLONED config, synthesizing a default section for
//! the keyless CLI providers when missing. Keyed providers without a section
//! stay un-creatable (there is no API key to use anyway).

use crate::brain::provider::factory::force_enable_section;
use crate::config::{Config, ProviderConfig};

#[test]
fn disabled_section_is_enabled() {
    let mut cfg = Config::default();
    cfg.providers.claude_cli = Some(ProviderConfig {
        enabled: false,
        ..ProviderConfig::default()
    });

    assert!(force_enable_section(&mut cfg, "claude-cli"));
    assert!(cfg.providers.claude_cli.as_ref().unwrap().enabled);
}

#[test]
fn absent_cli_section_is_synthesized() {
    // CLI providers authenticate through their own login, not an API key —
    // a missing section must not block restoring a pinned session.
    let mut cfg = Config::default();
    cfg.providers.claude_cli = None;
    cfg.providers.opencode_cli = None;
    cfg.providers.codex_cli = None;

    for id in ["claude-cli", "opencode-cli", "codex-cli"] {
        assert!(force_enable_section(&mut cfg, id), "{id} must synthesize");
    }
    assert!(cfg.providers.claude_cli.as_ref().unwrap().enabled);
    assert!(cfg.providers.opencode_cli.as_ref().unwrap().enabled);
    assert!(cfg.providers.codex_cli.as_ref().unwrap().enabled);
}

#[test]
fn absent_keyed_section_stays_uncreatable() {
    // No section means no API key — nothing to force.
    let mut cfg = Config::default();
    cfg.providers.anthropic = None;

    assert!(!force_enable_section(&mut cfg, "anthropic"));
    assert!(cfg.providers.anthropic.is_none());
}

#[test]
fn disabled_keyed_section_is_enabled() {
    let mut cfg = Config::default();
    cfg.providers.zhipu = Some(ProviderConfig {
        enabled: false,
        api_key: Some("test-key".into()),
        ..ProviderConfig::default()
    });

    assert!(force_enable_section(&mut cfg, "zhipu"));
    assert!(cfg.providers.zhipu.as_ref().unwrap().enabled);
    // The rest of the section is untouched.
    assert_eq!(
        cfg.providers.zhipu.as_ref().unwrap().api_key.as_deref(),
        Some("test-key")
    );
}

#[test]
fn unknown_and_custom_ids_are_untouched() {
    let mut cfg = Config::default();
    assert!(!force_enable_section(&mut cfg, "custom"));
    assert!(!force_enable_section(&mut cfg, "no-such-provider"));
}
