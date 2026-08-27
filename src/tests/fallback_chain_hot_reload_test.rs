//! `[providers.fallback]` reloads on config change like the primary does
//! (#1249).
//!
//! The ConfigWatcher hot-swapped the PRIMARY provider on every config write and
//! left the fallback chain frozen at process start, because the chain was built
//! once in `AgentService::new` and stored in a plain `Vec`. Editing
//! `fallback_chain` therefore did nothing until a restart: a provider deleted
//! from the config kept being handed live traffic for as long as the process
//! ran, which reads exactly like "config hot reload is broken".
//!
//! These tests pin the two halves of the fix: the chain is swappable at
//! runtime, and rebuilding from a config with no fallback section CLEARS it —
//! removal has to be representable, or deleting a provider stays impossible.

use std::sync::Arc;

use crate::config::Config;
use crate::tests::agent_service_mocks::{
    MockProvider, MockProviderWithTools, create_test_service_with_provider,
};

#[tokio::test]
async fn reload_clears_a_chain_that_config_no_longer_declares() {
    let (mut svc, _sid) = create_test_service_with_provider(Arc::new(MockProvider)).await;
    svc.set_fallback_providers_for_test(vec![Arc::new(MockProviderWithTools::new())]);
    assert!(
        svc.has_fallback_provider(),
        "precondition: the runtime is holding a chain"
    );

    // `Config::default()` declares no `[providers.fallback]` — the state after
    // a user deletes the section (or empties `fallback_chain`) and saves.
    svc.reload_fallback_providers(&Config::default()).await;

    assert!(
        !svc.has_fallback_provider(),
        "#1249: a provider removed from config must stop receiving traffic \
         without a restart"
    );
    assert!(svc.fallback_chain_snapshot().is_empty());
}

/// The snapshot accessor is what every walk reads. It must reflect the live
/// slot, not a copy captured at construction.
#[tokio::test]
async fn snapshot_reflects_the_current_chain() {
    let (mut svc, _sid) = create_test_service_with_provider(Arc::new(MockProvider)).await;

    assert!(
        svc.fallback_chain_snapshot().is_empty(),
        "test config carries no fallbacks"
    );

    svc.set_fallback_providers_for_test(vec![Arc::new(MockProviderWithTools::new())]);
    assert_eq!(svc.fallback_chain_snapshot().len(), 1);

    svc.set_fallback_providers_for_test(vec![]);
    assert!(svc.fallback_chain_snapshot().is_empty());
}

/// Per-session swaps wrap the CURRENT chain. Before the fix this read a frozen
/// vec, so a session switched after a config edit still inherited providers the
/// user had removed.
#[tokio::test]
async fn session_swap_wraps_the_reloaded_chain() {
    let (mut svc, sid) = create_test_service_with_provider(Arc::new(MockProvider)).await;
    svc.set_fallback_providers_for_test(vec![Arc::new(MockProviderWithTools::new())]);

    svc.swap_provider_for_session(sid, Arc::new(MockProvider), "mock-model");
    assert!(
        svc.provider_for_session(sid).is_fallback_chain(),
        "precondition: a chain exists, so the swap wraps"
    );

    // User empties the chain, then switches model again.
    svc.reload_fallback_providers(&Config::default()).await;
    svc.swap_provider_for_session(sid, Arc::new(MockProvider), "mock-model");

    assert!(
        !svc.provider_for_session(sid).is_fallback_chain(),
        "#1249: with the chain removed there is nothing to wrap — the session \
         must not keep cascading into providers the config no longer lists"
    );
}
