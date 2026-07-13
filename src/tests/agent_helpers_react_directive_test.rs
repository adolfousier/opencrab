//! Regression coverage for `retain_react_directive` in
//! `src/brain/agent/service/helpers.rs`.
//!
//! When a claude-cli turn streams only a `<<react:EMOJI>>` marker, the
//! displayable text is flushed as an intermediate message and the text blocks
//! are then reduced before `complete_response` reads them. Previously that
//! reduction cleared the block outright, so the react marker never reached
//! `response.content`; delivery saw empty content, skipped its react-only path,
//! and left a bare "Finished" flow block behind (#547). `retain_react_directive`
//! keeps the marker (and only the marker) so delivery treats the turn as
//! react-only, matching the API providers.

use crate::brain::agent::service::helpers::retain_react_directive;

#[test]
fn react_only_block_keeps_the_marker() {
    assert_eq!(retain_react_directive("<<react:👍>>"), "<<react:👍>>");
}

#[test]
fn text_wrapped_react_marker_is_reduced_to_the_bare_directive() {
    // The displayable prose was already delivered as an intermediate message,
    // so only the directive survives.
    assert_eq!(
        retain_react_directive("All set <<react:🎉>> done"),
        "<<react:🎉>>"
    );
}

#[test]
fn plain_text_block_is_cleared() {
    assert_eq!(retain_react_directive("just some finished prose"), "");
}

#[test]
fn empty_block_stays_empty() {
    assert_eq!(retain_react_directive(""), "");
}
