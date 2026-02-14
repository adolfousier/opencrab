//! Service Layer
//!
//! This module contains the business logic services that orchestrate
//! operations between the database layer and the application layer.

pub mod file;
pub mod message;
pub mod plan;
pub mod session;

pub use file::FileService;
pub use message::MessageService;
pub use plan::PlanService;
pub use session::SessionService;

use crate::db::Pool;
use std::sync::Arc;

/// Service context that holds shared resources
#[derive(Clone)]
pub struct ServiceContext {
    /// Database connection pool
    pub pool: Arc<Pool>,
}

impl ServiceContext {
    /// Create a new service context
    pub fn new(pool: Pool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    /// Get a clone of the database pool (cheap operation)
    pub fn pool(&self) -> Pool {
        (*self.pool).clone()
    }
}

/// Service manager that holds all services
pub struct ServiceManager {
    context: ServiceContext,
    session_service: SessionService,
    message_service: MessageService,
    file_service: FileService,
    plan_service: PlanService,
}

impl ServiceManager {
    /// Create a new service manager
    pub fn new(pool: Pool) -> Self {
        let context = ServiceContext::new(pool);

        Self {
            session_service: SessionService::new(context.clone()),
            message_service: MessageService::new(context.clone()),
            file_service: FileService::new(context.clone()),
            plan_service: PlanService::new(context.clone()),
            context,
        }
    }

    /// Get the session service
    pub fn sessions(&self) -> &SessionService {
        &self.session_service
    }

    /// Get the message service
    pub fn messages(&self) -> &MessageService {
        &self.message_service
    }

    /// Get the file service
    pub fn files(&self) -> &FileService {
        &self.file_service
    }

    /// Get the plan service
    pub fn plans(&self) -> &PlanService {
        &self.plan_service
    }

    /// Get the service context
    pub fn context(&self) -> &ServiceContext {
        &self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Pool, PoolExt};

    async fn create_test_pool() -> Pool {
        use crate::db::Database;

        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        db.pool().clone()
    }

    #[tokio::test]
    async fn test_service_context_creation() {
        let pool = create_test_pool().await;
        let context = ServiceContext::new(pool);
        assert!(context.pool().is_connected());
    }

    #[tokio::test]
    async fn test_service_manager_creation() {
        let pool = create_test_pool().await;
        let manager = ServiceManager::new(pool);

        // Verify all services are accessible
        let _sessions = manager.sessions();
        let _messages = manager.messages();
        let _files = manager.files();
        let _context = manager.context();
    }
}
