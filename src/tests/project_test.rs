//! Project repository and service integration tests.

use crate::db::Database;
use crate::db::models::{File, Project, Session};
use crate::db::repository::{FileRepository, ProjectRepository, SessionRepository};
use crate::services::{ProjectService, ServiceContext, SessionService};
use anyhow::Result;

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn test_pool() -> crate::db::Pool {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    db.pool().clone()
}

async fn services() -> (ProjectService, SessionService) {
    let pool = test_pool().await;
    let ctx = ServiceContext::new(pool);
    (ProjectService::new(ctx.clone()), SessionService::new(ctx))
}

// ── Repository tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn repo_create_find_delete_project() -> Result<()> {
    let pool = test_pool().await;
    let repo = ProjectRepository::new(pool.clone());

    let p = Project::new("my-project".into(), Some("desc".into()));
    repo.create(&p).await?;

    let found = repo.find_by_id(p.id).await?.expect("should exist");
    assert_eq!(found.name, "my-project");
    assert_eq!(found.description, Some("desc".into()));

    let by_name = repo.find_by_name("my-project").await?.expect("by name");
    assert_eq!(by_name.id, p.id);

    repo.delete(p.id).await?;
    assert!(repo.find_by_id(p.id).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn repo_list_all_ordering() -> Result<()> {
    let pool = test_pool().await;
    let repo = ProjectRepository::new(pool.clone());

    for i in 0..5 {
        let mut p = Project::new(format!("p{i}"), None);
        // Stagger timestamps so ordering is deterministic
        p.created_at = chrono::Utc::now() + chrono::Duration::seconds(i);
        p.updated_at = p.created_at;
        repo.create(&p).await?;
    }

    let all = repo.list_all().await?;
    assert_eq!(all.len(), 5);
    // Most recently updated first
    assert_eq!(all[0].name, "p4");
    assert_eq!(all[4].name, "p0");
    Ok(())
}

#[tokio::test]
async fn repo_assign_unassign_session() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let p = Project::new("proj".into(), None);
    prepo.create(&p).await?;

    let s = Session::new(Some("chat".into()), Some("m".into()), None);
    srepo.create(&s).await?;

    prepo.assign_session(s.id, p.id).await?;
    let sessions = prepo.find_sessions_by_project(p.id).await?;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, s.id);

    // Unassigned should be empty
    assert!(prepo.find_unassigned_sessions().await?.is_empty());

    prepo.unassign_session(s.id).await?;
    assert!(prepo.find_sessions_by_project(p.id).await?.is_empty());
    assert_eq!(prepo.find_unassigned_sessions().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn repo_delete_project_unassigns_sessions() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let p = Project::new("doomed".into(), None);
    prepo.create(&p).await?;

    let s = Session::new(Some("s".into()), Some("m".into()), None);
    srepo.create(&s).await?;
    prepo.assign_session(s.id, p.id).await?;

    prepo.delete(p.id).await?;

    // Session still exists but project_id is NULL
    let found = srepo.find_by_id(s.id).await?.expect("session gone");
    assert!(found.project_id.is_none());
    Ok(())
}

#[tokio::test]
async fn repo_count_sessions_and_files() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());
    let frepo = FileRepository::new(pool.clone());

    let p = Project::new("proj".into(), None);
    prepo.create(&p).await?;

    let s1 = Session::new(Some("s1".into()), Some("m".into()), None);
    let s2 = Session::new(Some("s2".into()), Some("m".into()), None);
    srepo.create(&s1).await?;
    srepo.create(&s2).await?;
    prepo.assign_session(s1.id, p.id).await?;
    prepo.assign_session(s2.id, p.id).await?;

    assert_eq!(prepo.count_sessions(p.id).await?, 2);

    frepo
        .create(&File::new(s1.id, "/f.rs".into(), None))
        .await?;
    frepo
        .create(&File::new(s1.id, "/g.rs".into(), None))
        .await?;
    frepo
        .create(&File::new(s2.id, "/h.rs".into(), None))
        .await?;

    assert_eq!(prepo.count_files(p.id).await?, 3);
    Ok(())
}

