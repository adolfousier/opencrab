//! Verification runs with the project's own toolchain, not cargo's.
//!
//! The machine-wide `ralph_loop.toml` names cargo and governs every project
//! without its own, so on a box holding Zig, Flutter, Python and TypeScript
//! work a task either matched no entry and was skipped, or was handed a
//! toolchain the project does not have.

use std::fs;

use tempfile::TempDir;

use crate::brain::tools::project_runner::{ProjectKind, detect, fallback_commands};

fn project_with(marker: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join(marker), "").expect("write marker");
    dir
}

#[test]
fn each_manifest_names_its_own_toolchain() {
    for (marker, expected) in [
        ("Cargo.toml", ProjectKind::Rust),
        ("build.zig", ProjectKind::Zig),
        ("pubspec.yaml", ProjectKind::Flutter),
        ("go.mod", ProjectKind::Go),
        ("pyproject.toml", ProjectKind::Python),
        ("package.json", ProjectKind::Node),
    ] {
        let dir = project_with(marker);
        assert_eq!(detect(dir.path()), Some(expected), "marker {marker}");
    }
}

#[test]
fn a_zig_project_is_tested_with_zig() {
    // The reported case: a task skipped because the gate only knew cargo.
    let dir = project_with("build.zig");
    assert_eq!(
        fallback_commands(dir.path(), "test"),
        Some(vec!["zig build test".to_string()])
    );
}

#[test]
fn a_flutter_project_is_tested_with_flutter() {
    let dir = project_with("pubspec.yaml");
    assert_eq!(
        fallback_commands(dir.path(), "test"),
        Some(vec!["flutter test".to_string()])
    );
    assert_eq!(
        fallback_commands(dir.path(), "build"),
        Some(vec!["flutter analyze".to_string()])
    );
}

#[test]
fn flutter_wins_over_a_bare_dart_or_node_manifest() {
    // A Flutter package carries other manifests too; the most specific one
    // decides, or the runner would be wrong in the repositories that matter.
    let dir = project_with("pubspec.yaml");
    fs::write(dir.path().join("package.json"), "{}").unwrap();
    assert_eq!(detect(dir.path()), Some(ProjectKind::Flutter));
}

#[test]
fn an_unrecognised_project_still_verifies_nothing() {
    // Guessing a runner from no manifest would run a command that describes
    // nothing. Skipping remains correct here.
    let dir = TempDir::new().unwrap();
    assert_eq!(detect(dir.path()), None);
    assert_eq!(fallback_commands(dir.path(), "test"), None);
}

#[test]
fn a_type_without_an_obvious_meaning_is_left_alone() {
    // Only build and test have an unambiguous command. A refactor or docs task
    // must not be handed one that does not describe it.
    let dir = project_with("Cargo.toml");
    assert_eq!(fallback_commands(dir.path(), "refactor"), None);
    assert_eq!(fallback_commands(dir.path(), "documentation"), None);
}
