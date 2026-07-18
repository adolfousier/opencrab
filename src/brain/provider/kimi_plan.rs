//! Kimi Code subscription-plan tiers and their context-window budgets.
//!
//! The Kimi Code token subscription (`api.kimi.com/coding/v1`) exposes several
//! tiers whose names come from musical tempo markings. The tier — not the model
//! name alone — decides how large a context window the account may use:
//!
//! - **moderato** — 256K context (the entry coding tier)
//! - **allegretto / allegro / vivace** — up to 1M context
//!
//! The model also caps the window: the K2.7 coding models
//! (`kimi-for-coding`, `kimi-for-coding-highspeed`) top out at 256K regardless
//! of tier, while K3 (`k3`) scales to 1M on allegretto and above.
//!
//! An explicit `context_window` in the provider config always wins; this
//! mapping only supplies a default when the window is left unset.

const CONTEXT_256K: u32 = 256_000;
const CONTEXT_1M: u32 = 1_000_000;

/// Kimi Code plan tiers, cheapest first. Used by onboarding to offer a picker.
pub const PLAN_TIERS: [&str; 4] = ["moderato", "allegretto", "allegro", "vivace"];

/// Returns true when the model id names a K2.7 coding model, which is capped at
/// 256K context on every plan tier.
fn is_k27_coding_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("kimi-for-coding")
}

/// Derive the context-window budget (in tokens) implied by a Kimi coding-plan
/// tier and the active model.
///
/// Returns `None` for an unrecognised plan name so the caller can fall back to
/// its own default rather than silently pinning a wrong budget.
pub fn context_window_for_plan(plan: &str, model: Option<&str>) -> Option<u32> {
    // K2.7 coding models are 256K on any tier.
    if let Some(m) = model
        && is_k27_coding_model(m)
    {
        return Some(CONTEXT_256K);
    }
    match plan.trim().to_ascii_lowercase().as_str() {
        "moderato" => Some(CONTEXT_256K),
        "allegretto" | "allegro" | "vivace" => Some(CONTEXT_1M),
        _ => None,
    }
}
