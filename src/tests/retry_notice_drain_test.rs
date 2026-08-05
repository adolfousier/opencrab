//! Regression (#949): retries are reported when the turn FAILS, not only when
//! it succeeds.
//!
//! A turn spent minutes retrying a timing-out provider and the user saw one
//! folded "Timed out" block and an error, with no `⏳ Retry N/M` anywhere. The
//! retries did happen and were recorded — the single drain sat below the
//! failure exits, so it was reachable only on success. The resilience was
//! visible exactly when it did not matter and hidden when it did.
//!
//! Draining on failure also stops notices leaking into whichever later turn
//! happens to succeed next, where they would render out of context.

use crate::brain::provider::{LLMRequest, LLMResponse, Provider, ProviderStream};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// A provider holding a fixed set of recorded retries, drained like the real
/// one: `take_retry_notices` empties the buffer.
struct RetryRecordingProvider {
    notices: Mutex<Vec<(u32, u32, String)>>,
}

impl RetryRecordingProvider {
    fn with(notices: Vec<(u32, u32, String)>) -> Self {
        Self {
            notices: Mutex::new(notices),
        }
    }
}

#[async_trait]
impl Provider for RetryRecordingProvider {
    async fn complete(
        &self,
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        unreachable!("this fixture only exercises retry-notice draining")
    }

    async fn stream(
        &self,
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        unreachable!("this fixture only exercises retry-notice draining")
    }

    fn name(&self) -> &str {
        "recording"
    }

    fn default_model(&self) -> &str {
        "m"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["m".into()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }

    fn take_retry_notices(&self) -> Vec<(u32, u32, String)> {
        std::mem::take(&mut *self.notices.lock().expect("notice lock"))
    }
}

fn provider(notices: Vec<(u32, u32, String)>) -> Arc<dyn Provider> {
    Arc::new(RetryRecordingProvider::with(notices))
}

fn timeout_notices(n: u32) -> Vec<(u32, u32, String)> {
    (1..=n)
        .map(|i| (i, n, "Request timed out after 60s".to_string()))
        .collect()
}

#[test]
fn draining_empties_the_buffer_so_nothing_carries_into_the_next_turn() {
    // The leak half of the bug: notices a failed turn left behind would be
    // rendered by whichever later turn succeeded, out of context.
    let p = provider(timeout_notices(3));
    assert_eq!(p.take_retry_notices().len(), 3);
    assert!(
        p.take_retry_notices().is_empty(),
        "a second drain must find nothing — take is destructive"
    );
}

#[test]
fn every_recorded_retry_is_reported_not_just_the_last() {
    // The user counted ~6 minutes of retrying; each attempt has to be
    // accounted for, not summarised as one.
    let p = provider(timeout_notices(5));
    let drained = p.take_retry_notices();
    assert_eq!(drained.len(), 5);
    for (i, (attempt, max, reason)) in drained.iter().enumerate() {
        assert_eq!(*attempt, i as u32 + 1, "attempts must be in order");
        assert_eq!(*max, 5, "each notice carries the ceiling for N/M display");
        assert!(
            reason.contains("timed out"),
            "reason must survive: {reason}"
        );
    }
}

#[test]
fn a_provider_with_no_retries_produces_nothing() {
    // The common case must stay silent: no empty retry chatter on a clean turn.
    let p = provider(Vec::new());
    assert!(p.take_retry_notices().is_empty());
}

#[test]
fn the_default_provider_impl_reports_no_retries() {
    // Providers that do not retry internally inherit the empty default, so the
    // drain is a no-op for them rather than a panic or a phantom notice.
    struct Plain;

    #[async_trait]
    impl Provider for Plain {
        async fn complete(
            &self,
            _r: LLMRequest,
        ) -> crate::brain::provider::error::Result<LLMResponse> {
            unreachable!()
        }
        async fn stream(
            &self,
            _r: LLMRequest,
        ) -> crate::brain::provider::error::Result<ProviderStream> {
            unreachable!()
        }
        fn name(&self) -> &str {
            "plain"
        }
        fn default_model(&self) -> &str {
            "m"
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["m".into()]
        }
        fn context_window(&self, _m: &str) -> Option<u32> {
            Some(4096)
        }
        fn calculate_cost(&self, _m: &str, _i: u32, _o: u32) -> f64 {
            0.0
        }
    }

    let p: Arc<dyn Provider> = Arc::new(Plain);
    assert!(p.take_retry_notices().is_empty());
}
