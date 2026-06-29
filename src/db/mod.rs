//! Database Layer
//!
//! Provides database connection management, models, and repositories.

pub(crate) mod database;
pub mod models;
pub mod repository;
pub mod retry;

pub use database::{Database, Pool, PoolExt, db_integrity_failed, global_pool, interact_err};
pub use models::*;
pub use repository::*;
pub use retry::{DbRetryConfig, retry_db_anyhow, retry_db_operation, retry_db_rusqlite};
