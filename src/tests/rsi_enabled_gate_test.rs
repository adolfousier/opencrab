//! #1063: the RSI master gate. `rsi_enabled` is an `Option<bool>` so an
//! absent key can default by run mode (TUI on, headless daemon off) instead
//! of being indistinguishable from an explicit `true`. These pin the parse
//! semantics and the `rsi_effectively_enabled` resolution matrix that the
//! boot gate and the per-cycle hot-reload re-check both consume.

use crate::brain::rsi::rsi_effectively_enabled;
use crate::config::Config;

fn config_with_agent(toml_fragment: &str) -> Config {
    toml::from_str(&format!("[agent]\n{toml_fragment}")).expect("agent config must parse")
}

#[test]
fn absent_key_is_none_not_a_silent_true() {
    // The whole fix rests on this distinction: None means "user did not
    // decide", which resolves by run mode. A plain bool default could not
    // tell an explicit opt-in from a never-configured install.
    let cfg = config_with_agent("");
    assert_eq!(cfg.agent.rsi_enabled, None);
}

#[test]
fn explicit_values_are_honored_verbatim() {
    assert_eq!(
        config_with_agent("rsi_enabled = true").agent.rsi_enabled,
        Some(true)
    );
    assert_eq!(
        config_with_agent("rsi_enabled = false").agent.rsi_enabled,
        Some(false)
    );
}

#[test]
fn mode_defaults_tui_on_daemon_off() {
    let cfg = config_with_agent("");
    assert!(
        rsi_effectively_enabled(&cfg, false),
        "TUI (headless=false) keeps RSI on: the interactive feature users expect"
    );
    assert!(
        !rsi_effectively_enabled(&cfg, true),
        "headless daemon defaults OFF: unattended hourly cycles are quota burn (#1063)"
    );
}

#[test]
fn explicit_overrides_mode_defaults_in_both_directions() {
    let off = config_with_agent("rsi_enabled = false");
    assert!(
        !rsi_effectively_enabled(&off, false),
        "explicit false wins over the TUI-on default"
    );
    let on = config_with_agent("rsi_enabled = true");
    assert!(
        rsi_effectively_enabled(&on, true),
        "explicit true wins over the daemon-off default (knowing opt-in)"
    );
}

#[test]
fn example_config_documents_the_gate() {
    // config.toml.example is the embedded fallback config (see
    // config::current::embedded_default), so the key must parse there too.
    let example = include_str!("../../config.toml.example");
    let parsed: Config = toml::from_str(example).expect("config.toml.example must parse");
    assert!(
        parsed.agent.rsi_enabled.is_none(),
        "example ships the key commented out, so the shipped default is mode-based"
    );
}
