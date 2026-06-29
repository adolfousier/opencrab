use super::*;
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::session::SessionRepository;
use crate::tui::plan::{PlanTask, TaskType};
use chrono::Utc;
use tokio;
use uuid::Uuid;

/// Helper to create a test database and session
async fn setup_test_db() -> (Database, SessionRepository, PlanRepository, Session) {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");

    let session_repo = SessionRepository::new(db.pool().clone());
    let plan_repo = PlanRepository::new(db.pool().clone());

    // Create a test session (required for foreign key)
    let session = Session::new(
        Some("Test Session".to_string()),
        Some("claude-sonnet-4-5".to_string()),
        None,
    );
    session_repo
        .create(&session)
        .await
        .expect("Failed to create test session");

    (db, session_repo, plan_repo, session)
}

/// Helper to create a test plan
fn create_test_plan(session_id: Uuid) -> PlanDocument {
    let mut plan = PlanDocument::new(
        session_id,
        "Test Plan".to_string(),
        "A test plan for unit testing".to_string(),
    );

    plan.context = "Test context".to_string();
    plan.risks = vec!["Risk 1".to_string(), "Risk 2".to_string()];

    // Add some tasks
    let task1 = PlanTask {
        id: Uuid::new_v4(),
        order: 0,
        title: "Task 1".to_string(),
        description: "First task".to_string(),
        task_type: TaskType::Research,
        dependencies: vec![],
        complexity: 3,
        acceptance_criteria: vec![],
        status: TaskStatus::Pending,
        notes: None,
        completed_at: None,
        retry_count: 0,
        max_retries: 3,
        artifacts: Vec::new(),
    };

    let task2 = PlanTask {
        id: Uuid::new_v4(),
        order: 1,
        title: "Task 2".to_string(),
        description: "Second task".to_string(),
        task_type: TaskType::Edit,
        dependencies: vec![TaskDep::Id(task1.id)],
        complexity: 5,
        acceptance_criteria: vec![],
        status: TaskStatus::Pending,
        notes: Some("Some notes".to_string()),
        completed_at: None,
        retry_count: 0,
        max_retries: 3,
        artifacts: Vec::new(),
    };

    plan.add_task(task1);
    plan.add_task(task2);

    plan
}

#[tokio::test]
async fn test_plan_create() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    let plan_id = plan.id;

    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Verify plan was created
    let found = plan_repo
        .find_by_id(plan_id)
        .await
        .expect("Failed to find plan");
    assert!(found.is_some());
    let found_plan = found.unwrap();
    assert_eq!(found_plan.title, "Test Plan");
    assert_eq!(found_plan.tasks.len(), 2);
}

#[tokio::test]
async fn test_plan_find_by_id() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Find existing plan
    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find plan");
    assert!(found.is_some());

    // Find non-existent plan
    let not_found = plan_repo
        .find_by_id(Uuid::new_v4())
        .await
        .expect("Failed to query plan");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_plan_find_by_session_id() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    // Create multiple plans for the same session
    let plan1 = create_test_plan(session.id);
    let plan2 = create_test_plan(session.id);

    plan_repo
        .create(&plan1)
        .await
        .expect("Failed to create plan1");
    plan_repo
        .create(&plan2)
        .await
        .expect("Failed to create plan2");

    // Find all plans for session
    let plans = plan_repo
        .find_by_session_id(session.id)
        .await
        .expect("Failed to find plans");
    assert_eq!(plans.len(), 2);

    // Verify plans are ordered by updated_at DESC (most recent first)
    // plan2 should be first since it was created later
    assert!(plans[0].updated_at >= plans[1].updated_at);
}

#[tokio::test]
async fn test_plan_update() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let mut plan = create_test_plan(session.id);
    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Update plan
    plan.title = "Updated Plan Title".to_string();
    plan.status = PlanStatus::Approved;
    plan.approved_at = Some(Utc::now());

    // Add a new task
    let task3 = PlanTask {
        id: Uuid::new_v4(),
        order: 2,
        title: "Task 3".to_string(),
        description: "Third task".to_string(),
        task_type: TaskType::Create,
        dependencies: vec![],
        complexity: 2,
        acceptance_criteria: vec![],
        status: TaskStatus::Pending,
        notes: None,
        completed_at: None,
        retry_count: 0,
        max_retries: 3,
        artifacts: Vec::new(),
    };
    plan.add_task(task3);

    plan_repo
        .update(&plan)
        .await
        .expect("Failed to update plan");

    // Verify updates
    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find plan")
        .unwrap();
    assert_eq!(found.title, "Updated Plan Title");
    assert_eq!(found.status, PlanStatus::Approved);
    assert!(found.approved_at.is_some());
    assert_eq!(found.tasks.len(), 3);
}

