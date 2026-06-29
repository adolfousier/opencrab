//! JSON-RPC 2.0 handler for A2A protocol operations.
//!
//! Dispatches JSON-RPC methods:
//! - `message/send` → create task + process message via AgentService
//! - `tasks/get`    → retrieve task by ID
//! - `tasks/cancel` → cancel a running task

mod send;
pub mod stream;
pub(crate) mod tasks;

use crate::a2a::types::*;
use crate::brain::agent::service::AgentService;
use crate::services::ServiceContext;
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

/// Dispatch a JSON-RPC request to the appropriate handler.
pub async fn dispatch(
    req: JsonRpcRequest,
    store: TaskStore,
    cancel_store: CancelStore,
    agent_service: Arc<AgentService>,
    service_context: ServiceContext,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "message/send" => {
            send::handle_send_message(
                req.id,
                req.params,
                store,
                cancel_store,
                agent_service,
                service_context,
            )
            .await
        }
        "tasks/get" => tasks::handle_get_task(req.id, req.params, store).await,
        "tasks/cancel" => {
            tasks::handle_cancel_task(
                req.id,
                req.params,
                store,
                cancel_store,
                &service_context.pool(),
            )
            .await
        }
        _ => JsonRpcResponse::error(
            req.id,
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", req.method),
        ),
    }
}
