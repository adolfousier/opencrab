//! The vacuous-pass guard, across the runners a multi-language box actually uses.
//!
//! It read cargo's format alone, so in any repository that is not Rust the
//! protection did not exist: a filter that matched nothing exited 0, parsed as
//! `None`, and the gate stamped it verified.

use crate::brain::tools::plan_tool::parse_test_pass_count;

#[test]
fn cargo_still_parses_both_ways() {
    assert_eq!(
        parse_test_pass_count("test result: ok. 42 passed; 0 failed; 3 ignored"),
        Some(42)
    );
    assert_eq!(
        parse_test_pass_count("test result: ok. 0 passed; 0 failed; 5783 filtered out"),
        Some(0),
        "the filtered-out case from the report is the whole point"
    );
}

#[test]
fn pytest_reports_its_count() {
    assert_eq!(
        parse_test_pass_count("===== 5 passed, 2 warnings in 0.31s ====="),
        Some(5)
    );
}

#[test]
fn pytest_spells_zero_in_words() {
    // pytest does not print "0 passed" when a filter matches nothing.
    assert_eq!(
        parse_test_pass_count("===== no tests ran in 0.01s ====="),
        Some(0)
    );
}

#[test]
fn jest_and_vitest_report_their_count() {
    assert_eq!(
        parse_test_pass_count("Tests:       5 passed, 5 total"),
        Some(5)
    );
    assert_eq!(parse_test_pass_count("Tests  0 passed (0)"), Some(0));
}

#[test]
fn flutter_counts_from_its_progress_marker() {
    assert_eq!(
        parse_test_pass_count("00:02 +7: All tests passed!"),
        Some(7)
    );
    assert_eq!(
        parse_test_pass_count("00:00 +0: All tests passed!"),
        Some(0),
        "a filter matching nothing still says all tests passed"
    );
}

#[test]
fn go_distinguishes_a_package_with_no_tests() {
    assert_eq!(
        parse_test_pass_count("ok  	example/pkg	0.02s"),
        Some(1),
        "something ran; the guard only cares that it was not zero"
    );
    assert_eq!(
        parse_test_pass_count("?   	example/pkg	[no test files]"),
        Some(0)
    );
}

#[test]
fn output_with_no_test_summary_is_not_a_zero() {
    // A lint, a build or a grep is a legitimate verification command with no
    // count to report. Reading that as zero would fail every one of them.
    assert_eq!(
        parse_test_pass_count("Finished `dev` profile in 3.2s"),
        None
    );
    assert_eq!(parse_test_pass_count(""), None);
    assert_eq!(
        parse_test_pass_count("error: could not compile `thing`"),
        None
    );
}
