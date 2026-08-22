//! #1154 — `status_mark` glyph set: every status renders a unique mark,
//! pending is an empty checkbox, blocked is a pause, failed is ❌.

use crate::tui::plan::{TaskStatus, status_mark};

#[test]
fn status_marks_are_unique_per_status() {
    let marks = vec![
        status_mark(&TaskStatus::Pending),
        status_mark(&TaskStatus::InProgress),
        status_mark(&TaskStatus::Completed),
        status_mark(&TaskStatus::Skipped),
        status_mark(&TaskStatus::Failed),
        status_mark(&TaskStatus::Blocked("waiting on approval".into())),
    ];
    let mut unique = marks.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        6,
        "each status must own its glyph, got duplicates in {marks:?}"
    );
}

#[test]
fn pending_blocked_failed_use_the_new_glyphs() {
    assert_eq!(status_mark(&TaskStatus::Pending), '☐');
    assert_eq!(status_mark(&TaskStatus::Blocked("x".into())), '⏸');
    assert_eq!(status_mark(&TaskStatus::Failed), '❌');
}

#[test]
fn unchanged_statuses_keep_their_marks() {
    assert_eq!(status_mark(&TaskStatus::Completed), '☑');
    assert_eq!(status_mark(&TaskStatus::InProgress), '▶');
    assert_eq!(status_mark(&TaskStatus::Skipped), '⏭');
}
