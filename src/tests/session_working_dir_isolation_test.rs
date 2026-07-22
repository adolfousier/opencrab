//! Regression for #703: the working directory must be isolated PER SESSION.
//!
//! Before this, `AgentService` held one process-global `Arc<RwLock<PathBuf>>`
//! shared by every session. Two sessions running concurrently in different
//! directories contaminated each other: a `cd` in one moved the other's cwd,
//! so the Runtime Info prompt line and tool execution reported the wrong
//! directory (observed: an ff7_remotion session's cwd leaking into the
//! opencrabs session's prompt while the footer stayed correct).
//!
//! These lock the isolation invariants at the `AgentService` API level.

use crate::brain::agent::service::AgentService;
use crate::brain::provider::Provider;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProvider;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

async fn make_service() -> AgentService {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    AgentService::new_for_test(provider, context).await
}

/// Setting one session's cwd must not move another session's cwd.
#[tokio::test]
async fn sessions_have_independent_working_directories() {
    let svc = make_service().await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();

    svc.set_working_directory_for_session(a, PathBuf::from("/tmp/session-a"));
    svc.set_working_directory_for_session(b, PathBuf::from("/tmp/session-b"));

    assert_eq!(
        svc.get_working_directory_for_session(a),
        PathBuf::from("/tmp/session-a")
    );
    assert_eq!(
        svc.get_working_directory_for_session(b),
        PathBuf::from("/tmp/session-b")
    );

    // Move A again; B stays put — the core contamination guard.
    svc.set_working_directory_for_session(a, PathBuf::from("/tmp/session-a-moved"));
    assert_eq!(
        svc.get_working_directory_for_session(a),
        PathBuf::from("/tmp/session-a-moved")
    );
    assert_eq!(
        svc.get_working_directory_for_session(b),
        PathBuf::from("/tmp/session-b")
    );
}

/// A background session mutating ITS OWN handle (as a tool `cd` does) must not
/// move the global cwd — the seed for future sessions.
#[tokio::test]
async fn per_session_cd_does_not_move_global() {
    let svc = make_service().await;
    let global_before = svc.get_working_directory();
    let bg = Uuid::new_v4();

    // First touch seeds from the global, then a `cd`-style mutation of the
    // session's own handle diverges it.
    let handle = svc.working_dir_handle_for_session(bg);
    assert_eq!(*handle.read().unwrap(), global_before);
    *handle.write().unwrap() = PathBuf::from("/tmp/background-cd");

    assert_eq!(
        svc.get_working_directory_for_session(bg),
        PathBuf::from("/tmp/background-cd")
    );
    // Global is untouched — a brand-new session still seeds from it.
    assert_eq!(svc.get_working_directory(), global_before);
    let fresh = Uuid::new_v4();
    assert_eq!(svc.get_working_directory_for_session(fresh), global_before);
}

/// An untouched session falls back to the global cwd (channels / first turn).
#[tokio::test]
async fn untouched_session_falls_back_to_global() {
    let svc = make_service().await;
    svc.set_working_directory(PathBuf::from("/tmp/global-seed"));
    let fresh = Uuid::new_v4();
    assert_eq!(
        svc.get_working_directory_for_session(fresh),
        PathBuf::from("/tmp/global-seed")
    );
}
