//! Session Service
//!
//! Provides business logic for session management operations.

use crate::db::{
    models::Session,
    repository::{SessionListOptions, SessionRepository, UsageLedgerRepository},
};
use crate::services::ServiceContext;
use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

/// Service for managing sessions
#[derive(Clone)]
pub struct SessionService {
    context: ServiceContext,
}

impl SessionService {
    /// Create a new session service
    pub fn new(context: ServiceContext) -> Self {
        Self { context }
    }

    /// Access the underlying database pool
    pub fn pool(&self) -> crate::db::Pool {
        self.context.pool()
    }

    /// Create a new session
    pub async fn create_session(&self, title: Option<String>) -> Result<Session> {
        self.create_session_with_provider(title, None, None, None).await
    }

    /// Create a new session with explicit provider and model
    pub async fn create_session_with_provider(
        &self,
        title: Option<String>,
        provider_name: Option<String>,
        model: Option<String>,
        working_directory: Option<String>,
    ) -> Result<Session> {
        let repo = SessionRepository::new(self.context.pool());

        let session = Session {
            id: Uuid::new_v4(),
            title,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            model,
            provider_name,
            token_count: 0,
            total_cost: 0.0,
            working_directory,
            auto_title_attempted: false,
            project_id: None,
        };

        repo.create(&session)
            .await
            .context("Failed to create session")?;

        tracing::info!("Created new session: {}", session.id);
        Ok(session)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: Uuid) -> Result<Option<Session>> {
        let repo = SessionRepository::new(self.context.pool());
        repo.find_by_id(id).await.context("Failed to get session")
    }

