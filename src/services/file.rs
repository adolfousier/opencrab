//! File Service
//!
//! Provides business logic for file tracking operations.

use crate::db::{models::File, repository::FileRepository};
use crate::services::ServiceContext;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Whether `path` lives inside a git repository — walk up its parents looking
/// for a `.git` entry. Repository code is excluded from project archiving: it's
/// already version-controlled and changes on the repo, so copying it elsewhere
/// is pointless. Everything outside a repo (shared uploads, generated artifacts)
/// is fair game to archive.
fn is_inside_git_repo(path: &Path) -> bool {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.join(".git").exists() {
            return true;
        }
        dir = d.parent();
    }
    false
}

/// True when `path` is an ephemeral share — a channel (Telegram/WhatsApp/...) or
/// clipboard/web download that landed under `~/.opencrabs/tmp/`. That directory
/// is the convention for incoming files across channels and the clipboard, and
/// it's periodically cleaned up — so such a file must be COPIED into a project
/// rather than symlinked (the source won't be there later). Anything else (a
/// real local path the user shared, or a file the agent produced) is persistent
/// and gets symlinked.
fn is_ephemeral_share(path: &Path) -> bool {
    path.starts_with(crate::config::opencrabs_home().join("tmp"))
}

/// Create a symlink `dest` -> `target`. Unix only; on other platforms it signals
/// failure so the caller falls back to copying.
#[cfg(unix)]
fn symlink_file(target: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, dest)
}

#[cfg(not(unix))]
fn symlink_file(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    // Windows symlinks require elevated privileges; let the caller copy instead.
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink not supported on this platform",
    ))
}

