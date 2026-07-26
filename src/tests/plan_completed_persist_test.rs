//! Reading an archived plan (#810, #813).
//!
//! These cover the archive READER only. The surfaces that once rendered from
//! it are gone: deriving "show the finished checklist" from "there is no live
//! plan" is true forever once a plan completes, so the finished card
//! resurrected on every refresh and every restart, in the TUI as an
//! undismissable panel and in Telegram as a completed plan pushed into chats
//! that had moved on.
//!
//! The reader is kept because it must stay side-effect free: the live loader
//! would archive or delete on terminal statuses, which would mutate history
//! just by reading it.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::tui::plan::TaskStatus;
use crate::utils::plan_files::latest_archived_plan_from_path;

/// A finished plan as it exists on disk. Written as JSON rather than built
/// from structs so this exercises the same parse the real loader does.
fn finished_plan_json(title: &str) -> String {
    format!(
        r#"{{"title":"{title}","description":"","status":"Active",
            "tasks":[{{"title":"Ship it","description":"The only step",
                       "task_type":"Edit","status":"Completed"}}]}}"#
    )
}

/// Write a plan into an `archive/` dir beside `json_path`, named the way
/// `archive_plan_files` names things.
fn archive_as(json_path: &std::path::Path, stamp: &str, title: &str) {
    let dir = json_path.parent().unwrap().join("archive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("plan-{stamp}.json")),
        finished_plan_json(title),
    )
    .unwrap();
}

#[test]
fn the_last_archived_plan_is_recovered() {
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("plan.json");
    archive_as(&live, "20260726-101500", "Ship the fix");

    let found = latest_archived_plan_from_path(&live).expect("archived plan must be readable");
    assert_eq!(found.title, "Ship the fix");
    assert_eq!(found.tasks[0].status, TaskStatus::Completed);
}

#[test]
fn the_newest_archive_wins() {
    // Archive names end in -%Y%m%d-%H%M%S, so lexicographic order IS
    // chronological order and the newest must be the one shown.
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("plan.json");
    archive_as(&live, "20260725-090000", "Older plan");
    archive_as(&live, "20260726-101500", "Newer plan");

    let found = latest_archived_plan_from_path(&live).unwrap();
    assert_eq!(found.title, "Newer plan");
}

#[test]
fn a_date_rollover_still_picks_the_newer_one() {
    // Guards the ordering assumption across a month boundary, where naive
    // string sorting of a different format would fail.
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("plan.json");
    archive_as(&live, "20260731-235900", "July");
    archive_as(&live, "20260801-000100", "August");

    assert_eq!(
        latest_archived_plan_from_path(&live).unwrap().title,
        "August"
    );
}

#[test]
fn no_archive_yields_nothing() {
    // A session that never completed a plan must show no panel at all.
    let tmp = tempfile::tempdir().unwrap();
    assert!(latest_archived_plan_from_path(&tmp.path().join("plan.json")).is_none());
}

#[test]
fn reading_an_archive_never_mutates_it() {
    // The live loader acts on terminal statuses: "Completed" archives the file
    // and returns None, "Cancelled" deletes it. Correct for a live plan and
    // destructive for one already archived, which would move the file deeper
    // on every read and render nothing (#809). Reading history must not
    // rewrite it.
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("plan.json");
    let dir = tmp.path().join("archive");
    std::fs::create_dir_all(&dir).unwrap();
    let archived = dir.join("plan-20260726-101500.json");
    std::fs::write(
        &archived,
        r#"{"title":"Terminal status","description":"","status":"Completed",
            "tasks":[{"title":"Ship it","description":"d","task_type":"Edit",
                      "status":"Completed"}]}"#,
    )
    .unwrap();

    let found = latest_archived_plan_from_path(&live);
    assert!(
        found.is_some(),
        "an archived plan must stay readable whatever status it carries"
    );
    assert!(
        archived.exists(),
        "reading an archived plan must not move or delete it"
    );
}

#[test]
fn a_non_json_file_in_the_archive_is_ignored() {
    // Archiving moves the .md alongside the .json; picking the .md would
    // fail to parse and lose the plan.
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("plan.json");
    archive_as(&live, "20260726-101500", "Ship the fix");
    let dir = tmp.path().join("archive");
    std::fs::write(dir.join("plan-20260726-999999.md"), "# design prose").unwrap();

    let found = latest_archived_plan_from_path(&live).expect("the .md must not shadow the .json");
    assert_eq!(found.title, "Ship the fix");
}
