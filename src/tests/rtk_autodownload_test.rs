//! Tests for the RTK first-use auto-download wiring in `src/rtk/rewrite.rs`.
//!
//! These guard the two things that silently break the feature: the platform
//! to release-asset mapping, and the pinned RTK version drifting away from the
//! version the release workflow bundles.

use crate::rtk::rewrite::{RTK_VERSION, rtk_asset_name, rtk_bin_filename};

#[test]
fn current_platform_has_a_release_asset() {
    // Whatever platform the test runs on must map to a real RTK asset,
    // otherwise auto-download can never succeed there.
    let asset = rtk_asset_name();
    assert!(
        asset.is_some(),
        "no RTK release asset mapped for {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

#[test]
fn asset_extension_matches_platform() {
    if let Some(asset) = rtk_asset_name() {
        if cfg!(windows) {
            assert!(
                asset.ends_with(".zip"),
                "windows asset must be a .zip: {asset}"
            );
        } else {
            assert!(
                asset.ends_with(".tar.gz"),
                "unix asset must be a .tar.gz: {asset}"
            );
        }
    }
}

#[test]
fn bin_filename_matches_platform() {
    if cfg!(windows) {
        assert_eq!(rtk_bin_filename(), "rtk.exe");
    } else {
        assert_eq!(rtk_bin_filename(), "rtk");
    }
}

#[test]
fn pinned_version_matches_release_workflow() {
    // The auto-download version must equal the version the release workflow
    // bundles, so source builds install the same RTK prebuilt releases ship.
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.github/workflows/release.yml"
    ))
    .expect("release.yml should exist at the repo root");

    let needle = "RTK_VERSION=\"";
    let start = workflow
        .find(needle)
        .map(|i| i + needle.len())
        .expect("release.yml should pin RTK_VERSION");
    let end = workflow[start..]
        .find('"')
        .map(|i| start + i)
        .expect("RTK_VERSION should be a quoted string");
    let workflow_version = &workflow[start..end];

    assert_eq!(
        workflow_version, RTK_VERSION,
        "rtk::RTK_VERSION ({RTK_VERSION}) drifted from release.yml ({workflow_version})"
    );
}
