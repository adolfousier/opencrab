//! Project Service
//!
//! Business logic for project management: CRUD, session assignment,
//! projects directory, and stats.

use crate::db::models::{Project, Session};
use crate::db::repository::ProjectRepository;
use crate::services::ServiceContext;
use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

/// Service for managing projects
#[derive(Clone)]
pub struct ProjectService {
    context: ServiceContext,
}

/// Stats for a project
#[derive(Debug, Clone)]
pub struct ProjectStats {
    pub session_count: i64,
    pub file_count: i64,
}

impl ProjectService {
    /// Create a new project service
    pub fn new(context: ServiceContext) -> Self {
        Self { context }
    }

    /// Get the projects directory path (~/.opencrabs/projects or profile-specific)
    pub fn projects_dir() -> std::path::PathBuf {
        let base = crate::config::profile::base_opencrabs_dir();
        base.join("projects")
    }

    /// Ensure the projects directory exists on disk
    pub fn ensure_projects_dir() -> Result<std::path::PathBuf> {
        let dir = Self::projects_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir).with_context(|| {
                format!("Failed to create projects directory: {}", dir.display())
            })?;
            tracing::info!("Created projects directory: {}", dir.display());
        }
        Ok(dir)
    }

    /// Create a new project
    pub async fn create_project(
        &self,
        name: String,
        description: Option<String>,
    ) -> Result<Project> {
        // Ensure projects dir exists
        Self::ensure_projects_dir()?;

        let repo = ProjectRepository::new(self.context.pool());

        // Check for duplicate name
        if let Some(existing) = repo.find_by_name(&name).await? {
            anyhow::bail!(
                "Project '{}' already exists (id: {})",
                existing.name,
                existing.id
            );
        }

        let project = Project::new(name, description);
        repo.create(&project).await?;
        Ok(project)
    }

    /// Get a project by ID
    pub async fn get_project(&self, id: Uuid) -> Result<Option<Project>> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.find_by_id(id).await
    }

    /// Get a project by ID, returning error if not found
    pub async fn get_project_required(&self, id: Uuid) -> Result<Project> {
        self.get_project(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", id))
    }

    /// List all projects
    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.list_all().await
    }

    /// Update a project
    pub async fn update_project(&self, project: &Project) -> Result<()> {
        let mut updated = project.clone();
        updated.updated_at = Utc::now();

        let repo = ProjectRepository::new(self.context.pool());
        repo.update(&updated).await
    }

    /// Rename a project
    pub async fn rename_project(&self, id: Uuid, new_name: String) -> Result<Project> {
        let mut project = self.get_project_required(id).await?;
        project.name = new_name;
        project.updated_at = Utc::now();

        let repo = ProjectRepository::new(self.context.pool());
        repo.update(&project).await?;
        Ok(project)
    }

    /// Delete a project (sessions are unassigned via FK ON DELETE SET NULL)
    pub async fn delete_project(&self, id: Uuid) -> Result<()> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.delete(id).await
    }

    /// Assign a session to a project
    pub async fn assign_session(&self, session_id: Uuid, project_id: Uuid) -> Result<()> {
        // Verify project exists
        self.get_project_required(project_id).await?;

        let repo = ProjectRepository::new(self.context.pool());
        repo.assign_session(session_id, project_id).await
    }

    /// Remove a session from its project
    pub async fn unassign_session(&self, session_id: Uuid) -> Result<()> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.unassign_session(session_id).await
    }

    /// Get all sessions in a project
    pub async fn get_sessions_for_project(&self, project_id: Uuid) -> Result<Vec<Session>> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.find_sessions_by_project(project_id).await
    }

    /// Get sessions not assigned to any project
    pub async fn get_unassigned_sessions(&self) -> Result<Vec<Session>> {
        let repo = ProjectRepository::new(self.context.pool());
        repo.find_unassigned_sessions().await
    }

    /// Get stats for a project (session count, file count)
    pub async fn get_project_stats(&self, project_id: Uuid) -> Result<ProjectStats> {
        let repo = ProjectRepository::new(self.context.pool());
        let session_count = repo.count_sessions(project_id).await?;
        let file_count = repo.count_files(project_id).await?;
        Ok(ProjectStats {
            session_count,
            file_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::services::SessionService;

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
}
