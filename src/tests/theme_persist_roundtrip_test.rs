//! A theme chosen with `/theme set` or the picker must survive a restart.
//!
//! Both write sites handed `write_key_string` a value they had already wrapped
//! in quotes, and `write_key_string` builds the TOML string itself, so the
//! quote characters landed inside the value: `theme = '"dracula"'`. The live
//! apply worked and the boot lookup is exact, so the theme came back as the
//! default with no error and no log line, reading as if it were randomly
//! forgotten rather than never stored (#1428).
//!
//! Config-writing tests run under a home override; the live home is refused in
//! test builds (#1399).

use crate::config::profile::with_home_override;
use crate::config::{Config, opencrabs_home};
use crate::tui::render::theme::configured_name;

fn stored_theme() -> String {
    let text = std::fs::read_to_string(opencrabs_home().join("config.toml")).expect("config.toml");
    let doc: toml::Value = toml::from_str(&text).expect("config must parse");
    doc.get("tui")
        .and_then(|t| t.get("theme"))
        .and_then(|v| v.as_str())
        .expect("tui.theme must be a string")
        .to_string()
}

#[test]
fn a_persisted_theme_name_carries_no_quote_characters() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        // Exactly what the write sites now pass: the bare name.
        Config::write_key_string("tui", "theme", "dracula").expect("write");

        assert_eq!(
            stored_theme(),
            "dracula",
            "the stored value must be the name itself, or the boot lookup can never match it"
        );
    });
}

#[test]
fn resetting_clears_the_key_rather_than_storing_two_quotes() {
    let home = tempfile::tempdir().expect("tempdir");
    with_home_override(home.path().to_path_buf(), || {
        Config::write_key_string("tui", "theme", "dracula").expect("write");
        Config::write_key_string("tui", "theme", "").expect("reset");

        let stored = stored_theme();
        assert!(
            stored.is_empty(),
            "reset must leave an empty value so boot falls through to the default, \
             not a two-character string of quotes: {stored:?}"
        );
    });
}

#[test]
fn a_config_poisoned_before_the_fix_still_resolves() {
    // The exact shape #1428 reports on disk.
    assert_eq!(configured_name("\"dracula\""), "dracula");
    // And the healthy shape is untouched.
    assert_eq!(configured_name("dracula"), "dracula");
    assert_eq!(configured_name(""), "");
}

#[test]
fn only_a_matching_quote_pair_is_stripped() {
    // A name that merely contains a quote is a lookup miss, not this bug, and
    // must not be silently rewritten into something else.
    assert_eq!(configured_name("\"dracula"), "\"dracula");
    assert_eq!(configured_name("dracula\""), "dracula\"");
}