#[tokio::test]
async fn test_plan_delete() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Verify plan exists
    let found = plan_repo.find_by_id(plan.id).await.expect("Failed to find");
    assert!(found.is_some());

    // Delete plan
    plan_repo
        .delete(plan.id)
        .await
        .expect("Failed to delete plan");

    // Verify plan is deleted
    let not_found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_plan_tasks_cascade_delete() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    let plan_id = plan.id;

    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Verify tasks exist
    let found = plan_repo
        .find_by_id(plan_id)
        .await
        .expect("Failed to find")
        .unwrap();
    assert_eq!(found.tasks.len(), 2);

    // Delete plan
    plan_repo.delete(plan_id).await.expect("Failed to delete");

    // Verify plan and tasks are deleted
    let not_found = plan_repo
        .find_by_id(plan_id)
        .await
        .expect("Failed to query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_plan_status_conversion() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let statuses = vec![
        PlanStatus::Draft,
        PlanStatus::PendingApproval,
        PlanStatus::Approved,
        PlanStatus::Rejected,
        PlanStatus::InProgress,
        PlanStatus::Completed,
        PlanStatus::Cancelled,
    ];

    for status in statuses {
        let mut plan = create_test_plan(session.id);
        plan.status = status.clone();

        plan_repo
            .create(&plan)
            .await
            .expect("Failed to create plan");

        let found = plan_repo
            .find_by_id(plan.id)
            .await
            .expect("Failed to find")
            .unwrap();
        assert_eq!(found.status, status);

        // Clean up for next iteration
        plan_repo.delete(plan.id).await.expect("Failed to delete");
    }
}

#[tokio::test]
async fn test_task_type_conversion() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let task_types = vec![
        TaskType::Research,
        TaskType::Edit,
        TaskType::Create,
        TaskType::Delete,
        TaskType::Test,
        TaskType::Refactor,
        TaskType::Documentation,
        TaskType::Configuration,
        TaskType::Build,
        TaskType::Other("CustomType".to_string()),
    ];

    for task_type in task_types {
        let mut plan = create_test_plan(session.id);
        plan.tasks[0].task_type = task_type.clone();

        plan_repo
            .create(&plan)
            .await
            .expect("Failed to create plan");

        let found = plan_repo
            .find_by_id(plan.id)
            .await
            .expect("Failed to find")
            .unwrap();
        assert_eq!(found.tasks[0].task_type, task_type);

        // Clean up for next iteration
        plan_repo.delete(plan.id).await.expect("Failed to delete");
    }
}

#[tokio::test]
async fn test_task_status_conversion() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let task_statuses = vec![
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Completed,
        TaskStatus::Skipped,
        TaskStatus::Failed,
        TaskStatus::Blocked("Waiting for review".to_string()),
    ];

    for task_status in task_statuses {
        let mut plan = create_test_plan(session.id);
        plan.tasks[0].status = task_status.clone();

        plan_repo
            .create(&plan)
            .await
            .expect("Failed to create plan");

        let found = plan_repo
            .find_by_id(plan.id)
            .await
            .expect("Failed to find")
            .unwrap();
        assert_eq!(found.tasks[0].status, task_status);

        // Clean up for next iteration
        plan_repo.delete(plan.id).await.expect("Failed to delete");
    }
}

#[tokio::test]
async fn test_task_dependencies_serialization() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    let task1_id = plan.tasks[0].id;

    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();

    // Task 1 should have no dependencies
    assert_eq!(found.tasks[0].dependencies.len(), 0);

    // Task 2 should depend on Task 1
    assert_eq!(found.tasks[1].dependencies.len(), 1);
    assert_eq!(found.tasks[1].dependencies[0], TaskDep::Id(task1_id));
}

#[tokio::test]
async fn test_plan_risks_serialization() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let plan = create_test_plan(session.id);
    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();

    assert_eq!(found.risks.len(), 2);
    assert_eq!(found.risks[0], "Risk 1");
    assert_eq!(found.risks[1], "Risk 2");
}

