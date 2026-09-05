//! `/compact` wording (#1375): the note is a starting note, not live status,
//! and a dropped dispatch is reported rather than swallowed.

use crate::tui::compact_notice::{dispatch_failed, requested};

#[test]
fn the_note_names_the_usage_and_says_it_is_not_live() {
    let n = requested(62.4);
    assert!(n.starts_with("Compaction requested at 62% context."), "{n}");
    assert!(
        n.contains("does not update"),
        "must not read as a live indicator: {n}"
    );
    assert!(n.contains("footer"), "must point at the live figure: {n}");
    assert!(
        !n.contains("Compacting context..."),
        "the old live-looking phrasing is gone: {n}"
    );
}

#[test]
fn a_failed_dispatch_names_the_command_and_says_nothing_is_running() {
    let e = dispatch_failed(&"channel closed");
    assert!(e.starts_with("/compact was not dispatched"), "{e}");
    assert!(e.contains("nothing is compacting"), "{e}");
    assert!(e.ends_with("channel closed"), "the cause rides along: {e}");
}
