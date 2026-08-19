//! Telegram-native userbot login flow tests.
//!
//! The network round-trip is covered by the live spike; these tests pin the
//! deterministic boundary: command recognition, credential validation and the
//! only disk write the conversational flow performs.

use crate::channels::telegram::userbot::setup::{
    CollectedCredentials, CredentialDraft, parse_login_command, persist_credentials,
};
use crate::config::profile::with_home_override;

#[test]
fn recognizes_hyphen_underscore_and_bot_suffix_forms() {
    assert_eq!(parse_login_command("/userbot-login"), Some(""));
    assert_eq!(
        parse_login_command("/userbot_login 123 hash phone"),
        Some("123 hash phone")
    );
    assert_eq!(
        parse_login_command("/userbot_login@opencrabsbot 123 hash phone"),
        Some("123 hash phone")
    );
    assert_eq!(parse_login_command("/userbot-login-now"), None);
    assert_eq!(parse_login_command("hello"), None);
}

#[test]
fn positional_args_name_the_exact_invalid_field() {
    let mut draft = CredentialDraft::default();
    let err = draft
        .ingest_positional("25625345 not-a-hash +254769000111")
        .expect_err("bad api_hash must fail");
    assert!(err.contains("api_hash"), "wrong error: {err}");
    assert!(err.contains("32 hexadecimal"), "wrong error: {err}");

    let err = draft
        .ingest_positional("25625345 0123456789abcdef0123456789abcdef 254769000111")
        .expect_err("phone without + must fail");
    assert!(err.contains("phone"), "wrong error: {err}");
}

#[test]
fn interactive_values_can_arrive_partially_and_in_any_order() {
    let mut draft = CredentialDraft::default();
    draft
        .ingest_unordered("+254769000111")
        .expect("phone is accepted");
    assert_eq!(draft.missing_names(), vec!["api_id", "api_hash"]);

    draft
        .ingest_unordered("0123456789abcdef0123456789abcdef 25625345")
        .expect("remaining values are accepted in either order");
    let creds = draft.complete().expect("draft is now complete");
    assert_eq!(creds.api_id, 25_625_345);
    assert_eq!(creds.phone, "+254769000111");
    assert_eq!(creds.api_hash.len(), 32);
}

#[test]
fn persistence_is_atomic_typed_and_owner_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join(".opencrabs");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("keys.toml"),
        "[channels.telegram]\ntoken = \"existing-bot-token\"\n",
    )
    .expect("fixture");

    with_home_override(home.clone(), || {
        persist_credentials(&CollectedCredentials {
            api_id: 25_625_345,
            api_hash: "0123456789abcdef0123456789abcdef".into(),
            phone: "+254769000111".into(),
        })
        .expect("persist credentials");
    });

    let raw = std::fs::read_to_string(home.join("keys.toml")).expect("read keys");
    let value: toml::Value = toml::from_str(&raw).expect("typed TOML");
    let telegram = &value["channels"]["telegram"];
    assert_eq!(telegram["token"].as_str(), Some("existing-bot-token"));
    assert_eq!(telegram["userbot"]["api_id"].as_integer(), Some(25_625_345));
    assert_eq!(telegram["userbot"]["phone"].as_str(), Some("+254769000111"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join("keys.toml"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