/// Turn a project name into a filesystem-safe directory slug
/// (lowercase alphanumerics, runs of other chars collapsed to a single dash).
pub(crate) fn slugify_project_name(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Service for managing file tracking
#[derive(Clone)]
pub struct FileService {
    context: ServiceContext,
}

impl FileService {
    /// Create a new file service
    pub fn new(context: ServiceContext) -> Self {
        Self { context }
    }

    /// Track a new file
    pub async fn track_file(
        &self,
        session_id: Uuid,
        path: PathBuf,
        content: Option<String>,
    ) -> Result<File> {
        let repo = FileRepository::new(self.context.pool());

        // Determine file size from disk if available
        let size = std::fs::metadata(&path).ok().map(|m| m.len() as i64);

        let file = File {
            id: Uuid::new_v4(),
            session_id,
            path: path.clone(),
            content,
            size,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        repo.create(&file).await.context("Failed to track file")?;

        tracing::debug!("Tracked new file: {:?} in session {}", path, session_id);
        Ok(file)
    }

    /// Get a file by ID
    pub async fn get_file(&self, id: Uuid) -> Result<Option<File>> {
        let repo = FileRepository::new(self.context.pool());
        repo.find_by_id(id).await.context("Failed to get file")
    }

    /// Get a file by ID, returning an error if not found
    pub async fn get_file_required(&self, id: Uuid) -> Result<File> {
        self.get_file(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", id))
    }

    /// List all files for a session
    pub async fn list_files_for_session(&self, session_id: Uuid) -> Result<Vec<File>> {
        let repo = FileRepository::new(self.context.pool());
        repo.find_by_session(session_id)
            .await
            .context("Failed to list files for session")
    }

    /// Find a file by path in a session
    pub async fn find_file_by_path(&self, session_id: Uuid, path: &Path) -> Result<Option<File>> {
        let repo = FileRepository::new(self.context.pool());
        repo.find_by_path(session_id, path)
            .await
            .context("Failed to find file by path")
    }

    /// Update a file
    pub async fn update_file(&self, file: &File) -> Result<()> {
        let repo = FileRepository::new(self.context.pool());

        // Update the updated_at timestamp
        let mut updated_file = file.clone();
        updated_file.updated_at = Utc::now();

        repo.update(&updated_file)
            .await
            .context("Failed to update file")?;

        tracing::debug!("Updated file: {:?}", file.path);
        Ok(())
    }

    /// Update file content
    pub async fn update_file_content(&self, id: Uuid, content: Option<String>) -> Result<()> {
        let mut file = self.get_file_required(id).await?;
        file.content = content;
        file.updated_at = Utc::now();

        let repo = FileRepository::new(self.context.pool());
        repo.update(&file)
            .await
            .context("Failed to update file content")?;

        tracing::debug!("Updated file content: {:?}", file.path);
        Ok(())
    }

    /// Delete a file
    pub async fn delete_file(&self, id: Uuid) -> Result<()> {
        let repo = FileRepository::new(self.context.pool());
        repo.delete(id).await.context("Failed to delete file")?;

        tracing::debug!("Deleted file: {}", id);
        Ok(())
    }

    /// Delete all files for a session
    pub async fn delete_files_for_session(&self, session_id: Uuid) -> Result<()> {
        let repo = FileRepository::new(self.context.pool());
        repo.delete_by_session(session_id)
            .await
            .context("Failed to delete files for session")?;

        tracing::info!("Deleted files for session {}", session_id);
        Ok(())
    }

    /// Count files in a session
    pub async fn count_files_in_session(&self, session_id: Uuid) -> Result<i64> {
        let repo = FileRepository::new(self.context.pool());
        repo.count_by_session(session_id)
            .await
            .context("Failed to count files in session")
    }

    /// Check if a file is tracked in a session
    pub async fn is_file_tracked(&self, session_id: Uuid, path: &Path) -> Result<bool> {
        let file = self.find_file_by_path(session_id, path).await?;
        Ok(file.is_some())
    }

    /// Get or create a file entry
    pub async fn get_or_create_file(
        &self,
        session_id: Uuid,
        path: PathBuf,
        content: Option<String>,
    ) -> Result<File> {
        // If the session belongs to a project, archive the file under
        // `projects/<name>/files/` so a project's artifacts live together.
        // Dedup + tracking then key off the archived path.
        let path = self.archive_into_project(session_id, path).await;

        // Try to find existing file
        if let Some(file) = self.find_file_by_path(session_id, &path).await? {
            return Ok(file);
        }

        // Create new file if not found
        self.track_file(session_id, path, content).await
    }

    /// `~/.opencrabs/projects/<slug>/files/` for the session's project, or
    /// `None` when the session has no project. Creates the directory.
    pub async fn project_files_dir(&self, session_id: Uuid) -> Option<PathBuf> {
        use crate::db::repository::{ProjectRepository, SessionRepository};
        let session = SessionRepository::new(self.context.pool())
            .find_by_id(session_id)
            .await
            .ok()??;
        let project_id = session.project_id?;
        let project = ProjectRepository::new(self.context.pool())
            .find_by_id(project_id)
            .await
            .ok()??;
        let dir = crate::services::ProjectService::projects_dir()
            .join(slugify_project_name(&project.name))
            .join("files");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// Bring `path` into the session's project files dir (best-effort). Returns
    /// the archived path on success, otherwise the original path unchanged.
    ///
    /// Archive everything that's a project ARTIFACT so a project's deliverables
    /// live together — but HOW depends on the source:
    /// - **Ephemeral share** (clipboard / WhatsApp / Telegram / web download —
    ///   anything under `~/.opencrabs/tmp/`): **copy** it in, because the source
    ///   is cleaned up and would otherwise be lost.
    /// - **Persistent local file** (a path the user dragged in, or a file the
    ///   agent produced in the working dir): **symlink** it in, so we don't
    ///   duplicate the user's file and the project entry stays in sync with the
    ///   original. Falls back to copy if symlinking isn't available (Windows) or
    ///   fails.
    ///
    /// The ONE exclusion is **repository code**: a file inside a git repo is
    /// already persistent + version-controlled, so it's tracked at its real path
    /// and never archived (copying would duplicate it and point at a stale copy).
    async fn archive_into_project(&self, session_id: Uuid, path: PathBuf) -> PathBuf {
        if is_inside_git_repo(&path) {
            return path;
        }
        let Some(dir) = self.project_files_dir(session_id).await else {
            return path;
        };
        if !path.is_file() || path.starts_with(&dir) {
            return path;
        }
        let Some(name) = path.file_name() else {
            return path;
        };
        let dest = dir.join(name);
        if dest.exists() {
            // Already archived under this name — don't clobber or re-link.
            return dest;
        }

        if is_ephemeral_share(&path) {
            // Downloaded from a channel / clipboard / web — the tmp source is
            // cleaned up, so COPY so the artifact persists in the project.
            match std::fs::copy(&path, &dest) {
                Ok(_) => {
                    tracing::info!(
                        "Copied shared file into project: {} -> {}",
                        path.display(),
                        dest.display()
                    );
                    dest
                }
                Err(e) => {
                    tracing::warn!("Failed to copy shared file into project dir: {e}");
                    path
                }
            }
        } else {
            // A persistent local file (user drag-drop or agent-produced) —
            // SYMLINK so we don't duplicate it and it stays in sync with the
            // original. Point the link at the canonical absolute path so it
            // resolves from inside the project dir.
            let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            match symlink_file(&target, &dest) {
                Ok(()) => {
                    tracing::info!(
                        "Linked local file into project: {} -> {}",
                        dest.display(),
                        target.display()
                    );
                    dest
                }
                Err(link_err) => match std::fs::copy(&path, &dest) {
                    Ok(_) => {
                        tracing::info!(
                            "Symlink unavailable ({link_err}); copied local file into project: {} -> {}",
                            path.display(),
                            dest.display()
                        );
                        dest
                    }
                    Err(e) => {
                        tracing::warn!("Failed to link or copy local file into project dir: {e}");
                        path
                    }
                },
            }
        }
    }

    /// Get files with content
    pub async fn get_files_with_content(&self, session_id: Uuid) -> Result<Vec<File>> {
        let files = self.list_files_for_session(session_id).await?;
        Ok(files.into_iter().filter(|f| f.content.is_some()).collect())
    }

    /// Get files without content
    pub async fn get_files_without_content(&self, session_id: Uuid) -> Result<Vec<File>> {
        let files = self.list_files_for_session(session_id).await?;
        Ok(files.into_iter().filter(|f| f.content.is_none()).collect())
    }
}
