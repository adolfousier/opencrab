//! Same-character run detection in a reasoning window (#1351).

use crate::brain::agent::service::helpers::detect_text_repetition;
use crate::brain::agent::service::reasoning_run::{MIN_RUN, degenerate_run};

#[test]
fn a_bang_run_at_the_threshold_is_degeneration() {
    let w = format!("Let me think.{}", "!".repeat(MIN_RUN));
    assert_eq!(degenerate_run(&w, MIN_RUN), Some(('!', MIN_RUN)));
    let short = format!("Really{}", "!".repeat(MIN_RUN - 1));
    assert_eq!(degenerate_run(&short, MIN_RUN), None);
}

#[test]
fn the_run_must_be_at_the_tail() {
    // The run ended and the model moved on: whatever happened is over.
    let w = format!("{} and then a plan", "!".repeat(MIN_RUN * 2));
    assert_eq!(degenerate_run(&w, MIN_RUN), None);
}

#[test]
fn whitespace_is_never_a_run() {
    assert_eq!(degenerate_run(&" ".repeat(MIN_RUN * 8), MIN_RUN), None);
    assert_eq!(degenerate_run(&"\n".repeat(MIN_RUN * 8), MIN_RUN), None);
}

#[test]
fn dividers_need_four_times_the_threshold() {
    let rule = format!("Summary\n{}", "-".repeat(80));
    assert_eq!(
        degenerate_run(&rule, MIN_RUN),
        None,
        "an 80-column rule is prose"
    );
    let runaway = "=".repeat(MIN_RUN * 4);
    assert_eq!(degenerate_run(&runaway, MIN_RUN), Some(('=', MIN_RUN * 4)));
}

#[test]
fn multibyte_characters_count_by_char_not_byte() {
    let w = "…".repeat(MIN_RUN);
    assert_eq!(degenerate_run(&w, MIN_RUN), Some(('…', MIN_RUN)));
}

#[test]
fn it_fires_long_before_the_substring_guard_would() {
    // The existing guard needs two matching 300-byte halves: a pure run is
    // invisible to it until 600 bytes. This one sees it at 64.
    let w = "!".repeat(MIN_RUN);
    assert!(degenerate_run(&w, MIN_RUN).is_some());
    assert!(!detect_text_repetition(&w, 300));
}
