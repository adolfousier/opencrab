//! Compaction walks `[providers.fallback]` like every other request path
//! (#1247).
//!
//! `compact_context` called `provider.complete()` exactly once and surfaced
//! whatever came back. A session whose primary was out of credit therefore kept
//! chatting fine (the tool loop walks the chain) while `/compact` failed every
//! time on the dead primary. That is the worst possible pairing: the session
//! that most needs compacting is the one whose window is full, and a session
//! that cannot compact cannot recover.
//!
//! Also pins the shared fall-through policy: a hard quota / 402 billing error
//! MUST advance the chain. `FallbackProvider::should_try_next` used to gate on
//! `is_retryable()` alone, and `is_retryable()` deliberately returns false for
//! hard quota (#952: don't burn backoff on a wall) — so the one error class a
//! chain exists to survive was the one that aborted it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::brain::agent::service::AgentService;
use crate::brain::provider::error::should_try_next_provider;
use crate::brain::provider::{
    LLMRequest, LLMResponse, Message, Provider, ProviderError, ProviderStream, TokenUsage,
};

/// How a mock answers `complete`.
enum Behaviour {
    Ok,
    /// Hard quota / no balance — the exact shape z.ai and modelscope return.
    QuotaExhausted,
    /// Not fall-through-able: nothing downstream should be tried.
    Fatal,
}

struct CountingMock {
    name: String,
    behaviour: Behaviour,
    models: Vec<String>,
    calls: Arc<AtomicUsize>,
    /// Model string of the last request this mock received.
    last_model: Arc<std::sync::Mutex<Option<String>>>,
}

