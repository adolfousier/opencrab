//! Pending select menus (#382) and modal forms (#383), keyed by interaction
//! id with a lazy TTL: a stale pick answers "expired" (#386) and is dropped
//! on the way out instead of being swept by a timer.

use std::time::Instant;

use super::DiscordState;
use super::interactions::FormSpec;

/// True once `created` is older than `ttl_hours`.
fn expired(created: &Instant, ttl_hours: f64) -> bool {
    created.elapsed().as_secs_f64() > ttl_hours * 3600.0
}

impl DiscordState {
    /// Register a pending select menu (#382). Bounded by lazy TTL expiry.
    pub(crate) async fn register_select(&self, id: String, options: Vec<String>) {
        self.pending_selects
            .lock()
            .await
            .insert(id, (Instant::now(), options));
    }

    /// Take a pending select if it is still within `ttl_hours`; expired or
    /// unknown entries return None (and expired ones are dropped).
    pub(crate) async fn take_select(&self, id: &str, ttl_hours: f64) -> Option<Vec<String>> {
        let mut map = self.pending_selects.lock().await;
        let (created, _) = map.get(id)?;
        if expired(created, ttl_hours) {
            map.remove(id);
            return None;
        }
        map.remove(id).map(|(_, opts)| opts)
    }

    /// Register a pending modal form spec (#383).
    pub(crate) async fn register_form(&self, id: String, spec: FormSpec) {
        self.pending_forms
            .lock()
            .await
            .insert(id, (Instant::now(), spec));
    }

    /// Fetch a pending form spec within TTL (kept until submitted so the
    /// button can be pressed once per open; submission consumes it).
    pub(crate) async fn get_form(&self, id: &str, ttl_hours: f64) -> Option<FormSpec> {
        let mut map = self.pending_forms.lock().await;
        let (created, _) = map.get(id)?;
        if expired(created, ttl_hours) {
            map.remove(id);
            return None;
        }
        map.get(id).map(|(_, spec)| spec.clone())
    }

    /// Consume a form spec on submission.
    pub(crate) async fn take_form(&self, id: &str) -> Option<FormSpec> {
        self.pending_forms.lock().await.remove(id).map(|(_, s)| s)
    }
}
