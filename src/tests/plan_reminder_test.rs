//! Tests for the pinned plan reminder (discussion #177): the active plan's
//! incomplete tasks are re-injected at the end of the prompt each turn so the
//! model doesn't forget it's mid-plan in a long conversation.

use crate::brain::agent::service::format_plan_reminder;
use crate::tui::plan::{PlanDocument, PlanStatus, PlanTask, TaskStatus, TaskType};
use uuid::Uuid;

fn plan_with(status: PlanStatus, tasks: Vec<(&str, TaskStatus)>) -> PlanDocument {
    let mut p = PlanDocument::new(Uuid::new_v4(), "Ship login flow".to_string(), String::new());
    p.status = status;
    for (i, (title, st)) in tasks.into_iter().enumerate() {
        let mut t = PlanTask::new(i, title.to_string(), String::new(), TaskType::Edit);
        t.status = st;
        p.add_task(t);
    }
    p
}

#[test]
fn executing_plan_with_incomplete_tasks_is_reminded() {
    let plan = plan_with(
        PlanStatus::InProgress,
        vec![
            ("wire auth service", TaskStatus::Completed),
            ("build login form", TaskStatus::InProgress),
            ("add tests", TaskStatus::Pending),
        ],
    );
    let out = format_plan_reminder(&plan).expect("should remind for an in-flight plan");
    assert!(out.contains("ACTIVE PLAN REMINDER"));
    assert!(out.contains("1/3 tasks done"));
    assert!(out.contains("→ In progress: build login form"));
    assert!(out.contains("☐ add tests"));
    // Completed tasks are not listed.
    assert!(!out.contains("wire auth service"));
}

#[test]
fn draft_or_pending_plan_is_not_reminded() {
    // A plan not yet approved/executing must not nag.
    let plan = plan_with(PlanStatus::Draft, vec![("x", TaskStatus::Pending)]);
    assert!(format_plan_reminder(&plan).is_none());
    let plan = plan_with(
        PlanStatus::PendingApproval,
        vec![("x", TaskStatus::Pending)],
    );
    assert!(format_plan_reminder(&plan).is_none());
}

#[test]
fn fully_resolved_plan_is_not_reminded() {
    // All tasks done or skipped → nothing left, no reminder.
    let plan = plan_with(
        PlanStatus::InProgress,
        vec![("a", TaskStatus::Completed), ("b", TaskStatus::Skipped)],
    );
    assert!(format_plan_reminder(&plan).is_none());
}

#[test]
fn approved_plan_is_reminded() {
    let plan = plan_with(
        PlanStatus::Approved,
        vec![("only task", TaskStatus::Pending)],
    );
    assert!(format_plan_reminder(&plan).is_some());
}
