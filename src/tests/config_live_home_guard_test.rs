//! Test builds must never write the live default home (#1399).

use std::path::Path;

use crate::config::live_home_guard::{refusal_for, refuse_live_home_write};

#[test]
fn a_file_directly_in_the_live_home_is_refused() {
    let home = Path::new("/Users/someone/.opencrabs");
    let reason = refusal_for(&home.join("config.toml"), home).expect("refused");
    assert!(reason.contains("config.toml"), "{reason}");
    assert!(reason.contains("with_home_override"), "{reason}");
    assert!(refusal_for(&home.join("keys.toml"), home).is_some());
}

#[test]
fn a_profile_scoped_or_temp_home_passes() {
    let home = Path::new("/Users/someone/.opencrabs");
    assert!(refusal_for(&home.join("profiles/test-x/config.toml"), home).is_none());
    assert!(refusal_for(Path::new("/tmp/abc/.opencrabs/config.toml"), home).is_none());
}

#[test]
fn the_guard_bites_in_a_test_build() {
    // This binary IS a test build, so the real base dir is off limits.
    let live = crate::config::profile::base_opencrabs_dir().join("config.toml");
    let err = refuse_live_home_write(&live).expect_err("must refuse");
    assert!(err.to_string().contains("test build"), "{err}");
}

#[test]
fn the_guard_lets_a_temp_home_through() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join(".opencrabs").join("config.toml");
    refuse_live_home_write(&target).expect("temp home is fine");
}

#[test]
fn atomic_write_into_the_live_home_is_refused_without_touching_it() {
    let live = crate::config::profile::base_opencrabs_dir().join("config.toml");
    let before = std::fs::metadata(&live).ok().map(|m| m.modified().ok());
    let err = crate::config::types::io::atomic_write(&live, "poison = true\n")
        .expect_err("the live config must not be rewritten by a test");
    assert!(err.to_string().contains("refusing to write"), "{err}");
    let after = std::fs::metadata(&live).ok().map(|m| m.modified().ok());
    assert_eq!(before, after, "the live file must be untouched");
}
