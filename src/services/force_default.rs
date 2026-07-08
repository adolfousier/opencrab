//! `force_default` push (#466): on config reload, when the ACTIVE default
//! provider's section carries `force_default = true`, its default pair is
//! written to every non-archived session, overriding their stored pairs.
//! Without the flag, the post-#379 isolation holds: defaults apply to new
//! sessions only. Live sessions pick the pair up through the existing
//! per-message sync path; nothing is invented — the push uses exactly the
//! section's configured default pair.

use crate::config::Config;
use crate::db::repository::SessionListOptions;
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
/// are skipped (no updated_at churn).
pub async fn apply_force_default(config: &Config, session_svc: &SessionService) -> Result<usize> {
    let Some((provider, model)) = force_default_pair(config) else {
        return Ok(0);
    };
    let sessions = session_svc
        .list_sessions(SessionListOptions {
            include_archived: false,
            ..Default::default()
        })
        .await?;
    let mut updated = 0usize;
    for mut session in sessions {
        if session.provider_name.as_deref() == Some(provider.as_str())
            && session.model.as_deref() == Some(model.as_str())
        {
            continue;
        }
        session.provider_name = Some(provider.clone());
        session.model = Some(model.clone());
        session_svc.update_session(&session).await?;
        updated += 1;
    }
    if updated > 0 {
        tracing::info!(
            "force_default (#466): pushed {provider}/{model} to {updated} session(s) on reload"
        );
    }
    Ok(updated)
}
