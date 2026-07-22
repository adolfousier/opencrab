//! #712/#713/#714: the config guard refuses writes that would leave config.toml
//! or keys.toml unparseable. These lock the parse checks the write paths and the
//! last-good snapshot rely on.

use crate::config::guard::{ProtectedFile, deny_if_would_break, parses};
use std::path::Path;

#[test]
fn valid_config_parses() {
    // Config has serde defaults everywhere, so a minimal section is valid.
    assert!(parses(ProtectedFile::Config, "[agent]\n").is_ok());
    assert!(parses(ProtectedFile::Config, "").is_ok());
}

#[test]
fn broken_toml_is_rejected_for_config() {
    // Unterminated array — the exact class of hand-edit corruption.
    assert!(parses(ProtectedFile::Config, "allowed = [\n").is_err());
    // Bare dangling key.
    assert!(parses(ProtectedFile::Config, "key =\n").is_err());
}

#[test]
fn valid_keys_parse() {
    assert!(parses(ProtectedFile::Keys, "[providers]\n").is_ok());
    assert!(parses(ProtectedFile::Keys, "").is_ok());
}

#[test]
fn broken_toml_is_rejected_for_keys() {
    assert!(parses(ProtectedFile::Keys, "[providers\n").is_err());
    assert!(parses(ProtectedFile::Keys, "token = \"unterminated\n").is_err());
}

#[test]
fn non_protected_path_is_never_denied() {
    // A project's own config.toml (in a random dir) is not the profile config,
    // so even broken content passes the guard untouched.
    let some_project = Path::new("/tmp/some-project/config.toml");
    assert!(deny_if_would_break(some_project, "totally [ broken").is_ok());
}
