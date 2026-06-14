//! `providers.<name>.context_window` must override the default budget for the
//! native providers too (Anthropic, Gemini), not only OpenAI-compatible ones.
//!
//! `builder.context_limit()` reads `provider.configured_context_window()` and
//! falls back to `agent.context_limit` (200k). These tests pin that the native
//! providers now expose the override (CLI providers share the same wiring but
//! need their binary present, so they're not constructed here).

use crate::brain::provider::{AnthropicProvider, GeminiProvider, Provider};

#[test]
fn anthropic_honors_context_window_override() {
    let p = AnthropicProvider::new("k".to_string());
    // Default: no override, so the budget falls back to agent.context_limit.
    assert_eq!(p.configured_context_window(), None);

    let p = AnthropicProvider::new("k".to_string()).with_context_window(500_000);
    assert_eq!(p.configured_context_window(), Some(500_000));
}

#[test]
fn gemini_honors_context_window_override() {
    let p = GeminiProvider::new("k".to_string());
    assert_eq!(p.configured_context_window(), None);

    let p = GeminiProvider::new("k".to_string()).with_context_window(2_000_000);
    assert_eq!(p.configured_context_window(), Some(2_000_000));
}
