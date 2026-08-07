//! Regression (#963): a Homebrew-managed install is recognised as such.
//!
//! `InstallMethod` had three variants and none of them was Homebrew, so a brew
//! install fell through to `PrebuiltBinary`. That path renames a downloaded
//! GitHub asset over the running binary — which SUCCEEDS in a Cellar, because
//! the prefix is user-owned, and leaves brew reporting a version that no longer
//! matches the disk until an unrelated `brew upgrade` silently reverts the user.
//!
//! Detection is path-based on purpose: `brew --prefix` needs brew on PATH,
//! which it often is not in a spawned process, and costs a subprocess to answer
//! what the path already answers.

use crate::brain::tools::evolve::homebrew;
use crate::utils::install::is_in_homebrew_cellar;
use std::path::Path;

#[test]
fn an_apple_silicon_cellar_binary_is_homebrew() {
    assert!(is_in_homebrew_cellar(Path::new(
        "/opt/homebrew/Cellar/opencrabs/0.3.79/bin/opencrabs"
    )));
}

#[test]
fn an_intel_cellar_binary_is_homebrew() {
    assert!(is_in_homebrew_cellar(Path::new(
        "/usr/local/Cellar/opencrabs/0.3.79/bin/opencrabs"
    )));
}

#[test]
fn a_linuxbrew_cellar_binary_is_homebrew() {
    assert!(is_in_homebrew_cellar(Path::new(
        "/home/linuxbrew/.linuxbrew/Cellar/opencrabs/0.3.79/bin/opencrabs"
    )));
}

#[test]
fn a_custom_prefix_cellar_is_still_homebrew() {
    // HOMEBREW_PREFIX is user-settable, so a known root cannot be required.
    assert!(is_in_homebrew_cellar(Path::new(
        "/Users/someone/brew/Cellar/opencrabs/0.3.79/bin/opencrabs"
    )));
}

#[test]
fn a_hand_installed_binary_in_usr_local_bin_is_not_homebrew() {
    // The whole reason detection keys on the Cellar component: /usr/local/bin
    // is full of binaries Homebrew does not own, and treating those as brew
    // installs would send their owners to `brew upgrade` for a file brew has
    // never heard of.
    assert!(!is_in_homebrew_cellar(Path::new(
        "/usr/local/bin/opencrabs"
    )));
    assert!(!is_in_homebrew_cellar(Path::new(
        "/opt/homebrew/bin/opencrabs"
    )));
}

#[test]
fn a_cargo_install_and_a_source_build_are_not_homebrew() {
    assert!(!is_in_homebrew_cellar(Path::new(
        "/Users/someone/.cargo/bin/opencrabs"
    )));
    assert!(!is_in_homebrew_cellar(Path::new(
        "/Users/someone/src/opencrabs/target/debug/opencrabs"
    )));
}

#[test]
fn another_formulas_cellar_is_still_a_homebrew_layout() {
    // Detection answers "is this path Homebrew-managed", not "is it ours" —
    // the caller already knows which binary it is running.
    assert!(is_in_homebrew_cellar(Path::new(
        "/opt/homebrew/Cellar/ripgrep/14.1.0/bin/rg"
    )));
}

#[test]
fn a_spawn_failure_tells_the_user_the_command_to_run() {
    // A refusal the user cannot act on is worse than the silent overwrite it
    // replaced, so the message has to carry the fix.
    let msg = homebrew::spawn_failure_message("No such file or directory (os error 2)");
    assert!(msg.contains("Homebrew"), "{msg}");
    assert!(msg.contains("brew upgrade opencrabs"), "{msg}");
    assert!(msg.contains("os error 2"), "must keep the cause: {msg}");
}

#[test]
fn an_upgrade_failure_quotes_brew_rather_than_guessing() {
    let msg = homebrew::upgrade_failure_message("Error: opencrabs not installed");
    assert!(msg.contains("brew upgrade failed"), "{msg}");
    assert!(msg.contains("not installed"), "must quote brew: {msg}");
}

#[test]
fn a_long_brew_error_is_truncated_rather_than_dumped() {
    let msg = homebrew::upgrade_failure_message(&"x".repeat(5000));
    assert!(
        msg.len() < 700,
        "a 5000-char stderr must not be pasted into chat whole: {} chars",
        msg.len()
    );
}
