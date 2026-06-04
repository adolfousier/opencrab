//! Regression test for RSI fallback provider wiring.
//!
//! Context (2026-06-04): The RSI autonomous loop called
//! `create_provider_by_name` directly, bypassing the `[providers.fallback]`
//! chain that `create_provider_with_warning` applied to normal sessions.
//! When the RSI provider rate-limited, the cycle died instead of cascading
//! to the configured fallback. The fix extracts the wrap logic into
//! `wrap_with_fallback_chain` and calls it from both paths.

use crate::brain::provider::factory::wrap_with_fallback_chain;
use crate::brain::provider::Provider;
use crate::config::{Config, FallbackProviderConfig};
use std::sync::Arc;

#[tokio::test]
async fn rsi_wrap_returns_raw_when_no_fallback_configured() {
    let mut config = Config::default();
    config.providers.fallback = None;

    let primary: Arc<dyn Provider> = Arc::new(crate::tests::agent_service_mocks::MockProvider);
    let result = wrap_with_fallback_chain(&config, primary.clone()).await.unwrap();

    assert!(
        !result.is_fallback_chain(),
        "with no fallback configured, wrap_with_fallback_chain must return the raw primary"
    );
}

#[tokio::test]
async fn rsi_wrap_returns_raw_when_fallback_disabled() {
    let mut config = Config::default();
    config.providers.fallback = Some(FallbackProviderConfig {
        enabled: false,
        providers: vec!["minimax".to_string()],
        provider: None,
    });

    let primary: Arc<dyn Provider> = Arc::new(crate::tests::agent_service_mocks::MockProvider);
    let result = wrap_with_fallback_chain(&config, primary.clone()).await.unwrap();

    assert!(
        !result.is_fallback_chain(),
        "with fallback disabled, wrap_with_fallback_chain must return the raw primary"
    );
}
