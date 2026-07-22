//! Kimi reasoning-control mapping, validated per model.
//!
//! Kimi coding models expose reasoning knobs the API validates per model
//! (see platform.kimi.ai/docs/api/chat):
//!
//! - **kimi-k3** — top-level `reasoning_effort`, currently only `"max"`
//!   (thinking is always on and preserved).
//! - **kimi-k2.7-code** — `thinking.type` only `"enabled"` (cannot be
//!   disabled; preserved thinking is always active).
//! - **kimi-k2.6** — `thinking.type` `"enabled"` or `"disabled"`.
//!
//! This module maps a user-facing setting (e.g. `"max"`, `"on"`, `"off"`) to
//! the exact body patch the active model accepts, or reports why the model
//! cannot take that value — so a config default can silently skip an
//! inapplicable value while `/reason` can surface a clear message.

/// True when the endpoint streams its reasoning inline as ordinary `content`
/// with no `reasoning_content` field and no delimiter tags — so the reasoning
/// cannot be separated at the stream layer and must be classified structurally.
///
/// This is the Kimi Code endpoint (`api.kimi.com/coding/v1`): K3 always thinks
/// with Preserved Thinking and emits the whole chain as content. The Moonshot
/// API endpoint (`api.moonshot.ai`) separates reasoning properly and is not
/// matched here.
pub fn streams_reasoning_inline(base_url: Option<&str>) -> bool {
    base_url
        .map(|u| u.to_ascii_lowercase().contains("kimi.com/coding"))
        .unwrap_or(false)
}

/// The concrete change to apply to an outgoing request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningPatch {
    /// Set top-level `reasoning_effort` to this value (kimi-k3).
    Effort(String),
    /// Set `thinking: { "type": "enabled" | "disabled" }` (kimi-k2.x).
    Thinking(bool),
}

/// Why a requested reasoning value can't be applied to the active model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningError {
    /// The Kimi model family the value was rejected for.
    pub family: &'static str,
    /// Human-readable list of what the family does accept.
    pub allowed: &'static str,
}

/// Kimi reasoning-model families, distinguished by model id substring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// kimi-k3 — `reasoning_effort`, only `max` today.
    K3,
    /// kimi-k2.7-code (a.k.a. `kimi-for-coding`) — thinking always enabled.
    K27Code,
    /// kimi-k2.6 — thinking enabled/disabled.
    K26,
}

/// Whether `model` is a Kimi family this module's reasoning controls apply to.
/// Non-Kimi custom-provider models (e.g. DashScope qwen) get a generic
/// `reasoning_effort` pass-through instead of this family-specific resolution
/// (#691), so a configured effort like `xhigh` is actually sent.
pub fn is_kimi_model(model: &str) -> bool {
    classify(model).is_some()
}

fn classify(model: &str) -> Option<Family> {
    let m = model.to_ascii_lowercase();
    // Order matters: k2.7 aliases before the generic k2.6 check.
    if m.contains("k3") {
        Some(Family::K3)
    } else if m.contains("k2.7") || m.contains("kimi-for-coding") {
        Some(Family::K27Code)
    } else if m.contains("k2.6") {
        Some(Family::K26)
    } else {
        None
    }
}

/// Does this setting string mean "thinking on"?
fn is_on(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "on" | "enabled" | "enable" | "true" | "max" | "high" | "low"
    )
}

/// Does this setting string mean "thinking off"?
fn is_off(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "off" | "disabled" | "disable" | "false" | "none"
    )
}

/// Resolve a user-facing reasoning setting for a model into a body patch.
///
/// - `Ok(None)` — the model has no configurable reasoning control (not a Kimi
///   reasoning model); leave the request untouched.
/// - `Ok(Some(patch))` — apply `patch` to the body.
/// - `Err(_)` — the model cannot accept this value; the caller decides whether
///   to skip silently (config default) or report it (`/reason`).
pub fn resolve(model: &str, setting: &str) -> Result<Option<ReasoningPatch>, ReasoningError> {
    let Some(family) = classify(model) else {
        return Ok(None);
    };
    let s = setting.trim().to_ascii_lowercase();
    match family {
        // K3 only accepts reasoning_effort=max today.
        Family::K3 => {
            if s == "max" {
                Ok(Some(ReasoningPatch::Effort("max".to_string())))
            } else {
                Err(ReasoningError {
                    family: "kimi-k3",
                    allowed: "max",
                })
            }
        }
        // K2.7-code thinking is always on; it cannot be disabled.
        Family::K27Code => {
            if is_on(&s) {
                Ok(Some(ReasoningPatch::Thinking(true)))
            } else {
                Err(ReasoningError {
                    family: "kimi-k2.7-code",
                    allowed: "on (thinking is always enabled)",
                })
            }
        }
        // K2.6 supports a real on/off toggle.
        Family::K26 => {
            if is_on(&s) {
                Ok(Some(ReasoningPatch::Thinking(true)))
            } else if is_off(&s) {
                Ok(Some(ReasoningPatch::Thinking(false)))
            } else {
                Err(ReasoningError {
                    family: "kimi-k2.6",
                    allowed: "on, off",
                })
            }
        }
    }
}

/// Split a [`ReasoningPatch`] into the two request-body fields it sets:
/// `(reasoning_effort, thinking)`. Exactly one is `Some`.
pub fn patch_fields(patch: &ReasoningPatch) -> (Option<String>, Option<serde_json::Value>) {
    match patch {
        ReasoningPatch::Effort(effort) => (Some(effort.clone()), None),
        ReasoningPatch::Thinking(enabled) => {
            let ty = if *enabled { "enabled" } else { "disabled" };
            (None, Some(serde_json::json!({ "type": ty })))
        }
    }
}

/// Resolve a setting for a model straight to the `(reasoning_effort, thinking)`
/// body fields, silently skipping a value the model can't accept. Used by the
/// config-default path where an inapplicable value is a no-op rather than an
/// error.
pub fn resolve_fields(model: &str, setting: &str) -> (Option<String>, Option<serde_json::Value>) {
    match resolve(model, setting) {
        Ok(Some(patch)) => patch_fields(&patch),
        _ => (None, None),
    }
}
