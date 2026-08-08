//! Turn duration must survive the turn settling (#964).
//!
//! The header used to re-derive the duration by subtracting the turn's first
//! and last message timestamps. That collapses to zero for the overwhelming
//! majority of turns, because the assistant row is created at turn START and
//! updated in place, so it carries the same `created_at` as the user message
//! that triggered it. The live clock was thrown away at settle, so the
//! duration simply vanished the moment a turn finished.

use crate::tui::app::DisplayMessage;
use crate::tui::render::chat::{TurnRange, turn_summary};
use chrono::{Duration, Utc};
use uuid::Uuid;

fn msg(role: &str, at: chrono::DateTime<Utc>) -> DisplayMessage {
    DisplayMessage {
        id: Uuid::new_v4(),
        role: role.to_string(),
        content: String::new(),
        timestamp: at,
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: None,
        expanded: false,
        expanded_full: false,
        tool_group: None,
        duration_secs: None,
    }
}

fn whole(n: usize) -> TurnRange {
    TurnRange { start: 0, end: n }
}

#[test]
fn stored_duration_survives_identical_timestamps() {
    // The regression: user and assistant share a created_at, so subtraction
    // yields 0 and the header dropped the duration entirely.
    let now = Utc::now();
    let mut assistant = msg("assistant", now);
    assistant.duration_secs = Some(1370);

    let messages = vec![msg("user", now), assistant];
    assert_eq!(turn_summary(&messages, whole(2)).duration_secs, 1370);
}

#[test]
fn stored_duration_wins_over_timestamp_subtraction() {
    // Timestamps say 5s, the stamped clock says 930s. The stamped value is
    // the real turn length; the timestamps only bracket two DB rows.
    let start = Utc::now();
    let mut assistant = msg("assistant", start + Duration::seconds(5));
    assistant.duration_secs = Some(930);

    let messages = vec![msg("user", start), assistant];
    assert_eq!(turn_summary(&messages, whole(2)).duration_secs, 930);
}

#[test]
fn falls_back_to_timestamps_for_rows_without_a_stored_value() {
    // Rows written before the column existed keep the old behaviour rather
    // than losing their duration outright.
    let start = Utc::now();
    let messages = vec![
        msg("user", start),
        msg("assistant", start + Duration::seconds(42)),
    ];
    assert_eq!(turn_summary(&messages, whole(2)).duration_secs, 42);
}

#[test]
fn a_negative_stored_duration_is_clamped() {
    let now = Utc::now();
    let mut assistant = msg("assistant", now);
    assistant.duration_secs = Some(-7);

    let messages = vec![msg("user", now), assistant];
    assert_eq!(turn_summary(&messages, whole(2)).duration_secs, 0);
}

#[test]
fn the_last_stamped_value_in_the_turn_wins() {
    // A turn can carry more than one assistant row; the final one holds the
    // full turn length, so it is the one that must be reported.
    let now = Utc::now();
    let mut first = msg("assistant", now);
    first.duration_secs = Some(12);
    let mut last = msg("assistant", now);
    last.duration_secs = Some(600);

    let messages = vec![msg("user", now), first, last];
    assert_eq!(turn_summary(&messages, whole(3)).duration_secs, 600);
}

#[test]
fn zero_stays_zero_so_instant_turns_stay_quiet() {
    // The header suppresses the field on 0, which is correct for a turn that
    // genuinely took under a second.
    let now = Utc::now();
    let mut assistant = msg("assistant", now);
    assistant.duration_secs = Some(0);

    let messages = vec![msg("user", now), assistant];
    assert_eq!(turn_summary(&messages, whole(2)).duration_secs, 0);
}
