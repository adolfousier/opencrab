//! Service context and service manager.

use crate::db::Pool;
use std::sync::Arc;

use super::{FileService, MessageService, ProjectService, SessionService};

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
    project_service: ProjectService,
}

impl ServiceManager {
    /// Create a new service manager
    pub fn new(pool: Pool) -> Self {
        let context = ServiceContext::new(pool);

        Self {
            session_service: SessionService::new(context.clone()),
            message_service: MessageService::new(context.clone()),
            file_service: FileService::new(context.clone()),
            project_service: ProjectService::new(context.clone()),
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

    /// Get the project service
    pub fn projects(&self) -> &ProjectService {
        &self.project_service
    }

    /// Get the service context
    pub fn context(&self) -> &ServiceContext {
        &self.context
    }
}
