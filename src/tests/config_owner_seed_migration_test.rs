//! Config migration seeds bot_owner from the first allow-list entry (#243).
//!
//! Owner identity was implicit (first allow-list entry). The migration makes it
//! explicit and stable by writing bot_owner into config.toml on load, for both
//! fresh setups (next load after onboarding) and existing configs (update).
//!
//! Two conditions were added in #1440, and the cases below carry them: the
//! channel must be enabled, since a channel nobody turned on has no operator to
//! pin yet, and the value must not be one of the template'"'"'s own documentation
//! placeholders, since seeding one makes a stranger the owner and locks the
//! real operator out of the positional fallback.

use crate::config::Config;
use tempfile::TempDir;

fn write_cfg(contents: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn seeds_bot_owner_from_allowed_users() {
    let (_d, path) = write_cfg("[channels.telegram]\nenabled = true\nallowed_users = [123, 456]\n");
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let owner = doc["channels"]["telegram"]["bot_owner"].as_array().unwrap();
    assert_eq!(owner.len(), 1);
    assert_eq!(owner[0].as_integer(), Some(123), "owner is the first entry");
}

#[test]
fn seeds_whatsapp_from_allowed_phones() {
    // Not the template'"'"'s "+15551234567": that is a documentation placeholder
    // the migration now refuses (#1440).
    let (_d, path) =
        write_cfg("[channels.whatsapp]\nenabled = true\nallowed_phones = [\"+15559876543\"]\n");
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let owner = doc["channels"]["whatsapp"]["bot_owner"].as_array().unwrap();
    assert_eq!(owner[0].as_str(), Some("+15559876543"));
}

#[test]
fn does_not_override_explicit_bot_owner() {
    let (_d, path) =
        write_cfg("[channels.telegram]\nallowed_users = [111, 222]\nbot_owner = [222]\n");
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let owner = doc["channels"]["telegram"]["bot_owner"].as_array().unwrap();
    assert_eq!(owner.len(), 1);
    assert_eq!(
        owner[0].as_integer(),
        Some(222),
        "explicit owner is preserved"
    );
}

#[test]
fn no_seed_when_allow_list_is_empty() {
    let (_d, path) = write_cfg("[channels.telegram]\nenabled = true\nallowed_users = []\n");
    Config::migrate_if_needed(&path);
    let content = std::fs::read_to_string(&path).unwrap();
    let doc: toml::Value = toml::from_str(&content).unwrap();
    assert!(
        doc["channels"]["telegram"].get("bot_owner").is_none(),
        "no owner to seed when the allow list is empty"
    );
}

#[test]
fn preserves_comments_in_untouched_sections() {
    // Format-preserving write must keep user comments.
    let (_d, path) = write_cfg(
        "# my config\n[channels.telegram]\nenabled = true\nallowed_users = [123] # the owner\n",
    );
    Config::migrate_if_needed(&path);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("# my config"), "top comment preserved");
    assert!(content.contains("bot_owner"), "owner seeded");
}

#[test]
fn no_seed_for_a_channel_nobody_enabled() {
    // A disabled channel has no operator to pin yet, and the template ships
    // every channel disabled with an example id in its allow-list, so seeding
    // here records a placeholder as the owner of a channel never configured
    // (#1440).
    let (_d, path) = write_cfg("[channels.discord]\nenabled = false\nallowed_users = [777]\n");
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        doc["channels"]["discord"].get("bot_owner").is_none(),
        "a disabled channel must not have an owner pinned for it"
    );
}

#[test]
fn no_seed_from_a_documentation_placeholder() {
    // The exact shape a fresh install has: the template'"'"'s own example id, on a
    // channel the operator has just switched on. Seeding it would make the
    // placeholder the owner set, and an explicit bot_owner IS the owner set, so
    // the real operator could never become owner by talking first.
    let (_d, path) =
        write_cfg("[channels.telegram]\nenabled = true\nallowed_users = [123456789]\n");
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        doc["channels"]["telegram"].get("bot_owner").is_none(),
        "a template placeholder must never be promoted to owner"
    );
}
