//! Restart binary-path resolution (#179 + the /rebuild follow-up).
//!
//! Two failure modes this pins:
//!  1. A pre-built install must restart the RUNNING exe — not a never-built
//!     `~/.opencrabs/source/target/release/opencrabs`. The original #179 bug
//!     was `auto_detect()` pointing `binary_path` at that unbuilt path, so
//!     restart after auto-update hit "exec() failed: No such file or
//!     directory".
//!  2. A source tree resolves to `<root>/target/release/opencrabs` (the
//!     /rebuild output).
//!
//! `resolve_paths` is the pure core of `auto_detect()`, factored out so it
//! can be tested without depending on the real `current_exe()`.

use crate::brain::self_update::SelfUpdater;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn prebuilt_install_resolves_binary_to_the_running_exe() {
    // exe lives in a dir with NO Cargo.toml anywhere up the tree.
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("usr").join("local").join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let exe = bin_dir.join("opencrabs");
    std::fs::write(&exe, b"binary").unwrap();
    let source_dir = tmp.path().join("home").join(".opencrabs").join("source");

    let (root, binary) = SelfUpdater::resolve_paths(&exe, source_dir.clone()).unwrap();

    // #179: the binary to restart is the exe ITSELF (which /evolve replaces
    // in place), never the unbuilt source target dir.
    assert_eq!(
        binary, exe,
        "pre-built install must restart the running exe, not a never-built target/ path"
    );
    assert_ne!(
        binary,
        source_dir.join("target").join("release").join("opencrabs"),
        "must NOT point at the unbuilt source target (the #179 regression)"
    );
    // project_root is just the lazy-clone target for a future /rebuild.
    assert_eq!(root, source_dir);
}

#[test]
fn source_tree_resolves_binary_to_target_release() {
    // exe under a project root that has a Cargo.toml.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let exe_dir = root.join("target").join("debug");
    std::fs::create_dir_all(&exe_dir).unwrap();
    std::fs::write(root.join("Cargo.toml"), b"[package]\nname=\"x\"").unwrap();
    let exe = exe_dir.join("opencrabs");
    std::fs::write(&exe, b"binary").unwrap();

    let (proot, binary) =
        SelfUpdater::resolve_paths(&exe, PathBuf::from("/unused/source")).unwrap();

    assert_eq!(proot, root, "project_root is the Cargo.toml dir");
    assert_eq!(
        binary,
        root.join("target").join("release").join("opencrabs"),
        "source tree restarts the release build output"
    );
}

#[test]
fn nested_source_tree_walks_up_to_the_cargo_toml() {
    // A deeply nested exe (target/release/deps/opencrabs-<hash>, the layout
    // `cargo test` itself uses) still finds the root Cargo.toml.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    let deep = root.join("target").join("release").join("deps");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(root.join("Cargo.toml"), b"[package]\nname=\"x\"").unwrap();
    let exe = deep.join("opencrabs-abc123");
    std::fs::write(&exe, b"binary").unwrap();

    let (proot, binary) = SelfUpdater::resolve_paths(&exe, PathBuf::from("/unused")).unwrap();
    assert_eq!(proot, root);
    assert_eq!(
        binary,
        root.join("target").join("release").join("opencrabs")
    );
}
