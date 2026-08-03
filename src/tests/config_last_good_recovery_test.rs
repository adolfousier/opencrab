//! Regression: a broken `config.toml` must NOT poison the last-known-good
//! snapshot, and `Config::load()` must recover from it.
//!
//! The bug: the config watcher called `Config::load()` (which already recovers
//! from last-good, so it returns Ok even when config.toml is broken) and then
//! `save_last_good_config()` did a RAW `fs::copy` of config.toml over the good
//! snapshot. A single malformed edit (an unterminated `models = [...]` array)
//! therefore corrupted BOTH config.toml and its snapshot at once. The next
//! `Config::load()` had no valid last-good to fall back to, returned an error,
//! and the approval check defaulted to "ask" — flipping auto-always (yolo)
//! users into tool-approval prompts after months without one.
//!
//! Fix: `save_last_good_config()` refuses to snapshot a config that does not
//! parse, and the watcher skips the snapshot when the load was a recovery.
//!
//! These tests scope the config home with `with_home_override` rather than
//! pointing `$HOME` at a tempdir. `$HOME` is process-global: any test loading
//! config in parallel resolved into this tempdir and rewrote the deliberately
//! broken config.toml, which is what made the suite order-dependent (#912).

use crate::config::profile::with_home_override;
use crate::config::{Config, load_last_good_config, opencrabs_home, save_last_good_config};

const GOOD_CONFIG: &str = r#"
[agent]
approval_policy = "auto-always"
"#;

// Mirrors the real failure: an unterminated array breaks the whole TOML file.
// This one IS mechanically repairable (just a missing `]`).
const FIXABLE_BROKEN: &str = r#"
[agent]
approval_policy = "auto-always"

[providers.custom.broken]
models = ["a", "b", "c"
"#;

// A non-delimiter syntax error the repairer can't safely fix — must fall back
// to last-known-good.
const UNFIXABLE_BROKEN: &str = r#"
[agent]
approval_policy = "auto-always" / oops
"#;

/// A tempdir laid out as an `.opencrabs` home, plus the config it starts with.
/// Returns the tempdir so the caller keeps it alive for the whole test.
fn temp_home_with(config_toml: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    std::fs::write(opencrabs.join("config.toml"), config_toml).expect("write config");
    std::fs::write(opencrabs.join("keys.toml"), b"").expect("write keys");
    (dir, opencrabs)
}

#[test]
fn broken_config_does_not_poison_last_good_snapshot() {
    let (_temp, home) = temp_home_with(GOOD_CONFIG);
    with_home_override(home, || {
        // Snapshot the good config — last-good now holds auto-always.
        save_last_good_config();
        let good = load_last_good_config().expect("snapshot of a valid config must load");
        assert_eq!(good.agent.approval_policy, "auto-always");

        // The user saves a malformed config.toml. Snapshotting again MUST refuse —
        // the broken file must not overwrite the good snapshot (the regression that
        // poisoned recovery and flipped yolo mode into approval prompts).
        let config_path = opencrabs_home().join("config.toml");
        std::fs::write(&config_path, UNFIXABLE_BROKEN).expect("overwrite with broken config");
        save_last_good_config();
        let still_good =
            load_last_good_config().expect("snapshot must still be the prior VALID config");
        assert_eq!(
            still_good.agent.approval_policy, "auto-always",
            "a broken config.toml must NOT clobber the last-good snapshot"
        );
    });
}

#[test]
fn fixable_broken_config_is_auto_repaired_in_place() {
    let (_temp, home) = temp_home_with(FIXABLE_BROKEN);
    with_home_override(home, || {
        let config_path = opencrabs_home().join("config.toml");

        // Precondition: the file does not parse as-is.
        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(toml::from_str::<toml::Value>(&raw).is_err());

        // load() repairs the missing `]`, saves it back, and loads cleanly —
        // preserving auto-always so yolo mode survives a typo.
        let (cfg, status) = Config::load_with_status().expect("load must auto-repair and succeed");
        assert_eq!(cfg.agent.approval_policy, "auto-always");
        assert!(
            status.autofixed,
            "load should report it auto-repaired the file"
        );

        // The fix is persisted: config.toml now parses, and the broken original was
        // backed up.
        let healed = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            toml::from_str::<toml::Value>(&healed).is_ok(),
            "config.toml must be valid on disk after auto-repair"
        );
        assert!(
            config_path.with_extension("toml.autofix.bak").exists(),
            "the broken original must be backed up"
        );
    });
}

#[test]
fn unfixable_broken_config_recovers_from_last_good() {
    let (_temp, home) = temp_home_with(GOOD_CONFIG);
    with_home_override(home, || {
        // Establish a good snapshot, then break config.toml unfixably.
        save_last_good_config();
        let config_path = opencrabs_home().join("config.toml");
        std::fs::write(&config_path, UNFIXABLE_BROKEN).expect("overwrite with unfixable config");

        // load() can't repair it, so it falls back to last-known-good — preserving
        // auto-always rather than defaulting to an approval prompt.
        let (recovered, status) =
            Config::load_with_status().expect("load must recover from last-good, not error out");
        assert_eq!(
            recovered.agent.approval_policy, "auto-always",
            "recovery must preserve the auto-always policy so yolo mode survives a broken edit"
        );
        assert!(
            status.recovered,
            "the load should report it fell back to last-known-good"
        );
        assert!(
            status.recovery_reason.is_some(),
            "recovery must carry the parse error that caused it (#909)"
        );
    });
}
