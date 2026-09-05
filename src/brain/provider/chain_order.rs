//! The order a chain is walked when the reason for walking is size (#1379).
//!
//! For most failures the configured order is the user's preference and is
//! kept. A context-length rejection is different: the request is too big for
//! the window it hit, so the entry most likely to accept it is the one with
//! the widest window, and asking a narrower one first is a guaranteed refusal
//! spent before the answer. Sorted by each entry's window for its own default
//! model (that is the model a substitute runs), widest first, stable so equal
//! or unknown windows keep the configured order, unknown last.

use std::sync::Arc;

use super::Provider;

/// `fallbacks` reordered widest window first. Entries with no known window
/// keep their relative order behind every known one.
pub(crate) fn widest_first(fallbacks: &[Arc<dyn Provider>]) -> Vec<Arc<dyn Provider>> {
    let mut ordered: Vec<Arc<dyn Provider>> = fallbacks.to_vec();
    ordered.sort_by_key(|fb| std::cmp::Reverse(window_of(fb.as_ref())));
    ordered
}

/// The window an entry offers the model it would actually run.
fn window_of(fb: &dyn Provider) -> Option<u32> {
    fb.context_window(fb.default_model())
}
