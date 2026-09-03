//! The provider/model a child agent runs on (#1316).
//!
//! spawn_agent, resume_agent and team_create all resolve the same way: a
//! per-call override beats `[agent] subagent_provider` / `subagent_model`, and
//! whichever value wins is normalised so `"custom:myprovider"` or
//! `"myprovider/some-model"` resolves to the provider it names instead of
//! failing to build and silently falling back to the parent's provider.

use crate::brain::provider_spec::{ProviderKey, normalize_in};
use crate::config::Config;

/// Where the winning provider value came from, for the log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSource {
    PerCall,
    Config,
}

/// The resolved pair: `provider` is `None` when neither the call nor the
/// config named one, in which case the child inherits the parent's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildPair {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub source: ProviderSource,
}

/// Resolve and normalise the child's pair. Logs one warning naming the
/// canonical spelling when a correction was applied.
pub fn child_pair(
    config: &Config,
    call_provider: Option<&str>,
    call_model: Option<&str>,
) -> ChildPair {
    let (provider, source) = match call_provider {
        Some(p) => (Some(p), ProviderSource::PerCall),
        None => (
            config.agent.subagent_provider.as_deref(),
            ProviderSource::Config,
        ),
    };
    let model = call_model.or(config.agent.subagent_model.as_deref());

    let Some(provider) = provider else {
        return ChildPair {
            provider: None,
            model: model.map(str::to_string),
            source,
        };
    };

    let pair = normalize_in(config, ProviderKey::SUBAGENT, provider, model);
    if let Some(note) = pair.note.as_deref() {
        tracing::warn!(
            "Sub-agent provider corrected to '{}': {note}",
            pair.provider
        );
    }
    ChildPair {
        provider: Some(pair.provider),
        model: pair.model,
        source,
    }
}
