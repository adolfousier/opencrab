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
        "logging",
        "debug",
        "database",
        "provider_registry",
        // The #1385 drift: all eight of these were refused by the write gate
        // despite being real tables serde loads fine.
        "daemon",
        "a2a",
        "image",
        "cron",
        "memory",
        "brain",
        "browser",
        "doctor",
    ] {
        assert!(
            validate_write_path(section).is_ok(),
            "#1199: rejected a real section: {section}"
        );
    }
}

#[test]
fn voice_is_not_a_writable_section() {
    // #1385: `voice` used to sit in CONFIG_SECTIONS, so writes passed the
    // gate and created a `[voice.*]` table serde silently discards. It is a
    // derived read-only view; the rejection must say where the keys live.
    let err = validate_write_path("voice").expect_err("must reject voice writes");
    assert!(
        err.contains("providers.stt") && err.contains("providers.tts"),
        "must name the real tables, got: {err}"
    );
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

// ── pre-write convergence: legacy provider spelling must not deny writes ──

use crate::config::profile::with_home_override;

fn in_temp_home(f: impl FnOnce()) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    with_home_override(opencrabs, f);
}

#[test]
fn test_write_to_renamed_provider_section_succeeds_on_a_legacy_file() {
    // The zai rename shipped with config.toml.example still spelling the
    // section [providers.zhipu]. A wizard apply then wrote providers.zai.*
    // next to it, serde read the alias as a duplicate field, and the write
    // guard denied it — every onboarding ProviderAuth apply failed with
    // "providers.zai.enabled could not be saved". The writer must converge
    // the legacy spelling before inserting, exactly like the load path.
    in_temp_home(|| {
        let home = crate::config::opencrabs_home();
        std::fs::create_dir_all(&home).expect("home dir");
        let cfg = home.join("config.toml");
        std::fs::write(
            &cfg,
            "# my notes\n[providers.zhipu]\nenabled = false\ndefault_model = \"glm-5.1\"\n",
        )
        .expect("seed legacy config");

        crate::config::Config::write_key("providers.zai", "enabled", "true")
            .expect("write must converge the legacy section, not be denied");

        let after = std::fs::read_to_string(&cfg).unwrap();
        assert!(
            after.contains("[providers.zai]"),
            "canonical section present"
        );
        assert!(!after.contains("zhipu"), "legacy spelling converged away");
        assert!(after.contains("true"), "the written value landed");
        assert!(after.contains("# my notes"), "comments survive");
        // And the file still loads.
        crate::config::Config::load().expect("converged config parses");
    });
}
