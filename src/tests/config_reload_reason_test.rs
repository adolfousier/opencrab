//! Transient read race vs a real config error (#1116).
//!
//! The original test was `reason.contains("line 1, column 1")`, on the belief
//! that only an empty file reports there. serde reports a FIELD error against
//! the whole struct, which is also line 1, column 1 — so a real error was
//! announced to users as a write race with the assurance "your file is almost
//! certainly fine", while every edit silently did nothing.

use crate::utils::config_reload_reason::is_transient_read_race;

/// The exact error users were shown as a harmless write race.
const DUPLICATE_FIELD: &str = "Failed to parse config file: \"/home/u/.opencrabs/config.toml\": \
     TOML parse error at line 1, column 1\n  |\n1 | [providers]\n  | ^\nduplicate field `a2a`";

#[test]
fn a_duplicate_field_is_never_a_write_race() {
    assert!(
        !is_transient_read_race(DUPLICATE_FIELD, false),
        "a real content error must not be described as transient"
    );
}

#[test]
fn a_duplicate_field_stays_real_even_if_the_file_reads_empty_now() {
    // The file may well be mid-write on the retry, but the error we are
    // explaining is still a content error. Emptiness must not override it.
    assert!(!is_transient_read_race(DUPLICATE_FIELD, true));
}

#[test]
fn other_content_errors_are_real_too() {
    for reason in [
        "unknown field `porrt`, expected one of ...",
        "invalid type: string, expected a boolean",
        "missing field `token`",
        "invalid value: integer `-1`",
    ] {
        assert!(
            !is_transient_read_race(reason, false),
            "{reason} needs a human, not a retry"
        );
    }
}

#[test]
fn an_empty_file_is_a_write_race() {
    assert!(is_transient_read_race(
        "TOML parse error at line 1, column 1",
        true
    ));
}

#[test]
fn an_eof_error_is_a_write_race_without_needing_the_file_check() {
    // The file may already have been rewritten by the time we look.
    assert!(is_transient_read_race("unexpected eof encountered", false));
}

#[test]
fn an_unrecognised_error_is_treated_as_real() {
    // Being wrong toward "real" costs a needless message; being wrong toward
    // "transient" tells the user to ignore a genuine problem.
    assert!(!is_transient_read_race(
        "something nobody has seen before",
        false
    ));
}

#[test]
fn line_one_column_one_alone_no_longer_implies_transient() {
    // The whole bug in one assertion: this signature is shared by an empty
    // read and a struct-level field error, so it cannot decide on its own.
    assert!(!is_transient_read_race(
        "TOML parse error at line 1, column 1\nduplicate field `a2a`",
        false
    ));
}
