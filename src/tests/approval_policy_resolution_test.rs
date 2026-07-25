//! `approval_policy` reaches the tool gate on every surface (#769).
//!
//! The policy used to be read only inside approval callbacks. A surface with
//! no callback (the one-shot `run` command) therefore never saw it, and a
//! config of `auto-always` was ignored: every approval-gated tool was denied
//! with "no approval mechanism configured", which read as missing plumbing
//! rather than a setting.

use crate::brain::agent::service::AgentService;
use crate::config::Config;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProviderWithTools;
use crate::utils::approval::policy_auto_approves;
use std::sync::Arc;

#[test]
fn both_auto_policies_grant_execution() {
    // check_approval_policy has always treated these the same; the gate must
    // agree, or auto-session works through a callback and nowhere else.
    assert!(policy_auto_approves("auto-always"));
    assert!(policy_auto_approves("auto-session"));
}

#[test]
fn gating_policies_do_not_grant_execution() {
    assert!(!policy_auto_approves("ask"));
    assert!(!policy_auto_approves("manual"));
    assert!(!policy_auto_approves(""));
}

#[test]
fn an_unknown_policy_never_grants_execution() {
    // Fail closed: a typo in config.toml must not silently auto-approve.
    assert!(!policy_auto_approves("auto"));
    assert!(!policy_auto_approves("Auto-Always"));
    assert!(!policy_auto_approves("yolo"));
}

async fn service_with_policy(policy: &str) -> AgentService {
    let db = Database::connect_in_memory().await.expect("in-memory db");
    db.run_migrations().await.expect("migrations");
    let provider = Arc::new(MockProviderWithTools::new());
    let mut config = Config::default();
    config.agent.approval_policy = policy.to_string();
    AgentService::new(provider, ServiceContext::new(db.pool().clone()), &config).await
}

#[tokio::test]
async fn auto_always_in_config_reaches_the_service_without_a_flag() {
    // The reported bug: `opencrabs run` passes no --auto-approve and installs
    // no approval callback, so the policy was the only thing that could grant
    // execution and it was never consulted.
    let service = service_with_policy("auto-always").await;
    assert!(
        service.auto_approves_tools(),
        "a configured auto-always must grant execution with no flag and no callback"
    );
}

#[tokio::test]
async fn a_gating_policy_still_withholds_the_grant() {
    let service = service_with_policy("ask").await;
    assert!(!service.auto_approves_tools());
}

#[tokio::test]
async fn an_explicit_grant_widens_a_gating_policy() {
    // `--auto-approve` on a config that does not auto-approve.
    let service = service_with_policy("ask")
        .await
        .with_auto_approve_tools(true);
    assert!(service.auto_approves_tools());
}

#[tokio::test]
async fn a_flagless_caller_does_not_revoke_the_policy() {
    // The regression that produced #769: cmd_run always calls
    // with_auto_approve_tools(auto_approve), and with the flag absent that
    // assigned false straight over a configured auto-always.
    let service = service_with_policy("auto-always")
        .await
        .with_auto_approve_tools(false);
    assert!(
        service.auto_approves_tools(),
        "an absent CLI flag means 'nothing to add', not 'revoke the policy'"
    );
}