    /// Get a session by ID, returning an error if not found
    pub async fn get_session_required(&self, id: Uuid) -> Result<Session> {
        self.get_session(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", id))
    }

    /// List all sessions
    pub async fn list_sessions(&self, options: SessionListOptions) -> Result<Vec<Session>> {
        let repo = SessionRepository::new(self.context.pool());
        repo.list(options).await.context("Failed to list sessions")
    }

    /// Update a session
    pub async fn update_session(&self, session: &Session) -> Result<()> {
        let repo = SessionRepository::new(self.context.pool());

        // Update the updated_at timestamp
        let mut updated_session = session.clone();
        updated_session.updated_at = Utc::now();

        repo.update(&updated_session)
            .await
            .context("Failed to update session")?;

        tracing::debug!("Updated session: {}", session.id);
        Ok(())
    }

    /// Update session title
    pub async fn update_session_title(&self, id: Uuid, title: Option<String>) -> Result<()> {
        let mut session = self.get_session_required(id).await?;
        session.title = title;
        session.updated_at = Utc::now();

        let repo = SessionRepository::new(self.context.pool());
        repo.update(&session)
            .await
            .context("Failed to update session title")?;

        tracing::info!("Updated session title: {}", id);
        Ok(())
    }

    /// Update session usage statistics and record to the cumulative usage ledger.
    /// The ledger persists even when sessions are deleted.
    pub async fn update_session_usage(&self, id: Uuid, token_count: i32, cost: f64) -> Result<()> {
        let mut session = self.get_session_required(id).await?;
        session.token_count += token_count;
        session.total_cost += cost;
        session.updated_at = Utc::now();

        let model = session.model.clone().unwrap_or_default();

        let repo = SessionRepository::new(self.context.pool());
        repo.update(&session)
            .await
            .context("Failed to update session usage")?;

        // Append to cumulative usage ledger (never deleted)
        let ledger = UsageLedgerRepository::new(self.context.pool());
        if let Err(e) = ledger
            .record(&id.to_string(), &model, token_count, cost)
            .await
        {
            tracing::warn!("Failed to record usage to ledger: {}", e);
        }

        tracing::debug!(
            "Updated session usage: {} (+{} tokens, +${:.4})",
            id,
            token_count,
            cost
        );
        Ok(())
    }

    /// Update session working directory
    pub async fn update_session_working_directory(
        &self,
        id: Uuid,
        dir: Option<String>,
    ) -> Result<()> {
        use crate::db::interact_err;
        use rusqlite::params;

        let id_str = id.to_string();
        let now = Utc::now().timestamp();
        self.context
            .pool()
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE sessions SET working_directory = ?1, updated_at = ?2 WHERE id = ?3",
                    params![dir, now, id_str],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to update session working directory")?;
        Ok(())
    }

    /// Mark that auto-title generation has been attempted for this session.
    /// Prevents re-triggering auto-title on subsequent messages.
    pub async fn mark_auto_title_attempted(&self, id: Uuid) -> Result<()> {
        use crate::db::interact_err;
        use rusqlite::params;

        let id_str = id.to_string();
        self.context
            .pool()
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE sessions SET auto_title_attempted = 1 WHERE id = ?1",
                    params![id_str],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to mark auto_title_attempted")?;
        Ok(())
    }

    /// Reset the auto-title attempt flag so the next user message can
    /// re-trigger auto-title generation. Called from the background task
    /// when the title-generation LLM call failed — without this the
    /// session would stay stuck on its default channel-generated title
    /// forever, because the attempt flag is set BEFORE the LLM call to
    /// prevent race conditions. Issue #118.
    pub async fn reset_auto_title_attempted(&self, id: Uuid) -> Result<()> {
        use crate::db::interact_err;
        use rusqlite::params;

        let id_str = id.to_string();
        self.context
            .pool()
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE sessions SET auto_title_attempted = 0 WHERE id = ?1",
                    params![id_str],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to reset auto_title_attempted")?;
        Ok(())
    }

    /// Archive a session
    pub async fn archive_session(&self, id: Uuid) -> Result<()> {
        let repo = SessionRepository::new(self.context.pool());
        repo.archive(id)
            .await
            .context("Failed to archive session")?;

        tracing::info!("Archived session: {}", id);
        Ok(())
    }

    /// Unarchive a session
    pub async fn unarchive_session(&self, id: Uuid) -> Result<()> {
        let repo = SessionRepository::new(self.context.pool());
        repo.unarchive(id)
            .await
            .context("Failed to unarchive session")?;

        tracing::info!("Unarchived session: {}", id);
        Ok(())
    }

    /// Delete a session permanently
    pub async fn delete_session(&self, id: Uuid) -> Result<()> {
        let repo = SessionRepository::new(self.context.pool());
        repo.delete(id).await.context("Failed to delete session")?;

        tracing::info!("Deleted session: {}", id);
        Ok(())
    }

    /// Find most recent non-archived session by exact title (used for persistent channel sessions).
    pub async fn find_session_by_title(&self, title: &str) -> Result<Option<Session>> {
        let repo = SessionRepository::new(self.context.pool());
        repo.find_by_title(title).await
    }

    /// Find the most recent non-archived session whose title ends with
    /// `suffix`. Channel handlers embed a stable platform id
    /// (e.g. `[chat:12345]`) as the title suffix on creation so a
    /// rename of the user-visible label still resolves to the same
    /// session row.
    pub async fn find_session_by_title_suffix(&self, suffix: &str) -> Result<Option<Session>> {
        let repo = SessionRepository::new(self.context.pool());
        repo.find_by_title_suffix(suffix).await
    }

    /// Get the most recent active session
    pub async fn get_most_recent_session(&self) -> Result<Option<Session>> {
        let repo = SessionRepository::new(self.context.pool());
        let options = SessionListOptions {
            include_archived: false,
            limit: Some(1),
            offset: 0,
            query: None,
        };

        let sessions = repo.list(options).await?;
        Ok(sessions.into_iter().next())
    }

    /// Count total sessions (excluding archived)
    pub async fn count_sessions(&self) -> Result<i64> {
        let repo = SessionRepository::new(self.context.pool());
        repo.count(false).await.context("Failed to count sessions")
    }

    /// Count archived sessions
    pub async fn count_archived_sessions(&self) -> Result<i64> {
        let repo = SessionRepository::new(self.context.pool());
        repo.count(true)
            .await
            .context("Failed to count archived sessions")
    }
}