impl CountingMock {
    fn new(name: &str, behaviour: Behaviour) -> Self {
        Self {
            name: name.to_string(),
            behaviour,
            models: Vec::new(),
            calls: Arc::new(AtomicUsize::new(0)),
            last_model: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Restrict this provider's catalogue, so a request for anything else has
    /// to be remapped to `default_model()` before it is sent.
    fn with_models(mut self, models: &[&str]) -> Self {
        self.models = models.iter().map(|m| m.to_string()).collect();
        self
    }

    fn call_counter(&self) -> Arc<AtomicUsize> {
        self.calls.clone()
    }

    fn model_spy(&self) -> Arc<std::sync::Mutex<Option<String>>> {
        self.last_model.clone()
    }
}

#[async_trait]
impl Provider for CountingMock {
    async fn complete(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_model.lock().unwrap() = Some(request.model.clone());
        match self.behaviour {
            Behaviour::Ok => Ok(LLMResponse {
                id: format!("{}-response", self.name),
                model: request.model,
                content: vec![crate::brain::provider::ContentBlock::Text {
                    text: format!("summary from {}", self.name),
                }],
                stop_reason: None,
                usage: TokenUsage::default(),
                streaming_active_secs: None,
            }),
            Behaviour::QuotaExhausted => Err(ProviderError::RateLimitExceeded(
                "Insufficient balance or no resource package. Please recharge.".to_string(),
            )),
            Behaviour::Fatal => Err(ProviderError::Internal("mock fatal".to_string())),
        }
    }

    async fn stream(
        &self,
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        Ok(Box::pin(futures::stream::empty()))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> &str {
        "mock-default"
    }

    fn supported_models(&self) -> Vec<String> {
        self.models.clone()
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(200_000)
    }

    fn calculate_cost(&self, _model: &str, _input_tokens: u32, _output_tokens: u32) -> f64 {
        0.0
    }
}

fn request(model: &str) -> LLMRequest {
    LLMRequest::new(model.to_string(), vec![Message::user("compact me")])
}

/// The headline regression: primary out of credit, chain healthy, compaction
/// must succeed instead of surfacing the primary's billing error.
#[tokio::test]
async fn quota_on_primary_falls_through_to_the_chain() {
    let primary: Arc<dyn Provider> = Arc::new(CountingMock::new(
        "cfc-primary-quota",
        Behaviour::QuotaExhausted,
    ));
    let healthy = CountingMock::new("cfc-healthy", Behaviour::Ok);
    let healthy_calls = healthy.call_counter();
    let chain: Vec<Arc<dyn Provider>> = vec![Arc::new(healthy)];

    let response = AgentService::complete_compaction_request(
        &primary,
        &chain,
        request("primary-model"),
        &CancellationToken::new(),
    )
    .await
    .expect("#1247: the chain must serve compaction when the primary is dead");

    assert_eq!(response.id, "cfc-healthy-response");
    assert_eq!(
        healthy_calls.load(Ordering::SeqCst),
        1,
        "the fallback must actually be called, once"
    );
}

/// A provider that doesn't publish the requested model gets the request
/// remapped to its own default — the same invariant the chat path enforces.
#[tokio::test]
async fn fallback_model_is_remapped_when_unsupported() {
    let primary: Arc<dyn Provider> = Arc::new(CountingMock::new(
        "cfc-primary-remap",
        Behaviour::QuotaExhausted,
    ));
    let healthy =
        CountingMock::new("cfc-remap-target", Behaviour::Ok).with_models(&["mock-default"]);
    let seen = healthy.model_spy();
    let chain: Vec<Arc<dyn Provider>> = vec![Arc::new(healthy)];

    AgentService::complete_compaction_request(
        &primary,
        &chain,
        request("a-model-only-the-primary-has"),
        &CancellationToken::new(),
    )
    .await
    .expect("remapped request must succeed");

    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("mock-default"),
        "a cross-provider model must never be sent to a fallback"
    );
}

/// No chain configured: the primary's error is surfaced verbatim, not masked
/// behind a "chain exhausted" summary that would name providers that don't
/// exist.
#[tokio::test]
async fn empty_chain_surfaces_the_primary_error() {
    let primary: Arc<dyn Provider> = Arc::new(CountingMock::new(
        "cfc-primary-alone",
        Behaviour::QuotaExhausted,
    ));

    let err = AgentService::complete_compaction_request(
        &primary,
        &[],
        request("primary-model"),
        &CancellationToken::new(),
    )
    .await
    .expect_err("nothing to fall back to");

    assert!(
        err.to_string().contains("Insufficient balance"),
        "expected the raw provider error, got: {err}"
    );
}

/// An error that is not fall-through-able must not spend the chain.
#[tokio::test]
async fn fatal_error_does_not_walk_the_chain() {
    let primary: Arc<dyn Provider> =
        Arc::new(CountingMock::new("cfc-primary-fatal", Behaviour::Fatal));
    let untouched = CountingMock::new("cfc-untouched", Behaviour::Ok);
    let untouched_calls = untouched.call_counter();
    let chain: Vec<Arc<dyn Provider>> = vec![Arc::new(untouched)];

    AgentService::complete_compaction_request(
        &primary,
        &chain,
        request("primary-model"),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a fatal error stays fatal");

    assert_eq!(
        untouched_calls.load(Ordering::SeqCst),
        0,
        "a non-retryable, non-quota failure must not be retried elsewhere"
    );
}

/// Everything dead: the caller gets a ledger naming what was tried, not one
/// provider's raw failure.
#[tokio::test]
async fn exhausted_chain_reports_what_was_tried() {
    let primary: Arc<dyn Provider> = Arc::new(CountingMock::new(
        "cfc-primary-exhaust",
        Behaviour::QuotaExhausted,
    ));
    let chain: Vec<Arc<dyn Provider>> = vec![
        Arc::new(CountingMock::new("cfc-dead-one", Behaviour::QuotaExhausted)),
        Arc::new(CountingMock::new("cfc-dead-two", Behaviour::QuotaExhausted)),
    ];

    let err = AgentService::complete_compaction_request(
        &primary,
        &chain,
        request("primary-model"),
        &CancellationToken::new(),
    )
    .await
    .expect_err("every provider failed");

    let text = err.to_string();
    assert!(
        text.contains("cfc-dead-one") && text.contains("cfc-dead-two"),
        "the failure ledger must name each provider tried, got: {text}"
    );
}

/// A cancelled compaction stops immediately rather than walking the chain.
#[tokio::test]
async fn cancellation_short_circuits_the_walk() {
    let primary: Arc<dyn Provider> = Arc::new(CountingMock::new(
        "cfc-primary-cancel",
        Behaviour::QuotaExhausted,
    ));
    let untouched = CountingMock::new("cfc-cancel-target", Behaviour::Ok);
    let untouched_calls = untouched.call_counter();
    let chain: Vec<Arc<dyn Provider>> = vec![Arc::new(untouched)];

    let cancel = CancellationToken::new();
    cancel.cancel();

    AgentService::complete_compaction_request(&primary, &chain, request("m"), &cancel)
        .await
        .expect_err("cancelled");

    assert_eq!(untouched_calls.load(Ordering::SeqCst), 0);
}

/// The policy itself: hard quota and 402 must advance a chain even though they
/// are (correctly) not retryable in place.
#[test]
fn quota_and_billing_errors_advance_the_chain() {
    let monthly = ProviderError::RateLimitExceeded(
        "You have exceeded this month's quota for model X, please try again next month".to_string(),
    );
    assert!(
        monthly.is_quota_exhausted(),
        "sanity: this wording is a hard quota"
    );
    assert!(
        !monthly.is_retryable(),
        "sanity: hard quota is not retryable in place (#952)"
    );
    assert!(
        should_try_next_provider(&monthly),
        "#1247: but it MUST advance the chain"
    );

    let no_balance = ProviderError::RateLimitExceeded(
        "Insufficient balance or no resource package. Please recharge.".to_string(),
    );
    assert!(should_try_next_provider(&no_balance));

    let payment_required = ProviderError::ApiError {
        status: 402,
        message: "payment required".to_string(),
        error_type: None,
    };
    assert!(
        should_try_next_provider(&payment_required),
        "#1247: 402 billing caps are per-account, the next provider bills elsewhere"
    );
}

/// Transient and credential failures keep falling through; genuinely internal
/// errors do not.
#[test]
fn fall_through_policy_covers_transient_and_auth_but_not_internal() {
    assert!(should_try_next_provider(&ProviderError::RateLimitExceeded(
        "slow down".to_string()
    )));
    assert!(should_try_next_provider(&ProviderError::InvalidApiKey));
    assert!(should_try_next_provider(&ProviderError::ApiError {
        status: 401,
        message: "unauthorized".to_string(),
        error_type: None,
    }));
    assert!(should_try_next_provider(&ProviderError::ModelNotFound(
        "nope".to_string()
    )));
    assert!(!should_try_next_provider(&ProviderError::Internal(
        "bug".to_string()
    )));
}
