//! #1158 regression tests: completed-plan card finalization support logic.
//!
//! Covers the two pure contracts behind `finalize_plan_card`:
//! 1. The "archived moments ago" recency gate (`recent_archive_in_dir`) that
//!    keeps finalization from firing on unrelated later settles (tool_loop
//!    archives at EVERY settling plan-turn; existence alone is not enough).
//! 2. The one-shot untrack contract (`TelegramState::take_plan_card`) that
//!    makes resurrection impossible (#809 lesson): after finalize takes the
//!    card, no refresh sees a tracked card ever again.

use crate::utils::plan_files::recent_archive_in_dir;
use std::time::Duration;

#[test]
fn recent_archive_in_dir_missing_dir_is_false() {
    let tmp =
        std::env::temp_dir().join(format!("oc1158-missing-{}", uuid::Uuid::new_v4().simple()));
    // Deliberately do NOT create it.
    assert!(!recent_archive_in_dir(&tmp, Duration::from_secs(120)));
}

#[test]
fn recent_archive_in_dir_fresh_file_is_true() {
    let tmp = std::env::temp_dir().join(format!("oc1158-fresh-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("20260101-000000.md"), "# done").unwrap();
    assert!(recent_archive_in_dir(&tmp, Duration::from_secs(120)));
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn recent_archive_in_dir_stale_file_is_false() {
    let tmp = std::env::temp_dir().join(format!("oc1158-stale-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("20250101-000000.md");
    std::fs::write(&path, "# done long ago").unwrap();
    // Age the mtime beyond the window so this is a real staleness test even
    // though the file was just written.
    let f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    f.set_modified(old).unwrap();
    drop(f);
    assert!(!recent_archive_in_dir(&tmp, Duration::from_secs(120)));
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn finalize_untrack_is_one_shot() {
    let state = crate::channels::telegram::state::TelegramState::new();
    let session_id = uuid::Uuid::new_v4();
    let chat = teloxide::types::ChatId(1158);
    let mid = teloxide::types::MessageId(42);
    // None keeps this test free of teloxide ThreadId construction details;
    // the untrack contract does not depend on threading.
    let thread: Option<teloxide::types::ThreadId> = None;
    state
        .set_plan_card(session_id, chat, thread, mid, "sig".to_string())
        .await;
    // First take succeeds (finalize's edit path), second take is empty
    // (any later refresh/restart): the card cannot come back as live.
    assert!(
        state.plan_card(session_id).await.is_some(),
        "tracked card must be visible to finalize"
    );
    assert_eq!(state.take_plan_card(session_id).await, Some(mid));
    assert!(state.plan_card(session_id).await.is_none());
}

// ---------------------------------------------------------------------------
// #16 regression: outcome-gated flag consumption. The pre-#16 settle gate
// consumed the just-archived flag BEFORE finalize ran; a flood-pacing abort
// then lost the completion notice forever. The contract now: peek never
// consumes, take consumes exactly once, and only finalize (after the notice
// lands) may take.
// ---------------------------------------------------------------------------

use crate::utils::plan_files::{
    mark_just_archived_in_dir, peek_just_archived_in_dir, take_just_archived_in_dir,
};

fn flag_test_dir(tag: &str) -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!("oc16-{tag}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

#[test]
fn just_archived_peek_never_consumes() {
    let dir = flag_test_dir("peek");
    let session_id = uuid::Uuid::new_v4();
    assert!(!peek_just_archived_in_dir(&dir, session_id));
    mark_just_archived_in_dir(&dir, session_id);
    // Two peeks in a row both see the flag: gate checks are idempotent and
    // can never eat the retry trail out from under finalize.
    assert!(peek_just_archived_in_dir(&dir, session_id));
    assert!(peek_just_archived_in_dir(&dir, session_id));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn just_archived_take_consumes_exactly_once() {
    let dir = flag_test_dir("take");
    let session_id = uuid::Uuid::new_v4();
    // No flag: take is false, not an error.
    assert!(!take_just_archived_in_dir(&dir, session_id));
    mark_just_archived_in_dir(&dir, session_id);
    assert!(
        take_just_archived_in_dir(&dir, session_id),
        "first take consumes"
    );
    assert!(
        !take_just_archived_in_dir(&dir, session_id),
        "second take is empty"
    );
    assert!(
        !peek_just_archived_in_dir(&dir, session_id),
        "flag gone after take"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn just_archived_remark_rearms_after_consume() {
    let dir = flag_test_dir("rearm");
    let session_id = uuid::Uuid::new_v4();
    mark_just_archived_in_dir(&dir, session_id);
    assert!(take_just_archived_in_dir(&dir, session_id));
    // A later completion stamps a fresh flag: the one-shot guard is per
    // completion, not per session lifetime.
    mark_just_archived_in_dir(&dir, session_id);
    assert!(peek_just_archived_in_dir(&dir, session_id));
    assert!(take_just_archived_in_dir(&dir, session_id));
    std::fs::remove_dir_all(&dir).ok();
}
