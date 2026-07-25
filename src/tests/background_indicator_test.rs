//! Indicator text for detached background commands (#762).
//!
//! Rendered on the input box's top border, so the label has to fit beside the
//! other border titles rather than run the width of a status bar.

use crate::tui::render::background::{LABEL_CHARS, format_background_tasks};

fn tasks(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
    pairs.iter().map(|(l, s)| (l.to_string(), *s)).collect()
}

#[test]
fn nothing_running_shows_no_field() {
    // The caller omits the border title entirely rather than rendering a blank.
    assert_eq!(format_background_tasks(&[], LABEL_CHARS), None);
}

#[test]
fn one_task_names_it_with_elapsed() {
    assert_eq!(
        format_background_tasks(&tasks(&[("cargo test", 32)]), LABEL_CHARS).as_deref(),
        Some("cargo test 32s")
    );
}

#[test]
fn several_tasks_name_the_oldest_and_count_the_rest() {
    // The oldest is the one the user has been waiting on longest, so its
    // elapsed time is the one that answers "is this stuck".
    let t = tasks(&[("cargo test", 90), ("cargo build", 12), ("npm test", 3)]);
    assert_eq!(
        format_background_tasks(&t, LABEL_CHARS).as_deref(),
        Some("cargo test 1m 30s +2")
    );
}

#[test]
fn elapsed_humanises_past_a_minute_and_an_hour() {
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 59)]), LABEL_CHARS).as_deref(),
        Some("build 59s")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 60)]), LABEL_CHARS).as_deref(),
        Some("build 1m")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 260)]), LABEL_CHARS).as_deref(),
        Some("build 4m 20s")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 3600)]), LABEL_CHARS).as_deref(),
        Some("build 1h")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 3900)]), LABEL_CHARS).as_deref(),
        Some("build 1h 5m")
    );
}

#[test]
fn a_task_that_just_started_reads_as_zero_not_blank() {
    // The field appears the instant the command is spawned; a missing number
    // would look like the indicator itself was broken.
    assert_eq!(
        format_background_tasks(&tasks(&[("cargo test", 0)]), LABEL_CHARS).as_deref(),
        Some("cargo test 0s")
    );
}

#[test]
fn a_long_command_is_cut_to_the_label_budget() {
    // The reported case: a real command runs well past the border title's
    // room, e.g. "cargo test --locked --profile ci --lib 2>&1 | tail -40".
    let long = "cargo test --locked --profile ci --lib 2>&1 | tail -40";
    let out = format_background_tasks(&tasks(&[(long, 29)]), LABEL_CHARS).expect("formats");
    assert!(
        out.starts_with("cargo test --locked --profil"),
        "got: {out}"
    );
    assert!(
        out.contains('\u{2026}'),
        "truncation must be marked, got: {out}"
    );
    assert!(out.ends_with("29s"), "elapsed survives the cut, got: {out}");
}

#[test]
fn the_elapsed_time_is_never_truncated() {
    // Truncating the number would defeat the whole indicator, so the budget
    // applies to the command only.
    let long = "x".repeat(200);
    let out = format_background_tasks(&tasks(&[(&long, 3900)]), 10).expect("formats");
    assert!(out.ends_with("1h 5m"), "got: {out}");
}

#[test]
fn the_overflow_count_survives_truncation_too() {
    let long = "y".repeat(200);
    let t = tasks(&[(&long, 5), ("second", 1), ("third", 1)]);
    let out = format_background_tasks(&t, 8).expect("formats");
    assert!(out.ends_with("5s +2"), "got: {out}");
}

#[test]
fn a_label_exactly_at_the_budget_is_not_marked() {
    // Off-by-one guard: an ellipsis on an untruncated label is a lie.
    let exact = "a".repeat(12);
    let out = format_background_tasks(&tasks(&[(&exact, 1)]), 12).expect("formats");
    assert_eq!(out, format!("{exact} 1s"));
}
