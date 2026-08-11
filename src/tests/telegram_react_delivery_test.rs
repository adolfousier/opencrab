//! A react directive must never cost the user the answer beside it (#1009).
//!
//! #928 suppressed the text on any react turn that had run no tools, on the
//! theory that a react turn holds no answer and whatever is in the content
//! channel must be reasoning. Tool state does not answer that question: an
//! answer built from analysis alone runs no tools, which is the ordinary shape
//! of an explanation or a follow-up. Four consecutive completions were dropped
//! that way in one conversation, silently, while the same turns rendered in
//! full on the TUI.
//!
//! The decision is now exactly the contract taught to the model: ONLY the
//! directive means react-only, anything after it is an answer to deliver.

use crate::channels::telegram::delivery::is_react_only;

/// The documented react-only shape: nothing survives stripping the directive.
#[test]
fn a_bare_directive_is_react_only() {
    assert!(is_react_only(""));
    assert!(is_react_only("   "));
    assert!(is_react_only("\n\n  \t\n"));
}

/// The documented react-AND-respond shape must reach the user.
#[test]
fn a_directive_followed_by_an_answer_is_not_react_only() {
    assert!(!is_react_only("Done, uploaded to Drive."));
    assert!(!is_react_only(
        "Your read is spot on. The bubbles translate to a compile loop."
    ));
}

/// The regression itself: these answers ran ZERO tools and were destroyed.
///
/// Nothing about the text is inspected, so the assertion stands for any
/// content; what matters is that no tool-state input exists to flip it.
#[test]
fn an_answer_that_needed_no_tools_is_still_delivered() {
    let analysis = "It is not the model. 2.2 minutes per call is command-wait, \
                    not inference. The fix: probe the binary before rebuilding.";
    assert!(!is_react_only(analysis));
}

/// A table in the answer is answer content like any other.
///
/// The dropped turns each carried one, and the drop happened before the rich
/// renderer ran, so the table never had a chance to render.
#[test]
fn an_answer_carrying_a_table_is_not_react_only() {
    let with_table = "Here is the breakdown:\n\n\
                      | Profile | Result | Tool calls |\n\
                      |---|---|---|\n\
                      | Slow | running | 111 |\n\
                      | Fast | finished | 84 |\n";
    assert!(!is_react_only(with_table));
}

/// Leading whitespace from stripping the directive must not read as empty.
#[test]
fn leading_whitespace_before_the_answer_does_not_hide_it() {
    assert!(!is_react_only("   \n\n  Yes, exactly that."));
}
