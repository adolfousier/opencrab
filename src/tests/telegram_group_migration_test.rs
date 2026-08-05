//! Regression (#946): a group that migrates to a supergroup is followed.
//!
//! A basic group that upgrades gets a new chat id, and every later call against
//! the old one fails. Nothing read `migrate_to_chat_id`, so the config kept the
//! dead key and menu registration re-hammered it once per allowed user per
//! refresh — 364 warnings in one day for a single group — while that group
//! never got its menus.
//!
//! Telegram returns the replacement id inside the failure text, so recovery
//! needs no extra call.

use crate::channels::telegram::menu_scope::{means_not_a_member, migrated_to};
use crate::config::Config;
use crate::config::profile::with_home_override;

/// The error verbatim, as teloxide renders it.
const MIGRATED: &str = "The group has been migrated to a supergroup with ID #-1004441241066";

#[test]
fn the_new_id_is_parsed_out_of_the_failure() {
    assert_eq!(migrated_to(MIGRATED), Some(-1004441241066));
}

#[test]
fn an_unrelated_failure_reports_no_migration() {
    // Must not fire on ordinary errors, or a transient fault would move a live
    // group's config onto a bogus key.
    assert_eq!(migrated_to("Bad Request: chat not found"), None);
    assert_eq!(migrated_to("USER_ID_INVALID"), None);
    assert_eq!(migrated_to(""), None);
}

#[test]
fn a_migration_is_not_mistaken_for_a_missing_member() {
    // The two are handled differently: one is quiet and expected, the other
    // means the group id itself is dead.
    assert!(!means_not_a_member(MIGRATED));
    assert!(means_not_a_member("USER_ID_INVALID"));
}

/// A config home with one group section carrying a setting worth preserving.
fn home_with_group(id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    std::fs::write(
        opencrabs.join("config.toml"),
        format!("[channels.telegram.groups.{id}]\nopen = true\nallowed_users = [\"42\"]\n"),
    )
    .expect("write config");
    (dir, opencrabs)
}

#[test]
fn the_section_moves_to_the_new_id_with_its_settings() {
    let (_temp, home) = home_with_group("-5324478558");
    with_home_override(home.clone(), || {
        let moved = Config::rename_section(
            "channels.telegram.groups.-5324478558",
            "channels.telegram.groups.-1004441241066",
        )
        .expect("rename");
        assert!(moved, "the section existed and must report as moved");

        let raw = std::fs::read_to_string(home.join("config.toml")).expect("read back");
        assert!(
            raw.contains("[channels.telegram.groups.-1004441241066]"),
            "new key must exist:\n{raw}"
        );
        assert!(
            !raw.contains("[channels.telegram.groups.-5324478558]"),
            "dead key must be gone, or it keeps warning:\n{raw}"
        );
        assert!(
            raw.contains("allowed_users") && raw.contains("\"42\""),
            "the group's settings are the user's and must survive the move:\n{raw}"
        );
    });
}

#[test]
fn an_existing_destination_is_never_overwritten() {
    // If the supergroup already registered itself, that entry is the live one.
    // Clobbering it with the stale copy would undo whatever was configured since.
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&home).expect("create");
    std::fs::write(
        home.join("config.toml"),
        "[channels.telegram.groups.-5324478558]\nopen = false\n\n\
         [channels.telegram.groups.-1004441241066]\nopen = true\n",
    )
    .expect("write");

    with_home_override(home.clone(), || {
        let moved = Config::rename_section(
            "channels.telegram.groups.-5324478558",
            "channels.telegram.groups.-1004441241066",
        )
        .expect("rename");
        assert!(
            !moved,
            "an occupied destination must be reported, not filled"
        );

        let raw = std::fs::read_to_string(home.join("config.toml")).expect("read back");
        let live = raw
            .split("[channels.telegram.groups.-1004441241066]")
            .nth(1)
            .expect("destination section present");
        assert!(
            live.contains("open = true"),
            "the live entry must be untouched:\n{raw}"
        );
    });
}

#[test]
fn renaming_a_section_that_does_not_exist_is_a_no_op() {
    let (_temp, home) = home_with_group("-5324478558");
    with_home_override(home, || {
        let moved = Config::rename_section(
            "channels.telegram.groups.-9999999999",
            "channels.telegram.groups.-1004441241066",
        )
        .expect("rename must not error");
        assert!(!moved);
    });
}
