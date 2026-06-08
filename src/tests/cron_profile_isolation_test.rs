//! Tests for the profile-home resolution that lets a cron job run under the
//! profile it was created in, even when another profile's process picks it up
//! from a shared database (#182).
//!
//! The fix materializes a foreign profile's config + brain under a scoped home
//! override (`with_profile_home`) that `resolve_profile_home()` consults before
//! the process-global active profile. These tests cover the pure path mapping
//! and the scoped override (applies inside the closure, cleared after).

use crate::config::profile::{
    base_opencrabs_dir, home_for_profile, resolve_profile_home, with_profile_home,
};

#[test]
fn home_for_profile_maps_default_to_base() {
    let base = base_opencrabs_dir();
    // None and the literal "default" both resolve to the base dir, never a
    // profiles/ subdir — the base profile is unannotated on disk.
    assert_eq!(home_for_profile(None), base);
    assert_eq!(home_for_profile(Some("default")), base);
}

#[test]
fn home_for_profile_maps_named_to_subdir() {
    let base = base_opencrabs_dir();
    assert_eq!(
        home_for_profile(Some("ops")),
        base.join("profiles").join("ops")
    );
    assert_eq!(
        home_for_profile(Some("staging")),
        base.join("profiles").join("staging")
    );
}

#[test]
fn with_profile_home_applies_override_inside_closure() {
    let ops_home = home_for_profile(Some("ops"));
    // Inside the scope, all home resolution points at the ops profile, which is
    // how Config::load() and the brain loader pick up the foreign profile.
    let seen = with_profile_home(Some("ops"), resolve_profile_home);
    assert_eq!(seen, ops_home);
}

#[test]
fn with_profile_home_clears_override_after_closure() {
    let ops_home = home_for_profile(Some("ops"));
    let _ = with_profile_home(Some("ops"), resolve_profile_home);
    // After the scope, the override is gone: resolution falls back to the
    // process profile (the base dir in tests, since no -p is set), never the
    // ops dir we briefly overrode to.
    assert_ne!(resolve_profile_home(), ops_home);
}

#[test]
fn with_profile_home_returns_closure_value() {
    let value = with_profile_home(Some("ops"), || 42);
    assert_eq!(value, 42);
}

#[test]
fn with_profile_home_default_resolves_to_base() {
    let base = base_opencrabs_dir();
    let seen = with_profile_home(Some("default"), resolve_profile_home);
    assert_eq!(seen, base);
}