#[tokio::test]
async fn test_plan_with_no_tasks() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let mut plan = PlanDocument::new(
        session.id,
        "Empty Plan".to_string(),
        "A plan with no tasks".to_string(),
    );
    plan.risks = vec![];

    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();

    assert_eq!(found.tasks.len(), 0);
    assert_eq!(found.risks.len(), 0);
}

#[tokio::test]
async fn test_plan_update_task_status() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let mut plan = create_test_plan(session.id);
    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    // Update task statuses
    let task0_id = plan.tasks[0].id;
    let task1_id = plan.tasks[1].id;

    if let Some(task) = plan.get_task_mut(&task0_id) {
        task.status = TaskStatus::Completed;
        task.completed_at = Some(Utc::now());
    }

    if let Some(task) = plan.get_task_mut(&task1_id) {
        task.status = TaskStatus::InProgress;
    }

    plan_repo
        .update(&plan)
        .await
        .expect("Failed to update plan");

    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();

    assert_eq!(found.tasks[0].status, TaskStatus::Completed);
    assert!(found.tasks[0].completed_at.is_some());
    assert_eq!(found.tasks[1].status, TaskStatus::InProgress);
}

#[tokio::test]
async fn test_plan_with_complex_task_graph() {
    let (_db, _session_repo, plan_repo, session) = setup_test_db().await;

    let mut plan = PlanDocument::new(
        session.id,
        "Complex Plan".to_string(),
        "A plan with complex dependencies".to_string(),
    );

    // Create 5 tasks with various dependencies
    let task_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

    for (i, &task_id) in task_ids.iter().enumerate() {
        let dependencies: Vec<TaskDep> = match i {
            0 => vec![],                                                   // No dependencies
            1 => vec![TaskDep::Id(task_ids[0])],                           // Depends on task 0
            2 => vec![TaskDep::Id(task_ids[0])],                           // Depends on task 0
            3 => vec![TaskDep::Id(task_ids[1]), TaskDep::Id(task_ids[2])], // Depends on tasks 1 and 2
            4 => vec![TaskDep::Id(task_ids[3])],                           // Depends on task 3
            _ => vec![],
        };

        let task = PlanTask {
            id: task_id,
            order: i,
            title: format!("Task {}", i),
            description: format!("Description for task {}", i),
            task_type: TaskType::Research,
            dependencies,
            complexity: ((i % 5) + 1) as u8,
            acceptance_criteria: vec![],
            status: TaskStatus::Pending,
            notes: None,
            completed_at: None,
            retry_count: 0,
            max_retries: 3,
            artifacts: Vec::new(),
        };
        plan.add_task(task);
    }

    plan_repo
        .create(&plan)
        .await
        .expect("Failed to create plan");

    let found = plan_repo
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();

    assert_eq!(found.tasks.len(), 5);

    // Verify dependencies are preserved
    assert_eq!(found.tasks[0].dependencies.len(), 0);
    assert_eq!(found.tasks[1].dependencies.len(), 1);
    assert_eq!(found.tasks[2].dependencies.len(), 1);
    assert_eq!(found.tasks[3].dependencies.len(), 2);
    assert_eq!(found.tasks[4].dependencies.len(), 1);
}

#[tokio::test]
async fn test_multiple_sessions_multiple_plans() {
    let (_db, session_repo, plan_repo, session1) = setup_test_db().await;

    // Create a second session
    let session2 = Session::new(
        Some("Test Session 2".to_string()),
        Some("claude-sonnet-4-5".to_string()),
        None,
    );
    session_repo
        .create(&session2)
        .await
        .expect("Failed to create session2");

    // Create plans for both sessions
    let plan1_s1 = create_test_plan(session1.id);
    let plan2_s1 = create_test_plan(session1.id);
    let plan1_s2 = create_test_plan(session2.id);

    plan_repo
        .create(&plan1_s1)
        .await
        .expect("Failed to create plan1_s1");
    plan_repo
        .create(&plan2_s1)
        .await
        .expect("Failed to create plan2_s1");
    plan_repo
        .create(&plan1_s2)
        .await
        .expect("Failed to create plan1_s2");

    // Verify session 1 has 2 plans
    let session1_plans = plan_repo
        .find_by_session_id(session1.id)
        .await
        .expect("Failed to find session1 plans");
    assert_eq!(session1_plans.len(), 2);

    // Verify session 2 has 1 plan
    let session2_plans = plan_repo
        .find_by_session_id(session2.id)
        .await
        .expect("Failed to find session2 plans");
    assert_eq!(session2_plans.len(), 1);
}
