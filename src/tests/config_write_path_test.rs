//! A config write may only address a real section (#1199).
//!
//! `write_item` creates any table its dotted path names, so a wrong section
//! wrote successfully into an orphan table that serde discards on load:
//! tooling reported success, the setting never applied, and reads kept
//! honestly returning the old value — which reads exactly like a stale cache.

use crate::config::sections::{resolve_section, validate_write_path};

#[test]
fn test_the_three_reported_paths_are_rejected_with_the_right_suggestion() {
    // Verbatim from the report: three writes that all returned Ok and left
    // 11 lines of dead TOML behind.
    let err = validate_write_path("opencode").expect_err("must reject");
    assert!(err.contains("opencode"), "{err}");

    let err = validate_write_path("custom.inferhub").expect_err("must reject");
    assert!(
        err.contains("providers.custom.inferhub"),
        "#1199: should name the intended path, got: {err}"
    );

    let err = validate_write_path("fallback").expect_err("must reject");
    assert!(
        err.contains("providers.fallback"),
        "#1199: should name the intended path, got: {err}"
    );
}

#[test]
fn test_the_real_paths_are_accepted() {
    for section in [
        "providers.opencode",
        "providers.custom.inferhub",
        "providers.fallback",
        "agent",
        "channels.telegram",
        "channels.telegram.groups",
        "providers.stt",
        "voice",
        "logging",
        "debug",
        "database",
        "provider_registry",
    ] {
        assert!(
            validate_write_path(section).is_ok(),
            "#1199: rejected a real section: {section}"
        );
    }
}

#[test]
fn test_writes_are_stricter_than_reads_on_purpose() {
    // `resolve_section` exists to ACCEPT shorthand, because the reader only
    // renders the top level. That is the right call for a read and fatal for
    // a write: `custom.inferhub` resolves to `providers` for a reader, while
    // writing it creates a top-level `[custom.inferhub]` serde ignores. So
    // the write rule is positional, not by-name.
    for shorthand in ["custom.inferhub", "fallback", "telegram", "stt"] {
        assert!(
            resolve_section(shorthand).is_some(),
            "reader still accepts the shorthand: {shorthand}"
        );
        assert!(
            validate_write_path(shorthand).is_err(),
            "#1199: writer must NOT accept the shorthand: {shorthand}"
        );
    }
}

#[test]
fn test_empty_and_dot_padded_paths() {
    assert!(validate_write_path("").is_err());
    assert!(validate_write_path("   ").is_err());
    assert!(validate_write_path("...").is_err());
    // Padding is tolerated on a real path, matching the reader.
    assert!(validate_write_path("  .providers.stt.  ").is_ok());
}

#[test]
fn test_case_is_ignored_like_the_reader_does() {
    assert!(validate_write_path("Providers.OpenCode").is_ok());
    assert!(validate_write_path("CHANNELS.telegram").is_ok());
}
