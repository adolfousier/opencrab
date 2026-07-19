//! Live-eval provider resolution (live-L0, #629).
//!
//! Turns the `agent.eval_providers` name chain into real [`Provider`] instances
//! for live evaluations. Each judge uses that provider's own configured
//! `default_model` ("set at their levels"). Unknown or unconfigured names are
//! skipped with a logged warning rather than dropped silently, and duplicates
//! are removed while preserving order.

use std::collections::HashSet;
use std::sync::Arc;

use crate::brain::provider::Provider;
use crate::config::Config;

/// Resolve `config.agent.eval_providers` into live providers. Returns an empty
/// vec when the chain is empty (live evals off) or nothing resolves.
pub async fn resolve_eval_providers(config: &Config) -> Vec<Arc<dyn Provider>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Arc<dyn Provider>> = Vec::new();
    for raw in &config.agent.eval_providers {
        let name = raw.trim();
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        match crate::brain::provider::factory::create_provider_by_name(config, name).await {
            Ok(provider) => {
                tracing::info!(
                    "eval_providers: resolved '{}' -> model '{}'",
                    name,
                    provider.default_model()
                );
                out.push(provider);
            }
            Err(e) => {
                tracing::warn!("eval_providers: skipping '{}' — {}", name, e);
            }
        }
    }
    out
}
