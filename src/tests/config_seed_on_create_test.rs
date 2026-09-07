//! Whatever first needs `config.toml` to exist must seed it from the shipped
//! example, not create it around its own write.
//!
//! `config.toml.example` carries a section per provider, disabled, documented
//! inline, and it is how a provider setting gets changed by hand. It reached
//! disk from a single onboarding step, guarded on the file being absent, while
//! two other paths created the file without it: `write_key` built its document
//! from nothing, and a startup auto-generate serialised the in-memory config
//! over the path. Whichever ran first won permanently, since the seeding guard
//! only fires while the file is missing, and the loser was a user holding a
//! config with no provider sections to edit (#1437).
//!
//! These tests scope the config home with `with_home_override` rather than
//! pointing `$HOME` at a tempdir, which is process-global and would let a
//! parallel test resolve into this one's directory (#912).

use crate::config::profile::with_home_override;
use crate::config::{Config, opencrabs_home};

/// Sections a user edits by hand, spread across the example so a truncated
/// seed cannot pass by accident.
const EXPECTED_SECTIONS: &[&str] = &[
    "[providers.xiaomi]",
    "[providers.anthropic]",
    "[providers.claude_cli]",
    "[providers.gemini]",
];

fn read_config() -> String {
    std::fs::read_to_string(opencrabs_home().join("config.toml")).expect("config.toml must exist")
}

#[test]
fn writing_one_key_seeds_the_whole_example() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        // A single unrelated key, of the kind any slash command writes.
        Config::write_key("agent", "approval_policy", "\"auto-always\"").expect("write");

        let written = read_config();
        for section in EXPECTED_SECTIONS {
            assert!(
                written.contains(section),
                "a config created by one key write is missing {section}, \
                 leaving nothing to hand-edit"
            );
        }
        assert!(
            written.contains("auto-always"),
            "the key that triggered the seed must still be written:\n{written}"
        );
    });
}

#[test]
fn seeding_never_overwrites_an_existing_config() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        let path = home.path().join("config.toml");
        std::fs::write(&path, "[agent]\napproval_policy = \"ask\"\n").expect("write");

        crate::config::seed::ensure_config_seeded();

        let written = std::fs::read_to_string(&path).expect("read");
        assert!(
            !written.contains("[providers.anthropic]"),
            "seeding supplies a starting point, it must not restore one over a \
             config the user already has:\n{written}"
        );
    });
}

#[test]
fn writing_one_credential_seeds_the_keys_example() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        Config::write_keys_key("providers.anthropic", "api_key", "test-key").expect("write");

        let written =
            std::fs::read_to_string(home.path().join("keys.toml")).expect("keys.toml must exist");
        assert!(
            written.contains("[providers.gemini]"),
            "keys.toml has the same hole as config.toml: a file created around \
             one credential carries a slot for nothing else:\n{written}"
        );
        assert!(written.contains("test-key"), "the written key must survive");
    });
}

#[test]
fn auto_generate_enables_only_providers_that_have_a_key() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        let mut config = Config::default();
        config.providers.anthropic = Some(crate::config::ProviderConfig {
            api_key: Some("test-key".to_string()),
            ..Default::default()
        });

        crate::config::seed::ensure_config_seeded();
        let enabled = crate::config::seed::enable_providers_with_keys(&config);

        assert_eq!(
            enabled,
            vec!["providers.anthropic".to_string()],
            "only the provider holding a key is enabled"
        );

        // The rest of the example survives the merge, which is the whole point
        // of writing key by key instead of serialising the struct over it.
        let written = read_config();
        for section in EXPECTED_SECTIONS {
            assert!(
                written.contains(section),
                "{section} was lost when the keyed provider was enabled"
            );
        }
    });
}
