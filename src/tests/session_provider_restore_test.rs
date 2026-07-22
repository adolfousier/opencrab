//! Guard tests for #704: `ensure_session_provider_restored` must restore a
//! session's saved provider before a turn, but must be a strict no-op when
//! there is nothing to restore — it must never switch a session that already
//! has a provider, has none saved, or is already on the global default.
//!
//! The "actually creates and registers the saved provider" path needs a real
//! config with a named provider and is exercised manually / by integration;
//! these lock the guards that protect an isolated session from an unwanted
//! switch (the whole point of #704).

use crate::brain::agent::service::AgentService;
use crate::brain::provider::Provider;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProvider;
use std::sync::Arc;
use uuid::Uuid;

async fn make_service() -> AgentService {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    AgentService::new_for_test(provider, context).await
}

/// No saved provider → nothing to restore; the session resolves to the global
/// default unchanged.
#[tokio::test]
async fn no_saved_provider_is_a_noop() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();
    svc.ensure_session_provider_restored(sid, None, None).await;
    assert_eq!(svc.provider_name_for_session(sid), "mock");
}

/// Saved provider equals the global default → nothing to restore.
#[tokio::test]
async fn saved_equals_global_default_is_a_noop() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();
    svc.ensure_session_provider_restored(sid, Some("mock"), Some("mock-model"))
        .await;
    assert_eq!(svc.provider_name_for_session(sid), "mock");
    assert_eq!(svc.provider_model_for_session(sid), "mock-model");
}

/// A session that already has a registered provider is never re-restored, so a
/// stale saved name can't clobber the live per-session pin.
#[tokio::test]
async fn already_registered_session_is_not_touched() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    svc.swap_provider_for_session(sid, provider, "pinned-model".to_string());

    // Even asked to restore a DIFFERENT provider, the existing entry wins:
    // ensure_* must not create providers by name when one is already registered.
    svc.ensure_session_provider_restored(sid, Some("some-other-provider"), Some("x"))
        .await;

    assert_eq!(svc.provider_name_for_session(sid), "mock");
    assert_eq!(svc.provider_model_for_session(sid), "pinned-model");
}
