//! Tests for issue #219: active skill tracking and body re-injection after compaction.
//!
//! Verifies that `register_active_skill` / `active_skills_for_session` work
//! correctly on `AgentService`, including cleanup on session removal.

use crate::brain::agent::service::AgentService;
use crate::brain::provider::Provider;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProvider;
use std::sync::Arc;
use uuid::Uuid;

/// Helper: create an in-memory AgentService for testing.
async fn make_service() -> AgentService {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();
    let context = ServiceContext::new(pool);
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    AgentService::new_for_test(provider, context).await
}

/// Registering a skill makes it visible via `active_skills_for_session`.
#[tokio::test]
async fn register_and_query_active_skill() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();

    svc.register_active_skill(sid, "security-audit");

    let skills = svc.active_skills_for_session(sid);
    assert!(skills.contains("security-audit"));
    assert_eq!(skills.len(), 1);
}

/// Multiple skills for the same session are all tracked.
#[tokio::test]
async fn multiple_skills_per_session() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();

    svc.register_active_skill(sid, "security-audit");
    svc.register_active_skill(sid, "cost-estimate");
    svc.register_active_skill(sid, "repo-audit");

    let skills = svc.active_skills_for_session(sid);
    assert_eq!(skills.len(), 3);
    assert!(skills.contains("security-audit"));
    assert!(skills.contains("cost-estimate"));
    assert!(skills.contains("repo-audit"));
}

/// Different sessions track independent skill sets.
#[tokio::test]
async fn skills_are_session_scoped() {
    let svc = make_service().await;
    let sid1 = Uuid::new_v4();
    let sid2 = Uuid::new_v4();

    svc.register_active_skill(sid1, "security-audit");
    svc.register_active_skill(sid2, "cost-estimate");

    let skills1 = svc.active_skills_for_session(sid1);
    let skills2 = svc.active_skills_for_session(sid2);

    assert!(skills1.contains("security-audit"));
    assert!(!skills1.contains("cost-estimate"));

    assert!(skills2.contains("cost-estimate"));
    assert!(!skills2.contains("security-audit"));
}

/// Duplicate registrations are idempotent (HashSet semantics).
#[tokio::test]
async fn duplicate_skill_registration_is_idempotent() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();

    svc.register_active_skill(sid, "security-audit");
    svc.register_active_skill(sid, "security-audit");
    svc.register_active_skill(sid, "security-audit");

    let skills = svc.active_skills_for_session(sid);
    assert_eq!(skills.len(), 1);
}

/// Session with no active skills returns an empty set.
#[tokio::test]
async fn unknown_session_returns_empty() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();

    let skills = svc.active_skills_for_session(sid);
    assert!(skills.is_empty());
}

/// Active skills are cleaned up when `remove_session_provider` is called.
#[tokio::test]
async fn cleanup_on_session_removal() {
    let svc = make_service().await;
    let sid = Uuid::new_v4();

    // Register a skill.
    svc.register_active_skill(sid, "security-audit");
    assert_eq!(svc.active_skills_for_session(sid).len(), 1);

    // Remove the session (the method exists for provider cleanup but also
    // clears our active_skills map).
    svc.remove_session_provider(sid);

    // After removal the skill set should be empty.
    let skills = svc.active_skills_for_session(sid);
    assert!(
        skills.is_empty(),
        "active skills should be cleared after session removal, got: {:?}",
        skills
    );
}
