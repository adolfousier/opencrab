//! Database Retry Logic
//!
//! Handles SQLite lock errors and retries with exponential backoff.
//!
//! ## Features
//! - Automatic retry for SQLITE_BUSY errors
//! - Exponential backoff for lock contention
//! - Configurable retry attempts
//! - Logging for debugging lock issues

use anyhow::{Context, Result};
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

/// Configuration for database retry logic
#[derive(Debug, Clone)]
pub struct DbRetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier (typically 2.0 for exponential)
    pub backoff_multiplier: f64,
}

impl Default for DbRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
        }
    }
}

impl DbRetryConfig {
    /// Create a new retry config with custom settings
    pub fn new(max_attempts: u32, initial_delay: Duration) -> Self {
        Self {
            max_attempts,
            initial_delay,
            ..Default::default()
        }
    }

    /// Create config for aggressive retry (for high contention)
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 10,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 1.5,
        }
    }

    /// Calculate delay for a given attempt
    pub(crate) fn calculate_delay(&self, attempt: u32) -> Duration {
        let base_delay = self.initial_delay.as_millis() as f64;
        let exponential = base_delay * self.backoff_multiplier.powi(attempt as i32);
        let max_delay_ms = self.max_delay.as_millis() as f64;

        let delay = exponential.min(max_delay_ms);
        Duration::from_millis(delay as u64)
    }
}

/// Check if a rusqlite error is a SQLite busy/locked error
pub(crate) fn is_database_locked(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                ..
            },
            _,
        ) | rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseLocked,
                ..
            },
            _,
        )
    )
}

/// Retry a database operation with exponential backoff
pub async fn retry_db_operation<F, Fut, T, E>(
    mut operation: F,
    config: &DbRetryConfig,
) -> std::result::Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    let mut last_error;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    tracing::info!("Database operation succeeded after {} retries", attempt);
                }
                return Ok(result);
            }
            Err(err) => {
                let error_msg = err.to_string();
                last_error = err;

                let is_locked = error_msg.to_lowercase().contains("locked")
                    || error_msg.to_lowercase().contains("busy");

                if !is_locked {
                    tracing::debug!("Database error is not retryable: {}", error_msg);
                    return Err(last_error);
                }

                if attempt >= config.max_attempts {
                    tracing::warn!(
                        "Max database retry attempts ({}) exceeded for lock error",
                        config.max_attempts
                    );
                    return Err(last_error);
                }

                let delay = config.calculate_delay(attempt);

                tracing::info!(
                    "Database locked (attempt {}/{}), retrying after {}ms",
                    attempt + 1,
                    config.max_attempts,
                    delay.as_millis()
                );

                sleep(delay).await;

                attempt += 1;
            }
        }
    }
}

/// Retry a database operation that returns anyhow::Result
pub async fn retry_db_anyhow<F, Fut, T>(operation: F, config: &DbRetryConfig) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    retry_db_operation(operation, config)
        .await
        .context("Database operation failed after retries")
}

/// Retry a database operation that returns rusqlite::Result
pub async fn retry_db_rusqlite<F, Fut, T>(
    mut operation: F,
    config: &DbRetryConfig,
) -> std::result::Result<T, rusqlite::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, rusqlite::Error>>,
{
    let mut attempt = 0;
    let mut last_error;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    tracing::info!("Database operation succeeded after {} retries", attempt);
                }
                return Ok(result);
            }
            Err(err) => {
                let is_locked = is_database_locked(&err);

                if !is_locked {
                    tracing::debug!("Database error is not retryable: {}", err);
                    return Err(err);
                }

                last_error = err;

                if attempt >= config.max_attempts {
                    tracing::warn!(
                        "Max database retry attempts ({}) exceeded for lock error",
                        config.max_attempts
                    );
                    return Err(last_error);
                }

                let delay = config.calculate_delay(attempt);

                tracing::info!(
                    "Database locked (attempt {}/{}), retrying after {}ms",
                    attempt + 1,
                    config.max_attempts,
                    delay.as_millis()
                );

                sleep(delay).await;

                attempt += 1;
            }
        }
    }
}
