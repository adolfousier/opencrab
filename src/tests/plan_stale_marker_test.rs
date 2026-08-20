//! Regression test for plan-gate stale marker recovery (#1109).
//!
//! When plan creation fails mid-flight (provider timeout, network error), the
//! `.preinit` marker file is left behind. Without staleness detection, the
//! session is locked out of plan operations indefinitely (6.6 hours observed
//! in Adi's audit). This test pins the fix: markers older than 5 minutes are
//! treated as stale and cleared automatically.

use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::utils::plan_files::{
    PRE_INIT_STALE_THRESHOLD, PlanModeState, plan_mode_state, pre_init_marker_path,
    set_pre_init_editing,
};
use std::time::SystemTime;
use uuid::Uuid;

/// Run `f` under a throwaway profile home so nothing touches the real
/// `~/.opencrabs/agents/session/`, then clean the profile dir up.
async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-stale-test-{}", Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

/// Fresh marker (< 5 min old) blocks plan creation: session is PreInitEditing.
#[tokio::test]
async fn fresh_marker_blocks_plan_creation() {
    in_temp_home(async {
        let session_id = Uuid::new_v4();

        // Create a fresh pre-init marker (just created, age ~0)
        set_pre_init_editing(session_id).await.unwrap();

        let state = plan_mode_state(session_id).await;
        assert_eq!(
            state,
            PlanModeState::PreInitEditing,
            "Fresh marker should block plan creation (PreInitEditing state)"
        );
    })
    .await;
}

/// Stale marker (> 5 min old) is cleared automatically: session is NoPlan.
#[tokio::test]
async fn stale_marker_allows_plan_creation() {
    in_temp_home(async {
        let session_id = Uuid::new_v4();

        // Create a pre-init marker
        set_pre_init_editing(session_id).await.unwrap();

        // Manually backdate the marker's mtime to 6 minutes ago
        let marker_path = pre_init_marker_path(session_id).await;
        assert!(marker_path.exists(), "Marker should exist after creation");

        let six_minutes_ago =
            SystemTime::now() - (PRE_INIT_STALE_THRESHOLD + std::time::Duration::from_secs(60));
        let file = std::fs::File::open(&marker_path).unwrap();
        file.set_modified(six_minutes_ago).unwrap();

        let state = plan_mode_state(session_id).await;
        assert_eq!(
            state,
            PlanModeState::NoPlan,
            "Stale marker (>5 min old) should be cleared, returning to NoPlan"
        );

        // Verify the marker file was actually deleted
        assert!(!marker_path.exists(), "Stale marker file should be deleted");
    })
    .await;
}

/// Marker at exactly the threshold (5 min) is NOT stale yet.
#[tokio::test]
async fn marker_at_threshold_is_fresh() {
    in_temp_home(async {
        let session_id = Uuid::new_v4();

        set_pre_init_editing(session_id).await.unwrap();

        let marker_path = pre_init_marker_path(session_id).await;

        // Backdate to just under the threshold (299s, not 300s) to account
        // for timing jitter between write and read. A marker at exactly the
        // threshold boundary should be fresh, but we test 1 second under to
        // avoid flakiness from millisecond-level timing differences.
        let just_under_threshold =
            SystemTime::now() - (PRE_INIT_STALE_THRESHOLD - std::time::Duration::from_secs(1));
        let file = std::fs::File::open(&marker_path).unwrap();
        file.set_modified(just_under_threshold).unwrap();

        let state = plan_mode_state(session_id).await;
        assert_eq!(
            state,
            PlanModeState::PreInitEditing,
            "Marker just under threshold (299s) should still be fresh"
        );
    })
    .await;
}
