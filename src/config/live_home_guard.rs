//! Test builds must never write the live default home (#1399).
//!
//! Two test modules drove the onboarding wizard's voice step to its save
//! without a home override, so every full `cargo test` on a developer
//! machine rewrote the real `~/.opencrabs/config.toml` with the wizard's
//! Off defaults: eight `enabled = false` lines over the user's voice setup.
//! That was the "voice disables itself" the owner enabled eleven times in
//! one evening. The per-key writer logs every write, but a test binary logs
//! nowhere, which is why the rewrite left no fingerprint.
//!
//! The check is pure and lives here; `atomic_write` consults it only in
//! test builds. Profile-scoped test homes (`~/.opencrabs/profiles/<name>`)
//! are not the live default home and pass.

use std::path::Path;

/// `Some(reason)` when a test build is about to write a file that sits
/// directly in the live default home. Callers pass the resolved target and
/// the real base directory so the rule is testable without touching disk.
pub(crate) fn refusal_for(target: &Path, live_default_home: &Path) -> Option<String> {
    if target.parent() != Some(live_default_home) {
        return None;
    }
    Some(format!(
        "refusing to write {} from a test build: tests must run under a home override \
         (with_home_override / in_temp_home), never against the live default profile",
        target.display()
    ))
}

/// The guard `atomic_write` applies in test builds: a no-op in a shipped
/// binary, since the release build carries no tests and users must keep
/// writing their own config.
pub(crate) fn refuse_live_home_write(target: &Path) -> std::io::Result<()> {
    if !cfg!(test) {
        return Ok(());
    }
    match refusal_for(target, &crate::config::profile::base_opencrabs_dir()) {
        Some(reason) => Err(std::io::Error::other(reason)),
        None => Ok(()),
    }
}
