//! File Repository
//!
//! Database operations for file tracking.

use crate::db::Pool;
use crate::db::database::interact_err;
use crate::db::models::File;
use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;
use uuid::Uuid;

/// Repository for file operations
#[derive(Clone)]
pub struct FileRepository {
    pool: Pool,
}

impl FileRepository {
    /// Create a new file repository
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Find file by ID
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<File>> {
        let id_str = id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.prepare_cached("SELECT * FROM files WHERE id = ?1")?
                    .query_row(params![id_str], File::from_row)
                    .optional()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find file")
    }

    /// Find all files for a session
    pub async fn find_by_session(&self, session_id: Uuid) -> Result<Vec<File>> {
        let sid = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT * FROM files WHERE session_id = ?1 ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(params![sid], File::from_row)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find files by session")
    }

    /// Find file by path in a session
    pub async fn find_by_path(&self, session_id: Uuid, path: &Path) -> Result<Option<File>> {
        let sid = session_id.to_string();
        let path_str = path.to_string_lossy().to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.prepare_cached("SELECT * FROM files WHERE session_id = ?1 AND path = ?2")?
                    .query_row(params![sid, path_str], File::from_row)
                    .optional()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find file by path")
    }

    /// Create a new file record
    pub async fn create(&self, file: &File) -> Result<()> {
        let f = file.clone();
        let path_str = f.path.to_string_lossy().to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO files (id, session_id, path, content, size, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        f.id.to_string(),
                        f.session_id.to_string(),
                        path_str,
                        f.content,
                        f.size,
                        f.created_at.timestamp(),
                        f.updated_at.timestamp(),
                    ],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to create file record")?;

        tracing::debug!("Created file record: {} - {:?}", file.id, file.path);
        Ok(())
    }

    /// Update an existing file record
    pub async fn update(&self, file: &File) -> Result<()> {
        let f = file.clone();
        let path_str = f.path.to_string_lossy().to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE files
                     SET path = ?1, content = ?2, size = ?3, updated_at = ?4
                     WHERE id = ?5",
                    params![
                        path_str,
                        f.content,
                        f.size,
                        f.updated_at.timestamp(),
                        f.id.to_string(),
                    ],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to update file")?;

        tracing::debug!("Updated file record: {}", file.id);
        Ok(())
    }

    /// Delete a file record
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let id_str = id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| conn.execute("DELETE FROM files WHERE id = ?1", params![id_str]))
            .await
            .map_err(interact_err)?
            .context("Failed to delete file")?;

        tracing::debug!("Deleted file record: {}", id);
        Ok(())
    }

    /// List all files for a session
    pub async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<File>> {
        self.find_by_session(session_id).await
    }

    /// Count files in a session
    pub async fn count_by_session(&self, session_id: Uuid) -> Result<i64> {
        let sid = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM files WHERE session_id = ?1",
                    params![sid],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to count files")
    }

    /// Delete all file records for a session
    pub async fn delete_by_session(&self, session_id: Uuid) -> Result<()> {
        let sid = session_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute("DELETE FROM files WHERE session_id = ?1", params![sid])
            })
            .await
            .map_err(interact_err)?
            .context("Failed to delete session files")?;

        tracing::debug!("Deleted all file records for session: {}", session_id);
        Ok(())
    }
}

/// Extension trait for rusqlite to add `.optional()` to query results
trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
