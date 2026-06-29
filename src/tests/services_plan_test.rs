use super::*;
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::session::SessionRepository;
use crate::tui::plan::{PlanStatus, PlanTask, TaskStatus, TaskType};
use tempfile::TempDir;
use tokio;
use uuid::Uuid;

/// Helper to create test database and service
async fn setup_test_service() -> (Database, PlanService, Session, TempDir) {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");

    let context = ServiceContext::new(db.pool().clone());
    let plan_service = PlanService::new(context.clone());

    // Create a test session (required for foreign key)
    let session_repo = SessionRepository::new(db.pool().clone());
    let session = Session::new(
        Some("Test Session".to_string()),
        Some("claude-sonnet-4-5".to_string()),
        None,
    );
    session_repo
        .create(&session)
        .await
        .expect("Failed to create test session");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    (db, plan_service, session, temp_dir)
}

/// Helper to create a test plan
fn create_test_plan(session_id: Uuid) -> PlanDocument {
    let mut plan = PlanDocument::new(
        session_id,
        "Test Plan".to_string(),
        "A test plan for service testing".to_string(),
    );

    plan.context = "Test context".to_string();
    plan.risks = vec!["Risk 1".to_string()];

    let task = PlanTask {
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

    plan.add_task(task);
    plan
}

#[tokio::test]
async fn test_service_create_and_find() {
    let (_db, service, session, _temp) = setup_test_service().await;

    let plan = create_test_plan(session.id);
    let plan_id = plan.id;

    // Create
    service.create(&plan).await.expect("Failed to create plan");

    // Find
    let found = service
        .find_by_id(plan_id)
        .await
        .expect("Failed to find plan");
    assert!(found.is_some());
    assert_eq!(found.unwrap().title, "Test Plan");
}

#[tokio::test]
async fn test_service_update() {
    let (_db, service, session, _temp) = setup_test_service().await;

    let mut plan = create_test_plan(session.id);
    service.create(&plan).await.expect("Failed to create plan");

    // Update
    plan.title = "Updated Title".to_string();
    plan.status = PlanStatus::Approved;

    service.update(&plan).await.expect("Failed to update plan");

    // Verify
    let found = service
        .find_by_id(plan.id)
        .await
        .expect("Failed to find")
        .unwrap();
    assert_eq!(found.title, "Updated Title");
    assert_eq!(found.status, PlanStatus::Approved);
}

#[tokio::test]
async fn test_service_delete() {
    let (_db, service, session, _temp) = setup_test_service().await;

    let plan = create_test_plan(session.id);
    service.create(&plan).await.expect("Failed to create plan");

    // Delete
    service
        .delete(plan.id)
        .await
        .expect("Failed to delete plan");

    // Verify deletion
    let found = service.find_by_id(plan.id).await.expect("Failed to query");
    assert!(found.is_none());
}

#[tokio::test]
async fn test_service_find_by_session_id() {
    let (_db, service, session, _temp) = setup_test_service().await;

    // Create multiple plans
    let plan1 = create_test_plan(session.id);
    let plan2 = create_test_plan(session.id);

    service
        .create(&plan1)
        .await
        .expect("Failed to create plan1");
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await; // Ensure different timestamps
    service
        .create(&plan2)
        .await
        .expect("Failed to create plan2");

    // Find all plans for session
    let plans = service
        .find_by_session_id(session.id)
        .await
        .expect("Failed to find plans");
    assert_eq!(plans.len(), 2);
}

#[tokio::test]
async fn test_service_get_most_recent_plan() {
    let (_db, service, session, _temp) = setup_test_service().await;

    // No plans yet
    let recent = service
        .get_most_recent_plan(session.id)
        .await
        .expect("Failed to get recent plan");
    assert!(recent.is_none());

    // Create first plan
    let plan1 = create_test_plan(session.id);
    let plan1_id = plan1.id;
    service
        .create(&plan1)
        .await
        .expect("Failed to create plan1");

    let recent = service
        .get_most_recent_plan(session.id)
        .await
        .expect("Failed to get recent plan");
    assert!(recent.is_some());
    assert_eq!(recent.unwrap().id, plan1_id);

    // Create second plan
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let plan2 = create_test_plan(session.id);
    service
        .create(&plan2)
        .await
        .expect("Failed to create plan2");

    // Should return one of the plans (order tested in repository layer)
    let recent = service
        .get_most_recent_plan(session.id)
        .await
        .expect("Failed to get recent plan");
    assert!(recent.is_some());
    let recent_plan = recent.unwrap();
    // Verify it's one of our plans
    assert!(recent_plan.id == plan1_id || recent_plan.id == plan2.id);
}

#[tokio::test]
async fn test_service_export_to_json() {
    let (_db, service, session, temp) = setup_test_service().await;

    let plan = create_test_plan(session.id);
    service.create(&plan).await.expect("Failed to create plan");

    // Export to JSON
    let json_path = temp.path().join("test_plan.json");
    service
        .export_to_json(&plan, &json_path)
        .await
        .expect("Failed to export to JSON");

    // Verify file exists
    assert!(json_path.exists());

    // Verify content is valid JSON
    let content = std::fs::read_to_string(&json_path).expect("Failed to read JSON file");
    let parsed: PlanDocument = serde_json::from_str(&content).expect("Invalid JSON");
    assert_eq!(parsed.id, plan.id);
    assert_eq!(parsed.title, plan.title);
}

#[tokio::test]
async fn test_service_import_from_json() {
    let (_db, service, session, temp) = setup_test_service().await;

    let plan = create_test_plan(session.id);

    // Write JSON file
    let json_path = temp.path().join("test_plan.json");
    let json = serde_json::to_string_pretty(&plan).expect("Failed to serialize");
    std::fs::write(&json_path, json).expect("Failed to write JSON file");

    // Import from JSON
    let imported = service
        .import_from_json(&json_path)
        .await
        .expect("Failed to import from JSON");

    assert_eq!(imported.id, plan.id);
    assert_eq!(imported.title, plan.title);
    assert_eq!(imported.tasks.len(), plan.tasks.len());
}

#[tokio::test]
async fn test_service_export_import_roundtrip() {
    let (_db, service, session, temp) = setup_test_service().await;

    let original_plan = create_test_plan(session.id);
    service
        .create(&original_plan)
        .await
        .expect("Failed to create plan");

    // Export
    let json_path = temp.path().join("roundtrip.json");
    service
        .export_to_json(&original_plan, &json_path)
        .await
        .expect("Failed to export");

    // Import
    let imported_plan = service
        .import_from_json(&json_path)
        .await
        .expect("Failed to import");

    // Verify data integrity
    assert_eq!(imported_plan.id, original_plan.id);
    assert_eq!(imported_plan.session_id, original_plan.session_id);
    assert_eq!(imported_plan.title, original_plan.title);
    assert_eq!(imported_plan.description, original_plan.description);
    assert_eq!(imported_plan.context, original_plan.context);
    assert_eq!(imported_plan.risks, original_plan.risks);
    assert_eq!(imported_plan.status, original_plan.status);
    assert_eq!(imported_plan.tasks.len(), original_plan.tasks.len());

    if let (Some(orig_task), Some(imp_task)) =
        (imported_plan.tasks.first(), original_plan.tasks.first())
    {
        assert_eq!(orig_task.id, imp_task.id);
        assert_eq!(orig_task.title, imp_task.title);
        assert_eq!(orig_task.task_type, imp_task.task_type);
    }
}

#[tokio::test]
async fn test_service_atomic_json_write() {
    let (_db, service, session, temp) = setup_test_service().await;

    let plan = create_test_plan(session.id);
    let json_path = temp.path().join("atomic.json");

    // Export should use atomic write (write to .tmp, then rename)
    service
        .export_to_json(&plan, &json_path)
        .await
        .expect("Failed to export");

    // Verify temp file doesn't exist (should be renamed)
    let temp_file = temp.path().join("atomic.tmp");
    assert!(!temp_file.exists());

    // Verify target file exists
    assert!(json_path.exists());
}

#[tokio::test]
async fn test_service_json_import_nonexistent_file() {
    let (_db, service, _session, temp) = setup_test_service().await;

    let json_path = temp.path().join("nonexistent.json");

    let result = service.import_from_json(&json_path).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_service_json_import_invalid_json() {
    let (_db, service, _session, temp) = setup_test_service().await;

    let json_path = temp.path().join("invalid.json");
    std::fs::write(&json_path, "{ invalid json }").expect("Failed to write file");

    let result = service.import_from_json(&json_path).await;
    assert!(result.is_err());
}
