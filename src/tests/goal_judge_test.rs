//! Tests for goal judge retry logic and max_tokens configuration.

use crate::brain::goal::judge::judge_goal;
use crate::brain::goal::types::GoalVerdict;
use crate::brain::provider::error::ProviderError;
use crate::brain::provider::{
    ContentBlock, LLMRequest, LLMResponse, Provider, ProviderStream, StopReason, TokenUsage,
};
use async_trait::async_trait;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

/// A mock provider that returns pre-configured responses, tracking calls.
/// Responses are wrapped in `Option` so they can be taken (avoids requiring
/// `Clone` on `ProviderError`).
struct MockProvider {
    responses: Mutex<Vec<Option<Result<LLMResponse, ProviderError>>>>,
    call_count: AtomicUsize,
    /// Captures every request that was handed to `complete()`.
    requests: Mutex<Vec<LLMRequest>>,
}

impl MockProvider {
    fn new(responses: Vec<Result<LLMResponse, ProviderError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Some).collect()),
            call_count: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    fn captured_requests(&self) -> Vec<LLMRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn complete(&self, request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        self.requests.lock().unwrap().push(request);
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut responses = self.responses.lock().unwrap();
        // Pop the first remaining response; if empty, return internal error
        match responses.first_mut() {
            Some(slot) => {
                let taken = slot.take();
                responses.retain(|s| s.is_some());
                match taken {
                    Some(r) => r,
                    None => Err(ProviderError::Internal(
                        "no more mock responses".to_string(),
                    )),
                }
            }
            None => Err(ProviderError::Internal(
                "no more mock responses".to_string(),
            )),
        }
    }

    async fn stream(&self, _request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        unimplemented!("judge_goal does not use streaming")
    }

    fn name(&self) -> &str {
        "mock-judge"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(128_000)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_response(text: &str, stop: StopReason) -> LLMResponse {
    LLMResponse {
        id: "test-1".to_string(),
        model: "mock-model".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        stop_reason: Some(stop),
        usage: TokenUsage::default(),
        streaming_active_secs: None,
        tool_text_leak: false,
    }
}

fn done_json(reason: &str) -> String {
    format!(r#"{{"verdict":"DONE","reason":"{}"}}"#, reason)
}

fn continue_json(reason: &str) -> String {
    format!(r#"{{"verdict":"CONTINUE","reason":"{}"}}"#, reason)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Valid DONE response on the first try: no retry, 1 call.
#[tokio::test]
async fn valid_done_response_no_retry() {
    let provider = MockProvider::new(vec![Ok(make_response(
        &done_json("goal achieved"),
        StopReason::EndTurn,
    ))]);

    let decision = judge_goal(&provider, "mock-model", "do the thing", "did the thing").await;
    assert_eq!(decision.verdict, GoalVerdict::Done);
    assert_eq!(decision.reason, "goal achieved");
    assert_eq!(provider.call_count(), 1);
}

/// Valid CONTINUE response on the first try: no retry, 1 call.
#[tokio::test]
async fn valid_continue_response_no_retry() {
    let provider = MockProvider::new(vec![Ok(make_response(
        &continue_json("still working"),
        StopReason::EndTurn,
    ))]);

    let decision = judge_goal(&provider, "mock-model", "do the thing", "partial").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert_eq!(decision.reason, "still working");
    assert_eq!(provider.call_count(), 1);
}

/// First response is empty, second is valid: retries and succeeds on attempt 2.
#[tokio::test]
async fn empty_response_retries_and_succeeds() {
    let provider = MockProvider::new(vec![
        Ok(make_response("", StopReason::EndTurn)),
        Ok(make_response(&done_json("recovered"), StopReason::EndTurn)),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Done);
    assert_eq!(decision.reason, "recovered");
    assert_eq!(provider.call_count(), 2);
}

/// Both attempts return empty: fail-open with Continue.
#[tokio::test]
async fn consecutive_empty_fails_open() {
    let provider = MockProvider::new(vec![
        Ok(make_response("", StopReason::EndTurn)),
        Ok(make_response("", StopReason::EndTurn)),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert!(decision.reason.contains("empty response"));
    assert_eq!(provider.call_count(), 2);
}

/// First response is unparseable garbage, second is valid: retries on parse error.
#[tokio::test]
async fn parse_failure_retries_and_succeeds() {
    let provider = MockProvider::new(vec![
        Ok(make_response("not json at all", StopReason::EndTurn)),
        Ok(make_response(
            &continue_json("kept going"),
            StopReason::EndTurn,
        )),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert_eq!(decision.reason, "kept going");
    assert_eq!(provider.call_count(), 2);
}

/// Both attempts return unparseable JSON: fail-open with Continue.
#[tokio::test]
async fn consecutive_parse_failures_fails_open() {
    let provider = MockProvider::new(vec![
        Ok(make_response("garbage1", StopReason::EndTurn)),
        Ok(make_response("garbage2", StopReason::EndTurn)),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert!(decision.reason.contains("judge parse error"));
    assert_eq!(provider.call_count(), 2);
}

/// First call is an API error, second succeeds: retries on error.
#[tokio::test]
async fn api_error_retries_and_succeeds() {
    let provider = MockProvider::new(vec![
        Err(ProviderError::Internal("transient".to_string())),
        Ok(make_response(&done_json("fixed"), StopReason::EndTurn)),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Done);
    assert_eq!(decision.reason, "fixed");
    assert_eq!(provider.call_count(), 2);
}

/// Both attempts are API errors: fail-open with Continue.
#[tokio::test]
async fn consecutive_api_errors_fails_open() {
    let provider = MockProvider::new(vec![
        Err(ProviderError::Internal("err1".to_string())),
        Err(ProviderError::Internal("err2".to_string())),
    ]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert!(decision.reason.contains("judge call error"));
    assert_eq!(provider.call_count(), 2);
}

/// max_tokens is set to 4096 in the request sent to the provider.
#[tokio::test]
async fn max_tokens_is_4096() {
    let provider = MockProvider::new(vec![Ok(make_response(
        &done_json("ok"),
        StopReason::EndTurn,
    ))]);

    judge_goal(&provider, "mock-model", "goal", "response").await;

    let requests = provider.captured_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].max_tokens, Some(4096));
}

/// Retry request also has max_tokens=4096.
#[tokio::test]
async fn retry_request_also_has_max_tokens_4096() {
    let provider = MockProvider::new(vec![
        Ok(make_response("", StopReason::EndTurn)),
        Ok(make_response(&done_json("ok"), StopReason::EndTurn)),
    ]);

    judge_goal(&provider, "mock-model", "goal", "response").await;

    let requests = provider.captured_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].max_tokens, Some(4096));
    assert_eq!(requests[1].max_tokens, Some(4096));
}

/// MaxTokens stop reason with valid JSON still parses correctly (no retry).
#[tokio::test]
async fn max_tokens_stop_reason_with_valid_json_no_retry() {
    let provider = MockProvider::new(vec![Ok(make_response(
        &continue_json("hit token limit"),
        StopReason::MaxTokens,
    ))]);

    let decision = judge_goal(&provider, "mock-model", "goal", "response").await;
    assert_eq!(decision.verdict, GoalVerdict::Continue);
    assert_eq!(decision.reason, "hit token limit");
    assert_eq!(provider.call_count(), 1);
}

/// Long response text is properly truncated in the prompt.
#[tokio::test]
async fn long_response_truncated() {
    let provider = MockProvider::new(vec![Ok(make_response(
        &done_json("ok"),
        StopReason::EndTurn,
    ))]);

    let long_response = "x".repeat(10_000);
    judge_goal(&provider, "mock-model", "goal", &long_response).await;

    let requests = provider.captured_requests();
    let prompt_text = match &requests[0].messages[0].content[0] {
        ContentBlock::Text { text } => text.as_str(),
        _ => panic!("expected text block"),
    };
    // The LAST_RESPONSE portion should be at most 4000 chars + framing
    assert!(
        prompt_text.len() < 10_000,
        "prompt should be truncated, got len={}",
        prompt_text.len()
    );
}
