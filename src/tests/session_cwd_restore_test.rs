//! Restoring a session's persisted working directory (`/cd` in a channel).

use std::path::PathBuf;

use crate::brain::agent::service::session_cwd::restorable_cwd;

#[test]
fn unset_or_blank_persisted_value_restores_nothing() {
    assert_eq!(restorable_cwd(None), None);
    assert_eq!(restorable_cwd(Some("")), None);
    assert_eq!(restorable_cwd(Some("   ")), None);
}

#[test]
fn stale_path_restores_nothing() {
    assert_eq!(
        restorable_cwd(Some("/nonexistent/repo/moved/away")),
        None,
        "a directory that no longer exists must not be restored"
    );
}

#[test]
fn existing_directory_is_restored() {
    let dir = std::env::temp_dir();
    let restored = restorable_cwd(Some(dir.to_string_lossy().as_ref()));
    assert_eq!(
        restored,
        Some(dir),
        "an existing directory is restored as written, not canonicalized"
    );
}

#[test]
fn collapsed_home_path_is_expanded() {
    let home = dirs::home_dir().expect("home dir");
    let restored = restorable_cwd(Some("~")).expect("home must restore");
    assert_eq!(restored, home);
    assert_ne!(
        restored,
        PathBuf::from("~"),
        "the tilde must be expanded, not passed through as a literal directory name"
    );
}
