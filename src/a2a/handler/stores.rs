//! Shared in-memory task and cancellation stores handed to every handler.

use crate::a2a::types::Task;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// In-memory task store.
pub type TaskStore = Arc<RwLock<HashMap<String, Task>>>;

/// Cancellation token store — keyed by task ID.
pub type CancelStore = Arc<RwLock<HashMap<String, CancellationToken>>>;

/// Create a new empty task store.
pub fn new_task_store() -> TaskStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Create a new empty cancel store.
pub fn new_cancel_store() -> CancelStore {
    Arc::new(RwLock::new(HashMap::new()))
}
