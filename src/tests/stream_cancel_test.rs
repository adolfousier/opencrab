//! Regression tests for #1148: `/stop` must win over the pre-first-token
//! window. The handshake call and every stream-retry backoff must observe
//! the cancel token instead of riding out timeouts and exponential sleeps.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::brain::agent::service::helpers::cancellable_backoff;
use crate::brain::provider::{LLMRequest, LLMResponse, Provider, ProviderError, ProviderStream};
use crate::tests::agent_service_mocks::create_test_service_with_provider;

/// A provider whose `stream()` never resolves — the shape of a wedged
/// server or an endless internal retry chain. The future is only released
/// when the caller drops it, which is exactly what the handshake race in
/// `stream_complete` must do on cancellation.
pub(crate) struct HangingProvider;

#[async_trait]
impl Provider for HangingProvider {
    async fn complete(&self, _request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        Err(ProviderError::Internal(
            "HangingProvider never completes".to_string(),
        ))
    }

    async fn stream(&self, _request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        std::future::pending().await
    }

    fn name(&self) -> &str {
        "hanging"
    }

    fn default_model(&self) -> &str {
        "hanging-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["hanging-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

// ── cancellable_backoff ──────────────────────────────────────────────

#[tokio::test]
async fn backoff_completes_without_token() {
    let started = Instant::now();
    assert!(cancellable_backoff(None, Duration::from_millis(30)).await);
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn backoff_completes_when_never_cancelled() {
    let token = CancellationToken::new();
    let started = Instant::now();
    assert!(cancellable_backoff(Some(&token), Duration::from_millis(30)).await);
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn backoff_aborts_mid_sleep_on_cancel() {
    let token = CancellationToken::new();
    let canceller = tokio::spawn({
        let token = token.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            token.cancel();
        }
    });

    let dur = Duration::from_secs(60);
    let started = Instant::now();
    let completed = cancellable_backoff(Some(&token), dur).await;

    canceller.await.unwrap();
    assert!(!completed, "cancel during backoff must abort the sleep");
    assert!(
        started.elapsed() < dur / 2,
        "abort took {:?}, expected well under the {:?} sleep",
        started.elapsed(),
        dur
    );
}

#[tokio::test]
async fn backoff_aborts_instantly_when_already_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    let started = Instant::now();
    assert!(!cancellable_backoff(Some(&token), Duration::from_secs(60)).await);
    assert!(started.elapsed() < Duration::from_secs(1));
}

// ── stream_complete handshake race ───────────────────────────────────

#[tokio::test]
async fn stream_complete_returns_promptly_when_cancelled_during_handshake() {
    let (service, session_id) = create_test_service_with_provider(Arc::new(HangingProvider)).await;

    let request = LLMRequest::new("hanging-model".to_string(), vec![]);
    let token = CancellationToken::new();

    // Fire /stop 100ms into the (never-resolving) handshake.
    let canceller = tokio::spawn({
        let token = token.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token.cancel();
        }
    });

    let started = Instant::now();
    // Hard failure bound: without the race this test rides out the full
    // 60s handshake timeout instead of returning with the cancel error.
    let result = tokio::select! {
        res = service.stream_complete(session_id, request, Some(&token), None, None, None, false) => res,
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            panic!("stream_complete ignored cancellation for 10s (#1148)")
        }
    };
    canceller.await.unwrap();

    let err = result.expect_err("cancelled handshake must return an error");
    assert!(
        matches!(err, ProviderError::Internal(ref m) if m.contains("cancelled")),
        "expected the cancel error, got: {err:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "handshake cancel took {:?}, expected a prompt return",
        started.elapsed()
    );
}

#[tokio::test]
async fn stream_complete_still_succeeds_without_cancellation() {
    let (service, session_id) = create_test_service_with_provider(Arc::new(HangingProvider)).await;
    let _ = session_id;
    // No token passed at all: the no-token branch must behave exactly as
    // before #1148. A hanging provider still hangs — assert only that the
    // call does NOT return a bogus cancel error by racing nothing. Kept as
    // a compile-shape guard for the None-token arm.
    let request = LLMRequest::new("hanging-model".to_string(), vec![]);
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        service.stream_complete(Uuid::nil(), request, None, None, None, None, false),
    )
    .await;
    assert!(
        result.is_err(),
        "no-token path must be unchanged: hanging provider should hit the timeout, not return early"
    );
}
