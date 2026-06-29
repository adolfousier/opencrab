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
    let _projects = manager.projects();
    let _context = manager.context();
}
