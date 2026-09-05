//! Nested config paths resolve to their top-level section (#889).
//!
//! Config is nested but `config_manager` rendered only the first level, so the
//! paths people actually write were rejected. Every recorded failure was one of
//! `providers.stt`, `stt` or `telegram` — the real shapes in config.toml.
//!
//! RSI had already tried to patch this from the other end by writing a brain
//! rule listing the valid names. That is guidance papering over an interface
//! gap: the rule decays, accepting the path does not.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::config::sections::{resolve_section, validate_write_path};

#[test]
fn the_observed_failures_now_resolve() {
    // The exact strings from the recorded failures.
    assert_eq!(resolve_section("providers.stt"), Some("providers"));
    assert_eq!(resolve_section("stt"), Some("providers"));
    assert_eq!(resolve_section("telegram"), Some("channels"));
}

#[test]
fn an_exact_section_is_unchanged() {
    for s in [
        "agent",
        "a2a",
        "brain",
        "browser",
        "channels",
        "cron",
        "daemon",
        "database",
        "debug",
        "doctor",
        "image",
        "logging",
        "memory",
        "provider_registry",
        "providers",
        "tui",
    ] {
        assert_eq!(resolve_section(s), Some(s), "rewrote {s}");
    }
}

#[test]
fn voice_is_a_derived_view_not_a_writable_section() {
    // #1385: `voice` reads as a section through the config tool's derived
    // view (dispatched before resolution), but it is not a table in
    // config.toml. Resolution returns None so the read fallback keeps
    // working, and writes are refused with a pointer at the real tables.
    assert_eq!(resolve_section("voice"), None);
    let err = validate_write_path("voice.stt_mode").unwrap_err();
    assert!(
        err.contains("providers.stt"),
        "voice write must name the real tables, got: {err}"
    );
}

#[test]
fn every_real_section_accepts_writes() {
    // The #1385 drift: eight real sections were refused by the write gate.
    for s in [
        "daemon", "a2a", "image", "cron", "memory", "brain", "browser", "doctor",
    ] {
        let dotted = format!("{s}.anything");
        assert!(
            validate_write_path(&dotted).is_ok(),
            "refused real section {s}"
        );
    }
}

#[test]
fn a_deep_path_takes_its_head() {
    // config.toml nests further than one level; the head is what this tool
    // can render.
    assert_eq!(
        resolve_section("providers.custom.modelstudio"),
        Some("providers")
    );
    assert_eq!(
        resolve_section("channels.telegram.groups"),
        Some("channels")
    );
}

#[test]
fn the_other_channel_children_resolve_too() {
    for child in ["discord", "slack", "whatsapp", "trello"] {
        assert_eq!(resolve_section(child), Some("channels"), "missed {child}");
    }
}

#[test]
fn case_and_whitespace_do_not_defeat_it() {
    assert_eq!(resolve_section("  Providers.STT  "), Some("providers"));
    assert_eq!(resolve_section("TELEGRAM"), Some("channels"));
}

#[test]
fn a_leading_or_trailing_dot_is_tolerated() {
    assert_eq!(resolve_section(".providers.stt"), Some("providers"));
    assert_eq!(resolve_section("channels."), Some("channels"));
}

#[test]
fn an_unknown_section_still_fails() {
    // Resolution must not become a way to silently accept nonsense; the
    // caller needs the error.
    for bad in ["nonsense", "agentt", "provider", "", "   ", "."] {
        assert_eq!(resolve_section(bad), None, "wrongly accepted {bad:?}");
    }
}
