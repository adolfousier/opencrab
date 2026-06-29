use crate::db::Database;
use crate::services::ServiceContext;
use crate::services::SessionService;
use crate::services::project::*;
use tokio;
use uuid::Uuid;

async fn create_test_services() -> (ProjectService, SessionService) {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();
    let context = ServiceContext::new(pool);
    (
        ProjectService::new(context.clone()),
        SessionService::new(context),
    )
}

#[tokio::test]
async fn test_create_and_list_projects() {
    let (project_svc, _session_svc) = create_test_services().await;

    let p1 = project_svc
        .create_project("Alpha".to_string(), Some("First".to_string()))
        .await
        .unwrap();
    let p2 = project_svc
        .create_project("Beta".to_string(), None)
        .await
        .unwrap();

    let projects = project_svc.list_projects().await.unwrap();
    assert_eq!(projects.len(), 2);

    // Verify names exist
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Alpha"));
    assert!(names.contains(&"Beta"));

    // Verify IDs
    assert_eq!(p1.name, "Alpha");
    assert_eq!(p2.name, "Beta");
}

#[tokio::test]
async fn test_duplicate_name_rejected() {
    let (project_svc, _) = create_test_services().await;

    project_svc
        .create_project("Unique".to_string(), None)
        .await
        .unwrap();

    let result = project_svc.create_project("Unique".to_string(), None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[tokio::test]
async fn test_assign_and_unassign_session() {
    let (project_svc, session_svc) = create_test_services().await;

    let project = project_svc
        .create_project("Test".to_string(), None)
        .await
        .unwrap();

    let session = session_svc
        .create_session(Some("Chat".to_string()))
        .await
        .unwrap();

    // Assign
    project_svc
        .assign_session(session.id, project.id)
        .await
        .unwrap();

    let sessions = project_svc
        .get_sessions_for_project(project.id)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session.id);

    // Unassigned should be empty
    let unassigned = project_svc.get_unassigned_sessions().await.unwrap();
    assert!(unassigned.is_empty());

    // Unassign
    project_svc.unassign_session(session.id).await.unwrap();

    let sessions = project_svc
        .get_sessions_for_project(project.id)
        .await
        .unwrap();
    assert!(sessions.is_empty());

    let unassigned = project_svc.get_unassigned_sessions().await.unwrap();
    assert_eq!(unassigned.len(), 1);
}

#[tokio::test]
async fn test_rename_project() {
    let (project_svc, _) = create_test_services().await;

    let project = project_svc
        .create_project("Old Name".to_string(), None)
        .await
        .unwrap();

    let renamed = project_svc
        .rename_project(project.id, "New Name".to_string())
        .await
        .unwrap();

    assert_eq!(renamed.name, "New Name");

    let found = project_svc.get_project(project.id).await.unwrap().unwrap();
    assert_eq!(found.name, "New Name");
}

#[tokio::test]
async fn test_delete_project() {
    let (project_svc, session_svc) = create_test_services().await;

    let project = project_svc
        .create_project("Doomed".to_string(), None)
        .await
        .unwrap();

    let session = session_svc
        .create_session(Some("Chat".to_string()))
        .await
        .unwrap();

    project_svc
        .assign_session(session.id, project.id)
        .await
        .unwrap();

    // Delete project
    project_svc.delete_project(project.id).await.unwrap();

    // Project should be gone
    assert!(project_svc.get_project(project.id).await.unwrap().is_none());

    // Session should still exist but unassigned
    let unassigned = project_svc.get_unassigned_sessions().await.unwrap();
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].id, session.id);
}

#[tokio::test]
async fn test_project_stats() {
    let (project_svc, session_svc) = create_test_services().await;

    let project = project_svc
        .create_project("Stats Test".to_string(), None)
        .await
        .unwrap();

    let s1 = session_svc
        .create_session(Some("S1".to_string()))
        .await
        .unwrap();
    let s2 = session_svc
        .create_session(Some("S2".to_string()))
        .await
        .unwrap();

    project_svc.assign_session(s1.id, project.id).await.unwrap();
    project_svc.assign_session(s2.id, project.id).await.unwrap();

    let stats = project_svc.get_project_stats(project.id).await.unwrap();
    assert_eq!(stats.session_count, 2);
    assert_eq!(stats.file_count, 0);
}

#[tokio::test]
async fn test_assign_to_nonexistent_project_fails() {
    let (project_svc, session_svc) = create_test_services().await;

    let session = session_svc
        .create_session(Some("Chat".to_string()))
        .await
        .unwrap();

    let fake_id = Uuid::new_v4();
    let result = project_svc.assign_session(session.id, fake_id).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}
