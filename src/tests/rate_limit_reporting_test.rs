//! What a 429 tells the user, and which model the log says served it (#1254).
//!
//! Incident: a provider with an empty account answered
//! `{"code":"1113","message":"Insufficient balance or no resource package.
//! Please recharge."}` on HTTP 429, and the agent rendered it as
//!
//!     … Please recharge. (rate limited, please retry later)
//!
//! straight into a group chat, because the advice suffix was picked from the
//! status code and never consulted the quota classifier the router already
//! trusts. Routing was correct throughout: the chain advanced with zero
//! in-place retries. Only the words were wrong, and they told the user to
//! wait for something that waiting does not fix.
//!
//! One line away, the "Streaming call served" log named the requested model
//! while a fallback had remapped to its own default, so the log answered
//! "what actually ran" with a model that did not run.

use crate::brain::provider::error::rate_limit_message;
use crate::brain::provider::{
    FallbackProvider, LLMRequest, LLMResponse, Provider, ProviderError, ProviderStream, TokenUsage,
};
use async_trait::async_trait;
use std::sync::Arc;

const BALANCE_429: &str = "Insufficient balance or no resource package. Please recharge.";

#[test]
fn a_balance_429_is_never_reported_as_a_temporary_throttle() {
    let msg = rate_limit_message(BALANCE_429, None);

    assert!(msg.contains(BALANCE_429), "upstream text is kept verbatim");
    assert!(
        msg.contains("quota or balance exhausted"),
        "names the real state: {msg}"
    );
    assert!(
        !msg.contains("retry later"),
        "advice must not contradict the message it is attached to: {msg}"
    );
    assert!(
        msg.contains("/models"),
        "the user needs the action that actually works: {msg}"
    );
}

#[test]
fn a_retry_after_header_does_not_turn_a_billing_state_into_a_countdown() {
    let msg = rate_limit_message(BALANCE_429, Some(60));

    assert!(msg.contains("quota or balance exhausted"));
    assert!(
        !msg.contains("60 seconds"),
        "an empty account does not refill in 60s: {msg}"
    );
}

#[test]
fn an_unrecognised_429_keeps_its_retry_advice() {
    // The phrase list is deliberately conservative: calling an unknown
    // throttle terminal is the more expensive mistake.
    let throttled = "Too many requests, slow down";

    let with_header = rate_limit_message(throttled, Some(30));
    assert_eq!(
        with_header,
        "Too many requests, slow down (retry after 30 seconds)"
    );

    let without_header = rate_limit_message(throttled, None);
    assert_eq!(
        without_header,
        "Too many requests, slow down (rate limited, please retry later)"
    );
}

#[test]
fn every_quota_wording_the_router_acts_on_gets_the_same_advice() {
    // Whatever `should_try_next_provider` treats as terminal must read as
    // terminal to the user too, or the two disagree in public.
    for upstream in [
        "You exceeded your current quota",
        "insufficient_quota",
        "You have exceeded this month's quota for model X",
        "credit balance is insufficient",
    ] {
        let msg = rate_limit_message(upstream, Some(120));
        assert!(
            msg.contains("quota or balance exhausted"),
            "{upstream:?} routes as terminal but reads as transient: {msg}"
        );
    }
}

// --------------------------------------------------------------------------
// Served-model provenance
// --------------------------------------------------------------------------

struct ModelMock {
    name: String,
    models: Vec<String>,
    fails: bool,
}

impl ModelMock {
    fn new(name: &str, models: &[&str], fails: bool) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            models: models.iter().map(|m| m.to_string()).collect(),
            fails,
        })
    }
}

#[async_trait]
impl Provider for ModelMock {
    async fn complete(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        if self.fails {
            return Err(ProviderError::RateLimitExceeded(BALANCE_429.to_string()));
        }
        Ok(LLMResponse {
            id: format!("{}-response", self.name),
            model: request.model,
            content: vec![],
            stop_reason: None,
            usage: TokenUsage {
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
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        if self.fails {
            return Err(ProviderError::RateLimitExceeded(BALANCE_429.to_string()));
        }
        Ok(Box::pin(futures::stream::empty()))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> &str {
        self.models.first().map(String::as_str).unwrap_or("unknown")
    }

    fn supported_models(&self) -> Vec<String> {
        self.models.clone()
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

fn request(model: &str) -> LLMRequest {
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
fn a_plain_provider_serves_exactly_what_was_asked_for() {
    let solo = ModelMock::new("solo", &["a-model"], false);
    assert_eq!(solo.served_model("a-model"), "a-model");
}

#[tokio::test]
async fn after_a_swap_the_served_model_is_the_one_the_fallback_remapped_to() {
    let primary = ModelMock::new("primary", &["primary-model"], true);
    let fb = ModelMock::new("fb", &["fb-model"], false);
    let chain = FallbackProvider::new(primary, vec![fb]);

    assert_eq!(
        chain.served_model("primary-model"),
        "primary-model",
        "before any failure the primary runs the requested model"
    );

    chain
        .complete(request("primary-model"))
        .await
        .expect("the fallback answers");

    assert_eq!(
        chain.served_model("primary-model"),
        "fb-model",
        "the fallback does not carry primary-model, so it ran its own default"
    );
}

#[tokio::test]
async fn a_fallback_that_carries_the_requested_model_reports_that_model() {
    let primary = ModelMock::new("primary", &["shared-model"], true);
    let fb = ModelMock::new("fb", &["fb-default", "shared-model"], false);
    let chain = FallbackProvider::new(primary, vec![fb]);

    chain
        .complete(request("shared-model"))
        .await
        .expect("the fallback answers");

    // `served_model` answers for the ACTIVE provider, which after the swap is
    // the fallback: asked for a model it lists, it runs that model. The
    // substitute rule (#1374) applies to the request that moved the chain,
    // and the swap event pins the session to fb-default for what follows.
    assert_eq!(
        chain.served_model("shared-model"),
        "shared-model",
        "the active provider runs a model it lists; no remap for a listed model"
    );
}