#[tokio::test]
async fn repo_update_project() -> Result<()> {
    let pool = test_pool().await;
    let repo = ProjectRepository::new(pool.clone());

    let mut p = Project::new("old".into(), None);
    repo.create(&p).await?;

    p.name = "new".into();
    p.description = Some("updated".into());
    repo.update(&p).await?;

    let found = repo.find_by_id(p.id).await?.expect("should exist");
    assert_eq!(found.name, "new");
    assert_eq!(found.description, Some("updated".into()));
    Ok(())
}

#[tokio::test]
async fn repo_assign_multiple_sessions_to_project() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let p = Project::new("multi".into(), None);
    prepo.create(&p).await?;

    for i in 0..10 {
        let s = Session::new(Some(format!("s{i}")), Some("m".into()), None);
        srepo.create(&s).await?;
        prepo.assign_session(s.id, p.id).await?;
    }

    assert_eq!(prepo.count_sessions(p.id).await?, 10);
    assert_eq!(prepo.find_sessions_by_project(p.id).await?.len(), 10);
    assert_eq!(prepo.find_unassigned_sessions().await?.len(), 0);
    Ok(())
}

#[tokio::test]
async fn repo_reassign_session_to_different_project() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let p1 = Project::new("p1".into(), None);
    let p2 = Project::new("p2".into(), None);
    prepo.create(&p1).await?;
    prepo.create(&p2).await?;

    let s = Session::new(Some("s".into()), Some("m".into()), None);
    srepo.create(&s).await?;

    // Assign to p1
    prepo.assign_session(s.id, p1.id).await?;
    assert_eq!(prepo.find_sessions_by_project(p1.id).await?.len(), 1);
    assert_eq!(prepo.find_sessions_by_project(p2.id).await?.len(), 0);

    // Reassign to p2
    prepo.assign_session(s.id, p2.id).await?;
    assert_eq!(prepo.find_sessions_by_project(p1.id).await?.len(), 0);
    assert_eq!(prepo.find_sessions_by_project(p2.id).await?.len(), 1);
    Ok(())
}

// ── Service tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn svc_create_and_list_projects() -> Result<()> {
    let (psvc, _) = services().await;

    psvc.create_project("alpha".into(), Some("first".into()))
        .await?;
    psvc.create_project("beta".into(), None).await?;

    let projects = psvc.list_projects().await?;
    assert_eq!(projects.len(), 2);
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
    Ok(())
}

