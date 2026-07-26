//! Re-submitting the message already being answered (#798).
//!
//! Arrow Up then Enter while a turn is still running used to queue the text
//! unconditionally, so an accidental double-Enter became a second identical
//! turn. Both copies persist and both reload into context on every later turn,
//! so the waste is paid repeatedly, not once.
//!
//! The comparison is deliberately short-sighted: only the turn in flight and
//! the pending queue. Repetition is meaningless only while the first copy has
//! not been answered yet.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::tui::app::duplicate_submit::{Submission, classify};

const MSG: &str = "check the owner gate on channel commands";

#[test]
fn resubmitting_the_message_in_flight_is_dropped() {
    // The reported case: the answer being produced IS the answer to it.
    assert_eq!(
        classify(MSG, Some(MSG), &[]),
        Submission::DuplicateOfInFlight
    );
}

#[test]
fn resubmitting_something_already_queued_is_dropped() {
    // Queueing the same text twice can only ever produce a duplicate.
    assert_eq!(
        classify(MSG, Some("something else"), &[MSG.to_string()]),
        Submission::DuplicateOfQueued
    );
}

#[test]
fn a_different_message_is_queued() {
    assert_eq!(
        classify("a new question", Some(MSG), &[MSG.to_string()]),
        Submission::Queue
    );
}

#[test]
fn whitespace_differences_do_not_defeat_the_check() {
    // History recall can reintroduce a trailing newline the original lacked.
    assert_eq!(
        classify("  check this  \n", Some("check this"), &[]),
        Submission::DuplicateOfInFlight
    );
}

#[test]
fn nothing_in_flight_means_nothing_to_duplicate() {
    // A retry after the first attempt failed or was cancelled must go through:
    // the session is no longer processing, and swallowing it would lose work.
    assert_eq!(classify(MSG, None, &[]), Submission::Queue);
}

#[test]
fn a_near_match_is_still_queued() {
    // Only exact matches are dropped. A wrongly dropped message silently loses
    // work, which is far worse than paying for a duplicate.
    assert_eq!(
        classify("check the owner gate on channel command", Some(MSG), &[]),
        Submission::Queue
    );
}

#[test]
fn an_empty_submission_is_not_a_duplicate() {
    // The caller's own empty-input guard owns this case; classify must not
    // claim an empty string duplicates an empty in-flight message.
    assert_eq!(classify("   ", Some("   "), &[]), Submission::Queue);
}

#[test]
fn only_queue_is_not_a_duplicate() {
    assert!(!Submission::Queue.is_duplicate());
    assert!(Submission::DuplicateOfInFlight.is_duplicate());
    assert!(Submission::DuplicateOfQueued.is_duplicate());
}
