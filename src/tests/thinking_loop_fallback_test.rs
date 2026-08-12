//! A spent thinking-loop nudge budget must reach the fallback chain (#1021).
//!
//! `ThinkingLoopTimeout` is raised in `helpers.rs`, ABOVE the provider, after
//! the stream returned successfully — so the `FallbackProvider` wrapper sees no
//! failure and cannot cascade. The manual walk in the tool loop was the only
//! remaining path, and the error never reached it: its own arm matched first
//! and returned directly once the nudge budget ran out.
//!
//! Observed live on a provider that reasoned for 120s without emitting a tool
//! call: the nudge retries logged, then a red error, with a healthy chain
//! configured and untried.
//!
//! These pin the two halves of the routing decision. The arm ordering itself is
//! a control-flow property of `run_tool_loop_inner` and is asserted on the
//! source, since reproducing it needs a live provider that stalls on demand.

use crate::brain::provider::ProviderError;

/// The error stays retryable — the fix is about routing, not classification.
#[test]
fn a_thinking_loop_timeout_is_still_retryable() {
    assert!(
        ProviderError::ThinkingLoopTimeout(120).is_retryable(),
        "reclassifying this as fatal would skip the nudge retries that fix \
         the transient case"
    );
}

/// It is not quota exhaustion, so the circuit breaker must not trip on it.
#[test]
fn a_thinking_loop_timeout_is_not_quota_exhaustion() {
    assert!(
        !ProviderError::ThinkingLoopTimeout(120).is_quota_exhausted(),
        "marking it exhausted would blacklist a provider that is merely slow \
         to emit a tool call"
    );
}

/// The nudge arm must be gated on the budget, or it swallows the error forever.
#[test]
fn the_nudge_arm_is_gated_on_the_retry_budget() {
    let src = std::fs::read_to_string("src/brain/agent/service/tool_loop.rs")
        .expect("tool_loop.rs must be readable");
    assert!(
        src.contains("&& phantom_retries_used < MAX_PHANTOM_RETRIES"),
        "the ThinkingLoopTimeout arm must stop matching once the budget is \
         spent, or the error can never fall through to the fallback walk"
    );
}

/// And the fallback walk must accept it once it does fall through.
#[test]
fn the_fallback_walk_admits_a_thinking_loop_timeout() {
    let src = std::fs::read_to_string("src/brain/agent/service/tool_loop.rs")
        .expect("tool_loop.rs must be readable");
    let walk = src
        .find("|| matches!(&e, crate::brain::provider::ProviderError::InvalidApiKey)")
        .expect("the fallback-walk guard must still exist");
    let tail = &src[walk..walk + 600];
    assert!(
        tail.contains("ThinkingLoopTimeout"),
        "the walk's allow-list must include ThinkingLoopTimeout, or a spent \
         budget dead-ends with a healthy chain untried"
    );
}

/// The dead-end return must be gone.
#[test]
fn the_retry_cap_no_longer_returns_to_the_user() {
    let src = std::fs::read_to_string("src/brain/agent/service/tool_loop.rs")
        .expect("tool_loop.rs must be readable");
    assert!(
        !src.contains("Thinking-loop timeout retry cap reached"),
        "the cap must hand off to the fallback chain, not surface the error \
         while fallbacks remain untried"
    );
}
