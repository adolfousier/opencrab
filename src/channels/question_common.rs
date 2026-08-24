//! Shared mechanics for `follow_up_question` and `suggest_followups`
//! (#764 R1/R2/R3/R5). The two tools stay semantically different
//! (blocking vs non-blocking); only their duplicated *mechanics* live here.

use std::collections::HashSet;
use std::sync::Mutex;

use tokio::task::JoinHandle;

use crate::brain::agent::AgentError;

/// Neutral validation outcome for [`check_options`]. Callers own their
/// tool-specific error wording — both tools' messages are pinned
/// byte-for-byte by existing tests, so this layer never formats text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OptionsError {
    TooFew { got: usize, min: usize },
    TooMany(usize),
    Duplicate(String),
}

/// Shared trim/filter/dedup validation (#764 R1): trim each entry, drop
/// empties, enforce the min..=max window and distinctness.
pub(crate) fn check_options(
    raw: Vec<String>,
    min: usize,
    max: usize,
) -> Result<Vec<String>, OptionsError> {
    let options: Vec<String> = raw
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if options.len() < min {
        return Err(OptionsError::TooFew {
            got: options.len(),
            min,
        });
    }
    if options.len() > max {
        return Err(OptionsError::TooMany(options.len()));
    }
    let mut seen = HashSet::new();
    for opt in &options {
        if !seen.insert(opt.as_str()) {
            return Err(OptionsError::Duplicate(opt.clone()));
        }
    }
    Ok(options)
}

/// Await a blocking question's oneshot with the shared 600s timeout and the
/// identical 3-arm match (#764 R2). Telegram is deliberately NOT rewired:
/// its arm carries extra semantics (#500 pending-question cleanup + answer
/// logging), so collapsing it would change behavior.
pub(crate) async fn await_answer(
    rx: tokio::sync::oneshot::Receiver<String>,
) -> Result<String, AgentError> {
    match tokio::time::timeout(std::time::Duration::from_secs(600), rx).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(_)) => Err(AgentError::Internal(
            "follow_up_question oneshot closed".into(),
        )),
        Err(_) => Err(AgentError::Internal("follow_up_question timed out".into())),
    }
}

/// Drain in-flight intermediate text spawns before posting the question so
/// context lands above the buttons/list instead of below (#142, #764 R3).
/// Identical block previously hand-copied across discord/slack/whatsapp.
pub(crate) async fn drain_intermediate_handles(handles: &Mutex<Vec<JoinHandle<()>>>, origin: &str) {
    let pending = {
        let mut g = handles.lock().expect("poisoned");
        std::mem::take(&mut *g)
    };
    for h in pending {
        if let Err(e) = h.await {
            tracing::warn!(error = %e, "{origin} follow-up task panicked");
        }
    }
}

/// Per-channel session→owner fallback, error variant (#764 R5): the blocking
/// tool surfaces a failure when neither lookup lands.
///
/// Takes the two lookup *futures* rather than a state type so Discord
/// (`u64`) and Slack (`String`) share one implementation without a trait.
pub(crate) async fn resolve_channel_or_error<T>(
    session_lookup: impl std::future::Future<Output = Option<T>>,
    owner_lookup: impl std::future::Future<Output = Option<T>>,
) -> Result<T, AgentError> {
    if let Some(id) = session_lookup.await {
        return Ok(id);
    }
    if let Some(id) = owner_lookup.await {
        return Ok(id);
    }
    Err(AgentError::Internal("no channel_id for session".into()))
}

/// Silent twin of [`resolve_channel_or_error`] for the non-blocking tool:
/// suggest_followups just returns without rendering when neither lands.
pub(crate) async fn resolve_channel_or_silent<T>(
    session_lookup: impl std::future::Future<Output = Option<T>>,
    owner_lookup: impl std::future::Future<Output = Option<T>>,
) -> Option<T> {
    match session_lookup.await {
        Some(id) => Some(id),
        None => owner_lookup.await,
    }
}
