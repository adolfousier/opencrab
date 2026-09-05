//! Tool ownership must follow the **active** chain entry (#1100).
//!
//! `cli_handles_tools()` answers one question: does the provider execute
//! its own tool calls? An agentic CLI does; an API provider does not, so
//! OpenCrabs' tool registry runs them. `FallbackProvider` used to answer
//! for the configured primary no matter which entry was actually serving,
//! and a quota 429 that rotated an API primary onto `claude-cli` therefore
//! kept answering `false`. The CLI ran each command inside its subprocess
//! and OpenCrabs ran the identical command again: two commits, two
//! `gh issue create`s, two `mod` declarations, one `sed -i` applied twice
//! into code that no longer compiled.
//!
//! The mirror direction is equally broken and is pinned here too: a CLI
//! primary that fails over to an API provider kept answering `true`, so
//! nobody executed the tools at all.

use crate::brain::agent::service::tool_loop::refresh_cli_flags;
use crate::brain::provider::{
    FallbackProvider, LLMRequest, LLMResponse, Provider, ProviderError, ProviderStream,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Mock that declares whether it owns tool execution and its own context,
/// and can be made to fail with a retryable error so the chain advances.
struct OwnershipMock {
    name: String,
    handles_tools: bool,
    manages_context: bool,
    fails: bool,
}

impl OwnershipMock {
    /// An agentic CLI: runs its own tools, persists its own session.
    fn cli(name: &str, fails: bool) -> Self {
        Self {
            name: name.to_string(),
            handles_tools: true,
            manages_context: true,
            fails,
        }
    }

    /// A plain HTTP API provider: OpenCrabs owns tools and compaction.
    fn api(name: &str, fails: bool) -> Self {
        Self {
            name: name.to_string(),
            handles_tools: false,
            manages_context: false,
            fails,
        }
    }
}

#[async_trait]
impl Provider for OwnershipMock {
    async fn complete(
        &self,
        request: LLMRequest,
    ) -> crate::brain::provider::error::Result<LLMResponse> {
        if self.fails {
            // A transient 429, deliberately NOT worded as a hard quota:
            // `is_quota_exhausted_message` matches phrases like "quota
            // exhausted" / "monthly limit" and makes the error
            // non-retryable, which stops `should_try_next` advancing the
            // chain at all. Ownership is what this file pins, so keep the
            // error in the plain-throttle class the chain does walk.
            return Err(ProviderError::RateLimitExceeded(format!(
                "{} throttled, retry shortly",
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
        _request: LLMRequest,
    ) -> crate::brain::provider::error::Result<ProviderStream> {
        if self.fails {
            // A transient 429, deliberately NOT worded as a hard quota:
            // `is_quota_exhausted_message` matches phrases like "quota
            // exhausted" / "monthly limit" and makes the error
            // non-retryable, which stops `should_try_next` advancing the
            // chain at all. Ownership is what this file pins, so keep the
            // error in the plain-throttle class the chain does walk.
            return Err(ProviderError::RateLimitExceeded(format!(
                "{} throttled, retry shortly",
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

    fn cli_handles_tools(&self) -> bool {
        self.handles_tools
    }

    fn cli_manages_context(&self) -> bool {
        self.manages_context
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
fn chain_on_primary_reports_the_primary() {
    // No swap yet: the API primary owns tool execution, as before.
    let chain = FallbackProvider::new(
        Arc::new(OwnershipMock::api("zhipu", false)),
        vec![Arc::new(OwnershipMock::cli("claude-cli", false))],
    );
    assert!(!chain.cli_handles_tools());
    assert!(!chain.cli_manages_context());
}

#[test]
fn swapping_onto_a_cli_hands_tool_execution_to_the_cli() {
    // The exact incident shape: API primary, CLI fallback. Once the chain
    // is on the CLI, OpenCrabs must NOT execute the tool calls it emits.
    let chain = FallbackProvider::new(
        Arc::new(OwnershipMock::api("zhipu", false)),
        vec![Arc::new(OwnershipMock::cli("claude-cli", false))],
    );
    assert!(!chain.cli_handles_tools());

    assert!(chain.force_next_fallback("throttled", "mock-default"));

    assert!(
        chain.cli_handles_tools(),
        "after rotating onto an agentic CLI the chain must report that the \
         CLI executes its own tools, or every call runs twice"
    );
    assert!(chain.cli_manages_context());
}

#[test]
fn swapping_off_a_cli_hands_tool_execution_back_to_opencrabs() {
    // Mirror direction: a CLI primary that falls back to an API provider
    // kept reporting `true`, so neither side executed the tools.
    let chain = FallbackProvider::new(
        Arc::new(OwnershipMock::cli("claude-cli", false)),
        vec![Arc::new(OwnershipMock::api("zhipu", false))],
    );
    assert!(chain.cli_handles_tools());

    assert!(chain.force_next_fallback("cli unavailable", "mock-default"));

    assert!(
        !chain.cli_handles_tools(),
        "after rotating onto an API provider OpenCrabs owns tool execution"
    );
    assert!(!chain.cli_manages_context());
}

#[tokio::test]
async fn a_real_failover_moves_ownership_not_just_a_forced_swap() {
    // Same assertion, but driven by a genuine retryable failure rather
    // than force_next_fallback, so the sticky-promotion path is covered.
    let chain = FallbackProvider::new(
        Arc::new(OwnershipMock::api("zhipu", true)),
        vec![Arc::new(OwnershipMock::cli("claude-cli", false))],
    );
    assert!(!chain.cli_handles_tools());

    let resp = chain
        .complete(request_for("mock-default"))
        .await
        .expect("the CLI fallback should serve the call");
    assert_eq!(resp.id, "claude-cli-response");

    assert!(
        chain.cli_handles_tools(),
        "the entry that served the call owns its tool execution"
    );
}

#[test]
fn refresh_cli_flags_adopts_the_post_swap_owner() {
    // The tool loop caches both flags before streaming. When a swap fires
    // mid-turn it must re-read them BEFORE deciding whether to execute the
    // emitted tool calls, or the swapping turn itself duplicates every one.
    let chain: Arc<dyn Provider> = Arc::new(FallbackProvider::new(
        Arc::new(OwnershipMock::api("zhipu", false)),
        vec![Arc::new(OwnershipMock::cli("claude-cli", false))],
    ));

    let mut is_cli_provider = chain.cli_handles_tools();
    let mut cli_owns_context = chain.cli_manages_context();
    assert!(!is_cli_provider);
    assert!(!cli_owns_context);

    chain.force_next_fallback("throttled", "mock-default");
    refresh_cli_flags(&chain, &mut is_cli_provider, &mut cli_owns_context);

    assert!(is_cli_provider);
    assert!(cli_owns_context);
}

#[test]
fn refresh_cli_flags_is_a_noop_without_a_swap() {
    // No rotation, no change: a plain provider's ownership never moves.
    let chain: Arc<dyn Provider> = Arc::new(FallbackProvider::new(
        Arc::new(OwnershipMock::api("zhipu", false)),
        vec![Arc::new(OwnershipMock::api("openrouter", false))],
    ));

    let mut is_cli_provider = false;
    let mut cli_owns_context = false;
    refresh_cli_flags(&chain, &mut is_cli_provider, &mut cli_owns_context);

    assert!(!is_cli_provider);
    assert!(!cli_owns_context);
}
