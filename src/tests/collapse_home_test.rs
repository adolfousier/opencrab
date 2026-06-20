//! Tests for `tools::error::collapse_home` and its pairing with
//! `expand_tilde`. The helper renders absolute paths under `$HOME` as
//! `~/...` so they don't leak the local username into the system
//! prompt and so the prompt-cache key stays stable across machines.

use crate::brain::tools::error::{collapse_home, expand_tilde};
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    dirs::home_dir().expect("home dir required for these tests")
}

#[test]
fn collapses_path_directly_under_home() {
    let p = home().join("srv/dart/myapp/lib/main.dart");
    assert_eq!(collapse_home(&p), "~/srv/dart/myapp/lib/main.dart");
}

#[test]
fn collapses_home_root_to_just_tilde() {
    let p = home();
    assert_eq!(collapse_home(&p), "~");
}

#[test]
fn collapses_dotfile_under_home() {
    let p = home().join(".opencrabs/logs/opencrabs.2026-04-26");
    assert_eq!(collapse_home(&p), "~/.opencrabs/logs/opencrabs.2026-04-26");
}

#[test]
fn passes_through_absolute_path_outside_home() {
    // System paths and other users' homes must not be munged.
    assert_eq!(collapse_home(Path::new("/etc/hosts")), "/etc/hosts");
    assert_eq!(collapse_home(Path::new("/usr/bin/git")), "/usr/bin/git");
}

#[test]
fn passes_through_path_with_home_substring_but_not_prefix() {
    // A path that just CONTAINS the home string mid-string must not be
    // collapsed — strip_prefix is a path-component check, not a string
    // contains, so this should never trigger, but pin it anyway.
    let weird = PathBuf::from(format!(
        "/var/{}/projects",
        home().display().to_string().trim_start_matches('/')
    ));
    let rendered = collapse_home(&weird);
    assert!(
        !rendered.starts_with("~/"),
        "must not collapse a path that has $HOME in the middle: {rendered}"
    );
}

#[test]
fn passes_through_relative_path_unchanged() {
    // Relative paths can't be inside $HOME by definition (no anchor).
    assert_eq!(collapse_home(Path::new("src/main.rs")), "src/main.rs",);
}

#[test]
fn round_trips_with_expand_tilde() {
    // collapse_home and expand_tilde are inverses: expanding what we
    // collapsed should give back the original absolute PathBuf.
    for relative in ["srv/dart/myapp", ".opencrabs", ".config/nvim/init.lua"] {
        let original = home().join(relative);
        let collapsed = collapse_home(&original);
        let re_expanded = expand_tilde(&collapsed);
        assert_eq!(
            re_expanded, original,
            "collapse → expand round trip failed for {}",
            relative
        );
    }
}

#[test]
fn collapsed_string_is_strictly_shorter_for_home_paths() {
    // The whole point of this helper, beyond privacy, is fewer
    // tokens. Pin that the home-prefixed render is at least 1 char
    // shorter than the absolute one (it's typically much more).
    let p = home().join("srv/dart/myapp/lib/data/services/app_service.dart");
    let absolute = p.display().to_string();
    let collapsed = collapse_home(&p);
    assert!(
        collapsed.len() < absolute.len(),
        "collapsed '{}' should be shorter than absolute '{}'",
        collapsed,
        absolute,
    );
    assert!(
        collapsed.starts_with("~/"),
        "collapsed string must use the tilde anchor: {collapsed}"
    );
}
