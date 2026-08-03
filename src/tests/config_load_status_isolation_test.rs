//! Regression (#912): the outcome of a config load belongs to the caller that
//! asked for it, not to the process.
//!
//! `Config::load()` used to record "I recovered from last-known-good" in a
//! process-wide flag that `was_recovered()` consumed with `swap(false)`. Any
//! thread that called the accessor took the signal away from everyone else, so
//! a test asserting on its own recovery failed whenever another thread read the
//! flag first. That is order-dependent by construction: it passed alone and
//! failed in the suite.
//!
//! These tests pin the replacement contract — a per-call `ConfigLoadStatus`
//! that concurrent loaders cannot take from each other.

use crate::config::profile::with_home_override;
use crate::config::{Config, opencrabs_home, save_last_good_config};

const GOOD_CONFIG: &str = r#"
[agent]
approval_policy = "auto-always"
"#;

/// A syntax error the mechanical repairer cannot safely fix, so the load has
/// to fall back to the last-known-good snapshot.
const UNFIXABLE_BROKEN: &str = r#"
[agent]
approval_policy = "auto-always" / oops
"#;

/// A tempdir laid out as an `.opencrabs` home, holding a valid config.
fn temp_home() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    std::fs::write(opencrabs.join("config.toml"), GOOD_CONFIG).expect("write config");
    std::fs::write(opencrabs.join("keys.toml"), b"").expect("write keys");
    (dir, opencrabs)
}

#[test]
fn concurrent_loaders_each_see_their_own_recovery() {
    let (_temp, home) = temp_home();

    with_home_override(home.clone(), || {
        // Snapshot the valid config, then break config.toml unfixably.
        save_last_good_config();
        std::fs::write(opencrabs_home().join("config.toml"), UNFIXABLE_BROKEN)
            .expect("overwrite with unfixable config");
    });

    // Every loader recovers, so every loader must be TOLD it recovered. Under
    // the consume-once flag exactly one of these would have seen `true` and the
    // rest would have silently observed a clean load of a broken file.
    //
    // The override is task-local, so each thread scopes it for itself.
    const LOADERS: usize = 8;
    let statuses: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..LOADERS)
            .map(|_| {
                let home = home.clone();
                s.spawn(move || {
                    with_home_override(home, || {
                        let (config, status) =
                            Config::load_with_status().expect("recovery must produce a config");
                        (config.agent.approval_policy.clone(), status)
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("loader thread panicked"))
            .collect()
    });

    for (policy, status) in &statuses {
        assert!(
            status.recovered,
            "each concurrent load must report its OWN recovery, not race for one flag"
        );
        assert!(
            status.recovery_reason.is_some(),
            "recovery must carry the parse error that caused it"
        );
        assert_eq!(
            policy, "auto-always",
            "recovery must preserve the snapshotted policy"
        );
    }
}

#[test]
fn a_clean_load_reports_nothing_to_report() {
    let (_temp, home) = temp_home();
    with_home_override(home, || {
        let (config, status) = Config::load_with_status().expect("valid config must load");
        assert_eq!(config.agent.approval_policy, "auto-always");
        assert!(!status.recovered, "a config that parses did not recover");
        assert!(!status.autofixed, "a config that parses needed no repair");
        assert!(status.recovery_reason.is_none());
    });
}

#[test]
fn loading_config_does_not_rewrite_it() {
    // Schema migration used to run inside `load_inner`, so reading config wrote
    // to disk. Two tests pointed at the same home then clobbered each other's
    // fixtures, and a background reload could rewrite a file nobody asked it to
    // touch. Loading is a pure read now; migration is an explicit startup step.
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let path = opencrabs_home().join("config.toml");
        let before = std::fs::read_to_string(&path).expect("fixture readable");

        Config::load().expect("valid config must load");
        Config::load().expect("valid config must load twice");

        let after = std::fs::read_to_string(&path).expect("config still readable");
        assert_eq!(
            before, after,
            "Config::load() must not modify config.toml — reads do not write"
        );
    });
}

#[test]
fn first_load_status_survives_being_read() {
    // The startup notification reads this; a second reader must not clear it.
    // Consuming reads are exactly what made the old flags order-dependent.
    let first = Config::first_load_status();
    let second = Config::first_load_status();
    assert_eq!(
        first, second,
        "first_load_status must be non-destructive — two readers see the same outcome"
    );
}