#[tokio::test]
async fn svc_duplicate_name_rejected() {
    let (psvc, _) = services().await;

    psvc.create_project("unique".into(), None).await.unwrap();
    let result = psvc.create_project("unique".into(), None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[tokio::test]
async fn svc_assign_unassign_session() -> Result<()> {
    let (psvc, ssvc) = services().await;

    let project = psvc.create_project("proj".into(), None).await?;
    let session = ssvc.create_session(Some("chat".into())).await?;

    psvc.assign_session(session.id, project.id).await?;

    let sessions = psvc.get_sessions_for_project(project.id).await?;
    assert_eq!(sessions.len(), 1);
    assert!(psvc.get_unassigned_sessions().await?.is_empty());

    psvc.unassign_session(session.id).await?;
    assert!(psvc.get_sessions_for_project(project.id).await?.is_empty());
    assert_eq!(psvc.get_unassigned_sessions().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn svc_rename_project() -> Result<()> {
    let (psvc, _) = services().await;

    let p = psvc.create_project("old".into(), None).await?;
    let renamed = psvc.rename_project(p.id, "new".into()).await?;
    assert_eq!(renamed.name, "new");

    let found = psvc.get_project(p.id).await?.expect("should exist");
    assert_eq!(found.name, "new");
    Ok(())
}

#[tokio::test]
async fn svc_delete_project_unassigns_sessions() -> Result<()> {
    let (psvc, ssvc) = services().await;

    let p = psvc.create_project("doomed".into(), None).await?;
    let s = ssvc.create_session(Some("chat".into())).await?;
    psvc.assign_session(s.id, p.id).await?;

    psvc.delete_project(p.id).await?;

    assert!(psvc.get_project(p.id).await?.is_none());
    assert_eq!(psvc.get_unassigned_sessions().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn svc_project_stats() -> Result<()> {
    let (psvc, ssvc) = services().await;

    let p = psvc.create_project("stats".into(), None).await?;
    let s1 = ssvc.create_session(Some("s1".into())).await?;
    let s2 = ssvc.create_session(Some("s2".into())).await?;

    psvc.assign_session(s1.id, p.id).await?;
    psvc.assign_session(s2.id, p.id).await?;

    let stats = psvc.get_project_stats(p.id).await?;
    assert_eq!(stats.session_count, 2);
    assert_eq!(stats.file_count, 0);
    Ok(())
}

#[tokio::test]
async fn svc_assign_to_nonexistent_project_fails() {
    let (psvc, ssvc) = services().await;

    let session = ssvc.create_session(Some("chat".into())).await.unwrap();
    let result = psvc.assign_session(session.id, uuid::Uuid::new_v4()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn svc_get_project_required_not_found() {
    let (psvc, _) = services().await;
    let result = psvc.get_project_required(uuid::Uuid::new_v4()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn svc_update_project() -> Result<()> {
    let (psvc, _) = services().await;

    let mut p = psvc.create_project("original".into(), None).await?;
    p.name = "updated".into();
    p.description = Some("new desc".into());
    psvc.update_project(&p).await?;

    let found = psvc.get_project(p.id).await?.expect("should exist");
    assert_eq!(found.name, "updated");
    assert_eq!(found.description, Some("new desc".into()));
    Ok(())
}

#[tokio::test]
async fn svc_projects_dir_is_correct() {
    let dir = ProjectService::projects_dir();
    assert!(dir.ends_with("projects"));
    assert!(dir.to_string_lossy().contains(".opencrabs"));
}

#[tokio::test]
async fn svc_ensure_projects_dir_creates_directory() -> Result<()> {
    let dir = ProjectService::ensure_projects_dir()?;
    assert!(dir.exists());
    assert!(dir.is_dir());
    Ok(())
}

// ── Model tests ──────────────────────────────────────────────────────────────

#[test]
fn model_project_new() {
    let p = Project::new("test".into(), Some("desc".into()));
    assert_eq!(p.name, "test");
    assert_eq!(p.description, Some("desc".into()));
    assert!(p.created_at <= chrono::Utc::now());
    assert!(p.updated_at <= chrono::Utc::now());
}

#[test]
fn model_project_new_no_description() {
    let p = Project::new("test".into(), None);
    assert_eq!(p.name, "test");
    assert!(p.description.is_none());
}

// ── Edge case tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn empty_project_list_returns_empty() -> Result<()> {
    let (psvc, _) = services().await;
    let projects = psvc.list_projects().await?;
    assert!(projects.is_empty());
    Ok(())
}

#[tokio::test]
async fn project_with_no_sessions_returns_empty_list() -> Result<()> {
    let (psvc, _) = services().await;
    let p = psvc.create_project("empty".into(), None).await?;
    let sessions = psvc.get_sessions_for_project(p.id).await?;
    assert!(sessions.is_empty());
    Ok(())
}

#[tokio::test]
async fn unassigned_sessions_excludes_archived() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let s = Session::new(Some("active".into()), Some("m".into()), None);
    srepo.create(&s).await?;

    let archived = Session::new(Some("archived".into()), Some("m".into()), None);
    srepo.create(&archived).await?;
    srepo.archive(archived.id).await?;

    // Only active should show in unassigned
    let unassigned = prepo.find_unassigned_sessions().await?;
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].id, s.id);
    Ok(())
}

#[tokio::test]
async fn archived_session_not_in_project_sessions() -> Result<()> {
    let pool = test_pool().await;
    let prepo = ProjectRepository::new(pool.clone());
    let srepo = SessionRepository::new(pool.clone());

    let p = Project::new("proj".into(), None);
    prepo.create(&p).await?;

    let s = Session::new(Some("s".into()), Some("m".into()), None);
    srepo.create(&s).await?;
    prepo.assign_session(s.id, p.id).await?;

    // Archive the session
    srepo.archive(s.id).await?;

    // Should not appear in project sessions
    let sessions = prepo.find_sessions_by_project(p.id).await?;
    assert!(sessions.is_empty());
    Ok(())
}
