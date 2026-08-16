//! Dotted commands survive the sentence split (#1074).
//!
//! `claims_uncalled_commands` cut the turn text into sentences on `.` before it
//! looked for backticks, and the span parser needs an opening and a closing
//! backtick in the same fragment. Any command naming a dotted path, a file
//! extension or a domain was therefore torn in half and never parsed:
//! `` `wc -l src/tui/mod.rs` `` became `` `wc -l src/tui/mod `` plus `` rs` ``.
//!
//! That is the majority shape of a real inspection claim, because the invented
//! fact is normally about one specific file. The allowlist widening in
//! b343f8e9 could not surface it: every fixture there was deliberately
//! dot-free, and the pre-existing `extra_flags_on_the_real_call_are_not_a_
//! fabrication` asserted `is_empty()` on a dotted fixture, passing for the
//! wrong reason.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_uncalled_commands;

#[test]
fn a_dotted_path_no_longer_hides_the_command() {
    let text = "I ran `wc -l src/tui/mod.rs` and it is 412 lines";
    let executed = vec![r#"{"command":"git status --short"}"#.to_string()];
    assert_eq!(
        claims_uncalled_commands(text, &executed),
        vec!["wc -l src/tui/mod.rs"]
    );
}

#[test]
fn a_dotted_command_that_really_ran_stays_clean() {
    // The head match still does its job once the span is actually parsed: the
    // real call added a pipeline the prose omitted.
    let text = "Checked with `wc -l src/tui/mod.rs`, real output above:";
    let executed = vec![r#"{"command":"cd /repo && wc -l src/tui/mod.rs | tail -1"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}

#[test]
fn dotted_shapes_across_families_are_all_visible() {
    // Extension, domain and relative path. Each was invisible for the same
    // reason, and each names a fact a turn is tempted to state without looking.
    for (text, expected) in [
        (
            "I ran `stat -f %m target/release/opencrabs.tmp` and it is fresh",
            "stat -f %m target/release/opencrabs.tmp",
        ),
        ("Ran `dig example.com` and it resolves", "dig example.com"),
        ("Ran `go build ./...` clean", "go build ./..."),
        (
            "Verified with `shasum -a 256 src/main.rs`, hash above",
            "shasum -a 256 src/main.rs",
        ),
    ] {
        assert_eq!(
            claims_uncalled_commands(text, &[]),
            vec![expected.to_string()],
            "dotted command must be visible: {text}"
        );
    }
}

#[test]
fn a_sentence_boundary_still_separates_framing_from_command() {
    // The split has to keep working outside backticks, or a framing in one
    // sentence would vouch for a command in the next one.
    let text = "I ran the tests. Next up is `cargo clippy --all-features` on a clean tree";
    assert!(claims_uncalled_commands(text, &[]).is_empty());
}

#[test]
fn a_stray_backtick_does_not_merge_sentences() {
    // Unmatched openers are dropped rather than treated as an open span. If the
    // stray backtick swallowed the remainder, the earlier framing would reach
    // the later proposal and flag an honest turn.
    let text = "I ran the check on `mod.rs and it was fine. \
                You could run `cargo test --all-features` next";
    assert!(claims_uncalled_commands(text, &[]).is_empty());
}

#[test]
fn multiline_output_recaps_are_still_split_per_line() {
    // Newlines are terminators too, and they must keep separating a real recap
    // from a fabricated one on the following line.
    let text = "I ran `git log --oneline -5` and the output is above\n\
                Someone should run `cargo test --all-features` too";
    let executed = vec![r#"{"command":"git log --oneline -5"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}
