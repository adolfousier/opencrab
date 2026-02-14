//! File Service
//!
//! Provides business logic for file tracking operations.

use crate::db::{models::File, repository::FileRepository};
use crate::services::ServiceContext;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use uuid::Uuid;

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

        let file = File {
            id: Uuid::new_v4(),
            session_id,
            path: path.clone(),
            content,
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
        // Try to find existing file
        if let Some(file) = self.find_file_by_path(session_id, &path).await? {
            return Ok(file);
        }

        // Create new file if not found
        self.track_file(session_id, path, content).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::SessionService;

    async fn create_test_service() -> (FileService, SessionService) {
        use crate::db::Database;

        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        let pool = db.pool().clone();

        let context = ServiceContext::new(pool);
        (
            FileService::new(context.clone()),
            SessionService::new(context),
        )
    }

    #[tokio::test]
    async fn test_track_file() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let file = file_service
            .track_file(
                session.id,
                PathBuf::from("/test/file.txt"),
                Some("content".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(file.session_id, session.id);
        assert_eq!(file.path, PathBuf::from("/test/file.txt"));
        assert_eq!(file.content, Some("content".to_string()));
    }

    #[tokio::test]
    async fn test_get_file() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let created = file_service
            .track_file(session.id, PathBuf::from("/test/file.txt"), None)
            .await
            .unwrap();

        let found = file_service.get_file(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_list_files_for_session() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        file_service
            .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
            .await
            .unwrap();
        file_service
            .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
            .await
            .unwrap();

        let files = file_service
            .list_files_for_session(session.id)
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn test_find_file_by_path() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let path = PathBuf::from("/test/file.txt");
        file_service
            .track_file(session.id, path.clone(), None)
            .await
            .unwrap();

        let found = file_service
            .find_file_by_path(session.id, &path)
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().path, path);
    }

    #[tokio::test]
    async fn test_update_file_content() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let file = file_service
            .track_file(session.id, PathBuf::from("/test/file.txt"), None)
            .await
            .unwrap();

        file_service
            .update_file_content(file.id, Some("new content".to_string()))
            .await
            .unwrap();

        let updated = file_service.get_file_required(file.id).await.unwrap();
        assert_eq!(updated.content, Some("new content".to_string()));
    }

    #[tokio::test]
    async fn test_delete_file() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let file = file_service
            .track_file(session.id, PathBuf::from("/test/file.txt"), None)
            .await
            .unwrap();

        file_service.delete_file(file.id).await.unwrap();

        let result = file_service.get_file(file.id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_files_for_session() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        file_service
            .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
            .await
            .unwrap();
        file_service
            .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
            .await
            .unwrap();

        file_service
            .delete_files_for_session(session.id)
            .await
            .unwrap();

        let files = file_service
            .list_files_for_session(session.id)
            .await
            .unwrap();
        assert_eq!(files.len(), 0);
    }

    #[tokio::test]
    async fn test_count_files_in_session() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        file_service
            .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
            .await
            .unwrap();
        file_service
            .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
            .await
            .unwrap();

        let count = file_service
            .count_files_in_session(session.id)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_is_file_tracked() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let path = PathBuf::from("/test/file.txt");
        file_service
            .track_file(session.id, path.clone(), None)
            .await
            .unwrap();

        let is_tracked = file_service
            .is_file_tracked(session.id, &path)
            .await
            .unwrap();
        assert!(is_tracked);

        let not_tracked = file_service
            .is_file_tracked(session.id, &PathBuf::from("/test/other.txt"))
            .await
            .unwrap();
        assert!(!not_tracked);
    }

    #[tokio::test]
    async fn test_get_or_create_file() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        let path = PathBuf::from("/test/file.txt");

        // First call should create
        let file1 = file_service
            .get_or_create_file(session.id, path.clone(), Some("content".to_string()))
            .await
            .unwrap();

        // Second call should return existing
        let file2 = file_service
            .get_or_create_file(session.id, path.clone(), None)
            .await
            .unwrap();

        assert_eq!(file1.id, file2.id);
    }

    #[tokio::test]
    async fn test_get_files_with_content() {
        let (file_service, session_service) = create_test_service().await;
        let session = session_service
            .create_session(Some("Test".to_string()))
            .await
            .unwrap();

        file_service
            .track_file(
                session.id,
                PathBuf::from("/test/file1.txt"),
                Some("content".to_string()),
            )
            .await
            .unwrap();
        file_service
            .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
            .await
            .unwrap();

        let with_content = file_service
            .get_files_with_content(session.id)
            .await
            .unwrap();
        let without_content = file_service
            .get_files_without_content(session.id)
            .await
            .unwrap();

        assert_eq!(with_content.len(), 1);
        assert_eq!(without_content.len(), 1);
    }
}
