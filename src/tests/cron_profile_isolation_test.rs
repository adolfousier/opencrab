//! Tests for `cron::job_runs_in_active_profile` — the guard that stops a cron
//! job from executing under the wrong profile's brain/config/tools (#182).
//!
//! The process-global active profile (a `OnceLock`) drives `Config::load()`,
//! the brain loader, and the DB path, and cannot change at runtime. So the
//! scheduler can only run a job with the active profile's environment. The
//! guard compares each job's stamped origin profile against the active profile
//! and skips on mismatch. Legacy jobs (no stamp) always run for back-compat.

use crate::cron::job_runs_in_active_profile;

#[test]
fn legacy_unstamped_job_runs_anywhere() {
    // Pre-stamping rows have profile_name = NULL. We can't know their origin
    // and the per-profile DB already isolates them, so they must still run.
    assert!(job_runs_in_active_profile(None, None));
    assert!(job_runs_in_active_profile(None, Some("ops")));
    assert!(job_runs_in_active_profile(None, Some("default")));
}

#[test]
fn default_stamped_job_runs_in_default_process() {
    // Base-profile process: active_profile() returns None, normalized to
    // "default". A job stamped "default" matches.
    assert!(job_runs_in_active_profile(Some("default"), None));
    assert!(job_runs_in_active_profile(Some("default"), Some("default")));
}

#[test]
fn named_profile_job_runs_in_its_own_process() {
    assert!(job_runs_in_active_profile(Some("ops"), Some("ops")));
}

#[test]
fn ops_job_does_not_run_in_default_process() {
    // The core bug: an "ops" job picked up from a shared DB by the default
    // process must be skipped, not run under default's brain/config.
    assert!(!job_runs_in_active_profile(Some("ops"), None));
    assert!(!job_runs_in_active_profile(Some("ops"), Some("default")));
}

#[test]
fn default_job_does_not_run_in_named_process() {
    assert!(!job_runs_in_active_profile(Some("default"), Some("ops")));
}

#[test]
fn mismatched_named_profiles_do_not_cross() {
    assert!(!job_runs_in_active_profile(Some("ops"), Some("staging")));
    assert!(!job_runs_in_active_profile(Some("staging"), Some("ops")));
}
