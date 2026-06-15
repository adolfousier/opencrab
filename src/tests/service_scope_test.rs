//! Tests for systemd unit-scope detection (`unit_scope_probe`): probe whether a
//! unit was installed at system or user scope, falling back to `running_as_root`
//! when neither unit file exists. Linux-only (systemd). Relocated here from an
//! inline module in `cli/commands.rs`; uses `tempfile` so the dirs are isolated
//! per test (the original shared one dir keyed by PID, which races under
//! cargo's parallel test runner).

use crate::cli::commands::{running_as_root, unit_scope_probe};
use std::path::Path;

/// Run `f` with a fresh, isolated `system` + `user` dir pair. The temp dir is
/// auto-removed when it drops.
fn with_dirs<F: FnOnce(&Path, &Path)>(f: F) {
    let base = tempfile::tempdir().expect("tempdir");
    let system_dir = base.path().join("system");
    let user_dir = base.path().join("user");
    std::fs::create_dir_all(&system_dir).unwrap();
    std::fs::create_dir_all(&user_dir).unwrap();
    f(&system_dir, &user_dir);
}

fn touch(dir: &Path, name: &str) {
    std::fs::File::create(dir.join(name)).unwrap();
}

#[test]
fn system_file_exists_returns_system_scope() {
    with_dirs(|sys, usr| {
        touch(sys, "opencrabs.service");
        assert!(unit_scope_probe("opencrabs", sys, Some(usr)));
    });
}

#[test]
fn only_user_file_exists_returns_user_scope() {
    with_dirs(|sys, usr| {
        touch(usr, "opencrabs.service");
        assert!(!unit_scope_probe("opencrabs", sys, Some(usr)));
    });
}

#[test]
fn both_exist_system_wins() {
    with_dirs(|sys, usr| {
        touch(sys, "opencrabs.service");
        touch(usr, "opencrabs.service");
        assert!(unit_scope_probe("opencrabs", sys, Some(usr)));
    });
}

#[test]
fn neither_exists_falls_back_to_running_as_root() {
    with_dirs(|sys, usr| {
        // No files created — must fall back to `running_as_root()`.
        let expected = running_as_root();
        assert_eq!(unit_scope_probe("opencrabs", sys, Some(usr)), expected);
    });
}

#[test]
fn no_user_dir_falls_back_to_running_as_root() {
    with_dirs(|sys, _usr| {
        // user_dir = None, no system file — falls back.
        let expected = running_as_root();
        assert_eq!(unit_scope_probe("opencrabs", sys, None), expected);
    });
}

#[test]
fn system_file_with_no_user_dir() {
    with_dirs(|sys, _usr| {
        touch(sys, "opencrabs.service");
        assert!(unit_scope_probe("opencrabs", sys, None));
    });
}

#[test]
fn profiled_service_name() {
    with_dirs(|sys, usr| {
        touch(usr, "opencrabs-ops.service");
        assert!(!unit_scope_probe("opencrabs-ops", sys, Some(usr)));
    });
}
