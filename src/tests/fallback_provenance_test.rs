//! Provenance labels for per-call logging (#969).
//!
//! During the v1.1 incident investigation the logs could not distinguish
//! "a fallback entry failed and the chain advanced" from "the calls came
//! from a different session": per-call logs carried no record of WHICH
//! chain entry served them. `provenance_label()` gives every streaming
//! call a one-line answer (primary vs fallback #N + name). These tests
//! pin the label's shape and that it tracks sticky promotion.

use crate::brain::provider::{
    FallbackProvider, LLMRequest, LLMResponse, Provider, ProviderError, ProviderStream,
};
use crate::tests::agent_service_mocks::MockProvider;
use async_trait::async_trait;
use std::sync::Arc;

/// Minimal mock: name + optional always-fail (retryable, so the chain
/// advances).
struct NamedMock {
    name: String,
    fails: bool,
}

impl NamedMock {
    fn new(name: &str, fails: bool) -> Self {
        Self {
            name: name.to_string(),
            fails,
        }
    }
}

#[async_trait]
impl Provider for NamedMock {
    async fn complete(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        if self.fails {
            return Err(ProviderError::RateLimitExceeded(format!(
                "{} mock failure",
                self.name
            )));
        }
        Ok(LLMResponse {
            id: format!("{}-response", self.name),
            model: request.model,
            content: vec![],
            stop_reason: None,
            usage: crate::brain::provider::TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                ..Default::default()
            },
            streaming_active_secs: None,
        })
    }

    async fn stream(
        &self,
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        if self.fails {
            return Err(ProviderError::RateLimitExceeded(format!(
                "{} stream mock failure",
                self.name
            )));
        }
        Ok(Box::pin(futures::stream::empty()))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> &str {
        "mock-default"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-default".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

fn request_for(model: &str) -> LLMRequest {
    LLMRequest {
        model: model.into(),
        messages: vec![],
        system: None,
        system_suffix: None,
        max_tokens: None,
        temperature: None,
        tools: None,
        stream: false,
        metadata: None,
        working_directory: None,
        session_id: None,
    }
}

#[test]
fn chain_label_starts_at_primary() {
    let chain = FallbackProvider::new(
        Arc::new(NamedMock::new("alpha", false)),
        vec![Arc::new(NamedMock::new("beta", false))],
    );
    assert_eq!(chain.provenance_label(), "primary 'alpha'");
}

#[test]
fn chain_label_tracks_force_next_fallback() {
    let chain = FallbackProvider::new(
        Arc::new(NamedMock::new("alpha", false)),
        vec![
            Arc::new(NamedMock::new("beta", false)),
            Arc::new(NamedMock::new("gamma", false)),
        ],
    );
    assert!(chain.force_next_fallback("test", "mock-default"));
    assert_eq!(chain.provenance_label(), "fallback #1 'beta'");
    assert!(chain.force_next_fallback("test", "mock-default"));
    assert_eq!(chain.provenance_label(), "fallback #2 'gamma'");
}

#[tokio::test]
async fn chain_label_follows_a_real_advance_after_primary_failure() {
    // Primary fails with a retryable error; the chain advances to the
    // first fallback, which succeeds. The label must then name the
    // entry that actually served the call.
    let chain = FallbackProvider::new(
        Arc::new(NamedMock::new("alpha", true)),
        vec![Arc::new(NamedMock::new("beta", false))],
    );
    assert_eq!(chain.provenance_label(), "primary 'alpha'");

    let resp = chain
        .complete(request_for("mock-default"))
        .await
        .expect("fallback should serve the call");
    assert_eq!(resp.id, "beta-response");
    assert_eq!(chain.provenance_label(), "fallback #1 'beta'");
}

#[test]
fn plain_provider_label_uses_the_trait_default() {
    // Non-chain providers have no entries to distinguish; the default
    // label just quotes the provider name.
    let plain = MockProvider;
    assert_eq!(plain.provenance_label(), "'mock'");
}
