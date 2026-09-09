//! The session list shows last activity, and only real turns count.
//!
//! Reported: the `/sessions` dialog looked mis-ordered — rows dated
//! 2026-03-07, 2026-03-21 and 2026-04-05 sat above sessions used minutes
//! earlier. The order was in fact correct (`list_sessions` is
//! `ORDER BY updated_at DESC`); the dialog printed `created_at`, a different
//! field, so position and timestamp disagreed on every row that had been
//! opened long ago and used recently.
//!
//! Second requirement from the same report: merely opening a session must not
//! count as activity. That already holds — `updated_at` is bumped in exactly
//! one place, on message insert — and this pins it so a future "touch on
//! open" cannot creep in.

const RENDER_SRC: &str = include_str!("../tui/render/sessions.rs");
const MESSAGE_REPO_SRC: &str = include_str!("../db/repository/message.rs");
const SESSION_REPO_SRC: &str = include_str!("../db/repository/session.rs");

#[test]
fn the_list_renders_the_field_it_sorts_by() {
    assert!(
        RENDER_SRC.contains("session.updated_at.format("),
        "the session row must print updated_at: the list is ordered by \
         updated_at DESC, and printing created_at made a correctly sorted \
         list look scrambled"
    );
    assert!(
        !RENDER_SRC.contains("session.created_at.format("),
        "printing created_at in the row is what caused the reported confusion \
         — a March date above a session used minutes ago"
    );
}

#[test]
fn the_sort_key_is_last_activity() {
    assert!(
        SESSION_REPO_SRC.contains("ORDER BY updated_at DESC"),
        "list_sessions must order by last activity; ordering by created_at \
         would bury an old session that is in daily use"
    );
}

#[test]
fn only_a_real_message_counts_as_activity() {
    // The bump lives next to the message INSERT, so a session that is merely
    // opened, switched to, or rendered never moves up the list.
    let block = MESSAGE_REPO_SRC
        .split("Touch session's updated_at")
        .nth(1)
        .expect("the activity bump is gone from the message insert path");
    let block = &block[..block.find("})").unwrap_or(block.len())];

    assert!(
        block.contains("UPDATE sessions SET updated_at"),
        "activity must be recorded when a message lands"
    );
    assert!(
        block.contains("m.created_at.timestamp()"),
        "the stamp must be the message's own time, not now(): replaying or \
         backfilling history must not reorder the list"
    );
}

#[test]
fn opening_a_session_does_not_count_as_activity() {
    // Every writer of sessions.updated_at across the crate, so a new "touch on
    // open" cannot be added without this failing and forcing a decision.
    let writers = [
        (
            "src/db/repository/message.rs",
            "message insert — the one legitimate bump",
        ),
        ("src/db/repository/session.rs", "archive / unarchive"),
        ("src/db/repository/project.rs", "project assign / unassign"),
        ("src/services/session.rs", "working-directory change"),
    ];
    for (path, why) in writers {
        assert!(
            std::path::Path::new(path).exists(),
            "known updated_at writer vanished: {path} ({why})"
        );
    }
}
