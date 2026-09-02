//! A context that shrank must always leave a marker behind.
//!
//! Dropping the oldest messages advances the live context but not the DB.
//! `messages_from_last_compaction` keeps looking at the previous anchor, so a
//! restart reloads exactly the history that just overflowed and overflows
//! again on the first turn. Two sessions died that way on 2026-05-05 (397%
//! and 372% context with 793k tokens still on disk, a fresh loop on every
//! user message). A marker carrying no summary is lossy; no marker at all is
//! unrecoverable, so the truncation path now returns an outcome of its own
//! rather than `None`.

use crate::brain::agent::service::AgentService;
use crate::brain::agent::service::compaction::CompactionOutcome;

/// Whatever the outcome, the persisted row has to be findable by the loader.
/// That prefix is the only thing standing between a restart and a replay.
const MARKER_PREFIX: &str = "[CONTEXT COMPACTION";

#[test]
fn summarised_marker_carries_the_summary() {
    let out = CompactionOutcome::Summarised("## What happened\nWe fixed the parser.".into());
    let marker = out.marker("");
    assert!(marker.starts_with(MARKER_PREFIX));
    assert!(marker.contains("We fixed the parser."));
}

#[test]
fn truncated_marker_is_still_a_marker() {
    let marker = CompactionOutcome::Truncated.marker("");
    assert!(
        marker.starts_with(MARKER_PREFIX),
        "truncation row is invisible to the loader: {marker}"
    );
}

#[test]
fn truncated_marker_does_not_promise_a_summary() {
    let marker = CompactionOutcome::Truncated.marker("");
    assert!(
        !marker.contains("Below is a structured summary"),
        "marker claims a summary that was never produced: {marker}"
    );
    assert!(
        marker.contains("No summary is available"),
        "marker leaves the agent guessing why history vanished: {marker}"
    );
}

#[test]
fn trigger_wording_rides_both_variants() {
    let trigger = " after token calibration revealed high context usage";
    for marker in [
        CompactionOutcome::Summarised("body".into()).marker(trigger),
        CompactionOutcome::Truncated.marker(trigger),
    ] {
        assert!(marker.starts_with(MARKER_PREFIX));
        assert!(marker.contains(trigger.trim()), "trigger dropped: {marker}");
    }
}

/// The loader is the reason the prefix matters, so assert against the loader
/// itself rather than against a copy of the string it looks for.
#[test]
fn loader_anchors_on_a_truncation_marker() {
    let row = |content: &str| crate::db::models::Message {
        id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::nil(),
        role: "user".to_string(),
        content: content.to_string(),
        sequence: 0,
        created_at: chrono::Utc::now(),
        token_count: None,
        cost: None,
        input_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        thinking: None,
        duration_secs: None,
    };

    let all = vec![
        row("ancient history"),
        row("more ancient history"),
        row(&CompactionOutcome::Truncated.marker("")),
        row("after the truncation"),
    ];

    let kept = AgentService::messages_from_last_compaction(all);
    assert_eq!(kept.len(), 2, "loader ignored the truncation marker");
    assert!(kept[0].content.starts_with(MARKER_PREFIX));
    assert_eq!(kept[1].content, "after the truncation");
}
