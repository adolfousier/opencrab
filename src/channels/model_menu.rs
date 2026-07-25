//! What the model picker last put on screen, per provider.
//!
//! Telegram caps inline-button `callback_data` at 64 bytes, so a model whose
//! name does not fit is encoded as its *position* in the rendered list and
//! resolved back when the button is tapped. That resolution has to see the
//! same list the user was looking at.
//!
//! Config alone used to reconstruct it, because the picker rendered exactly
//! the `models = [...]` array. Now that the list is the config seed unioned
//! with the provider's live inventory (#761), it cannot. And an unresolved
//! index does not fail safe: the caller falls back to the raw token, so a
//! miss would try to select a model literally named `#7`.
//!
//! This deliberately stores what was rendered rather than caching provider
//! state. It answers "what was on screen when that button was drawn", which
//! stays right even if the provider's inventory changes before the tap.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static RENDERED: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Vec<String>>> {
    RENDERED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the list a provider's picker just rendered, replacing any previous
/// one. Called on every render so the newest menu is the one indices resolve
/// against.
pub(crate) fn remember(provider_name: &str, models: &[String]) {
    match registry().lock() {
        Ok(mut map) => {
            map.insert(provider_name.to_string(), models.to_vec());
        }
        // A poisoned lock only costs index resolution for long model names,
        // which then falls back to config. Never worth taking the process down.
        Err(e) => tracing::warn!("model menu: could not record rendered list: {e}"),
    }
}

/// Resolve a button's position back to the model name that occupied it.
///
/// `None` when this provider's picker has not rendered in this process (a
/// restart between render and tap), leaving the caller to fall back to config.
pub(crate) fn resolve_index(provider_name: &str, index: usize) -> Option<String> {
    match registry().lock() {
        Ok(map) => map.get(provider_name)?.get(index).cloned(),
        Err(e) => {
            tracing::warn!("model menu: could not read rendered list: {e}");
            None
        }
    }
}
