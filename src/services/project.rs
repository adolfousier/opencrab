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

    /// Get the projects directory path — profile-aware: `~/.opencrabs/projects`
    /// for the default, `~/.opencrabs/profiles/<name>/projects` under `-p`.
    /// Uses `opencrabs_home()` (not the root `base_opencrabs_dir`) so a
    /// profile's project files live entirely under that profile.
    pub fn projects_dir() -> std::path::PathBuf {
        crate::config::opencrabs_home().join("projects")
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

    /// `~/.opencrabs/projects/<slug>/` (the project ROOT, not `/files`) for the
    /// session's project, plus the project name for labelling. `None` when the
    /// session has no project, the project row is gone, or the lookup fails.
    ///
    /// This is the hook point for per-project brain overlays: a project may
    /// carry its own `AGENTS.md`, `SOUL.md`, etc. that load ON TOP of the
    /// profile's brain. Read-only resolution, the directory is NOT created here
    /// (overlays are optional; absence just means "no overlay").
    pub async fn project_brain_dir(
        &self,
        session_id: Uuid,
    ) -> Option<(String, std::path::PathBuf)> {
        use crate::db::repository::SessionRepository;
        let session = SessionRepository::new(self.context.pool())
            .find_by_id(session_id)
            .await
            .ok()??;
        let project_id = session.project_id?;
        let project = ProjectRepository::new(self.context.pool())
            .find_by_id(project_id)
            .await
            .ok()??;
        let dir =
            Self::projects_dir().join(crate::services::file::slugify_project_name(&project.name));
        Some((project.name, dir))
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
