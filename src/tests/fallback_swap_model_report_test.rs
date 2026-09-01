//! Regression (#918): a sticky-fallback swap must announce the models that
//! were actually used, not the providers' defaults.
//!
//! The swap alert in chat and the footer disagreed. The footer reads the
//! session's resolved pair and was right; the alert was built from
//! `Provider::default_model()` on both sides. A session pinned to a
//! non-default model still ran on its own model, and a fallback that supports
//! that model receives it unremapped — so the alert named up to two models
//! that no request ever carried.

use crate::brain::provider::{
    FallbackProvider, LLMRequest, LLMResponse, Provider, ProviderError, ProviderStream,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Mock provider with a configurable model catalogue, recording the model of
/// the request it was actually handed.
struct ModelAwareMock {
    name: String,
    default_model: String,
    supported: Vec<String>,
    fails: bool,
    seen_model: Mutex<Option<String>>,
}

impl ModelAwareMock {
    fn new(name: &str, default_model: &str, supported: &[&str], fails: bool) -> Self {
        Self {
            name: name.to_string(),
            default_model: default_model.to_string(),
            supported: supported.iter().map(|s| s.to_string()).collect(),
            fails,
            seen_model: Mutex::new(None),
        }
    }

    fn seen_model(&self) -> Option<String> {
        self.seen_model.lock().expect("seen_model lock").clone()
    }

    fn record(&self, request: &LLMRequest) {
        *self.seen_model.lock().expect("seen_model lock") = Some(request.model.clone());
    }
}

#[async_trait]
impl Provider for ModelAwareMock {
    async fn complete(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        self.record(&request);
        if self.fails {
            // Retryable, so the chain advances rather than bailing out.
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
            tool_text_leak: false,
        })
    }

    async fn stream(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        self.record(&request);
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
        &self.default_model
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported.clone()
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

#[tokio::test]
async fn swap_reports_the_fallbacks_own_default_even_when_it_lists_the_pinned_model() {
    // The session is pinned to "shared-pinned" and the fallback lists it, but
    // a substitute runs its configured default_model (#1374): models[] is
    // capability, default_model is intent. The alert names what each side
    // actually ran: the pinned model on the failing primary, the fallback's
    // default on the succeeding side.
    let primary = Arc::new(ModelAwareMock::new(
        "primary",
        "primary-default",
        &["primary-default", "shared-pinned"],
        true,
    ));
    let fallback = Arc::new(ModelAwareMock::new(
        "fallback",
        "fallback-default",
        &["fallback-default", "shared-pinned"],
        false,
    ));
    let provider = FallbackProvider::new(primary, vec![fallback.clone()]);

    provider
        .complete(request_for("shared-pinned"))
        .await
        .expect("fallback should answer");

    assert_eq!(
        fallback.seen_model().as_deref(),
        Some("fallback-default"),
        "a substitute runs its own configured model, even one that lists the pinned model (#1374)"
    );

    let swap = provider
        .take_swap_event()
        .expect("promoting a fallback must record a swap event");
    assert_eq!(swap.from_name, "primary");
    assert_eq!(swap.to_name, "fallback");
    assert_eq!(
        swap.from_model, "shared-pinned",
        "the alert must name the model the failing request carried, not the primary's default"
    );
    assert_eq!(
        swap.to_model, "fallback-default",
        "the alert must name the model the succeeding request actually carried"
    );
}

#[tokio::test]
async fn swap_reports_the_remapped_model_when_the_fallback_cannot_take_it() {
    // The fallback does not support the pinned model, so the request IS
    // remapped — and the alert must name what was actually sent.
    let primary = Arc::new(ModelAwareMock::new(
        "primary",
        "primary-default",
        &["primary-default", "primary-only"],
        true,
    ));
    let fallback = Arc::new(ModelAwareMock::new(
        "fallback",
        "fallback-default",
        &["fallback-default"],
        false,
    ));
    let provider = FallbackProvider::new(primary, vec![fallback.clone()]);

    provider
        .complete(request_for("primary-only"))
        .await
        .expect("fallback should answer");

    assert_eq!(
        fallback.seen_model().as_deref(),
        Some("fallback-default"),
        "an unsupported model must be remapped to the fallback's default"
    );

    let swap = provider
        .take_swap_event()
        .expect("promoting a fallback must record a swap event");
    assert_eq!(
        swap.from_model, "primary-only",
        "the from side is the model the session was running"
    );
    assert_eq!(
        swap.to_model, "fallback-default",
        "the to side is the model actually sent after the remap"
    );
}

#[tokio::test]
async fn streaming_swap_reports_the_same_models_as_completion() {
    // The stream path builds its own request and had the same defect.
    let primary = Arc::new(ModelAwareMock::new(
        "primary",
        "primary-default",
        &["primary-default", "shared-pinned"],
        true,
    ));
    let fallback = Arc::new(ModelAwareMock::new(
        "fallback",
        "fallback-default",
        &["fallback-default", "shared-pinned"],
        false,
    ));
    let provider = FallbackProvider::new(primary, vec![fallback]);

    let stream = provider
        .stream(request_for("shared-pinned"))
        .await
        .expect("fallback should answer");
    // The swap is recorded when the fallback accepts the request; the stream's
    // contents are not what this test is about.
    drop(stream);

    let swap = provider
        .take_swap_event()
        .expect("promoting a fallback must record a swap event");
    assert_eq!(swap.from_model, "shared-pinned");
    assert_eq!(
        swap.to_model, "fallback-default",
        "the streaming chain applies the same substitute rule as completion (#1374)"
    );
}

#[tokio::test]
async fn forced_fallback_reports_the_session_model_it_was_given() {
    // No request is in flight here, so the caller passes the session's model.
    // It must be reported verbatim rather than replaced by a provider default.
    let primary = Arc::new(ModelAwareMock::new(
        "primary",
        "primary-default",
        &["primary-default"],
        false,
    ));
    let fallback = Arc::new(ModelAwareMock::new(
        "fallback",
        "fallback-default",
        &["fallback-default"],
        false,
    ));
    let provider = FallbackProvider::new(primary, vec![fallback]);

    assert!(provider.force_next_fallback("stream dropped", "session-pinned"));

    let swap = provider
        .take_swap_event()
        .expect("a forced promotion must record a swap event");
    assert_eq!(
        swap.from_model, "session-pinned",
        "the model the session was on, as the caller reported it"
    );
    assert_eq!(
        swap.to_model, "fallback-default",
        "with no request in flight, the promoted provider's default is what it will receive"
    );
}
