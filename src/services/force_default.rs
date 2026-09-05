//! `force_default` push (#466): on config reload, when the ACTIVE default
//! provider's section carries `force_default = true`, its default pair is
//! written to every non-archived session, overriding their stored pairs.
//! Without the flag, the post-#379 isolation holds: defaults apply to new
//! sessions only. Live sessions pick the pair up through the existing
//! per-message sync path; nothing is invented — the push uses exactly the
//! section's configured default pair.

use crate::config::Config;
use crate::services::SessionService;
use anyhow::Result;

/// The `(provider, model)` a reload should broadcast, or `None` when the
/// active default provider's section doesn't opt in. Pure — unit tested
/// without a database.
pub fn force_default_pair(config: &Config) -> Option<(String, String)> {
    let (provider, model) = config.providers.active_provider_and_model();
    let section = crate::brain::provider::factory::provider_config_by_name(config, &provider)?;
    if !section.force_default {
        return None;
    }
    Some((provider, model))
}

/// Apply the force-default push, returning how many sessions changed.
/// Archived sessions are never touched, and sessions already on the pair
/// are skipped. Routed through the bulk UPDATE (#1367): only provider/model
/// columns are written, `updated_at` is never stamped, so a reload cannot
/// flatten `/sessions` recency ordering.
pub async fn apply_force_default(config: &Config, session_svc: &SessionService) -> Result<usize> {
    let Some((provider, model)) = force_default_pair(config) else {
        return Ok(0);
    };
    let updated = session_svc
        .set_provider_model_all_sessions(&provider, &model)
        .await?;
    if updated > 0 {
        tracing::info!(
            "force_default (#466): pushed {provider}/{model} to {updated} session(s) on reload"
        );
    }
    Ok(updated)
}
