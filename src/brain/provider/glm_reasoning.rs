//! z.ai GLM request knobs.
//!
//! z.ai (`api.z.ai`, `open.bigmodel.cn`) is an OpenAI-shaped endpoint with
//! three fields the generic shape does not carry. Each one is here because
//! leaving it unset produced a real failure, not because the docs list it:
//!
//! - `tool_stream: true` (#1347). Without it z.ai buffers a tool call's
//!   arguments and emits them as ONE chunk after the whole call is generated.
//!   A 40 KB `write_file` argument at ~75 tok/s is minutes of silence on the
//!   SSE stream, which the 90s idle timer reads as a dead connection; the
//!   retry re-sends the same request and dies the same way. With the flag the
//!   arguments arrive as `delta.tool_calls[].function.arguments` fragments,
//!   which the stream reader already accumulates.
//! - `thinking: {"type": "enabled", "clear_thinking": false}` (#1348).
//!   GLM-5.x is always-on thinking; `clear_thinking: false` is z.ai's
//!   Preserved Thinking: the server keeps prior turns' reasoning so the model
//!   does not re-derive the whole chain on every tool step, and reasoning
//!   tokens become cacheable. It defaults to `true` on the standard endpoint,
//!   so an agentic loop there rethought from scratch on every call.
//!
//! Gated on the HOST via [`super::identity::Vendor`], never on the model id:
//! `glm-*` models are re-served by OpenRouter and Model Studio, and those
//! gateways do not read z.ai's fields.

use super::identity::Vendor;

/// Is this request bound for z.ai itself (either documented host)?
pub(crate) fn serves_zai(base_url: &str) -> bool {
    matches!(Vendor::from_base_url(base_url), Some(Vendor::Zai))
}

/// Value for the request's `tool_stream` field: `Some(true)` on z.ai when
/// the response is streamed, `None` (field omitted) everywhere else. z.ai
/// documents the flag as streaming-only, so a non-streamed request never
/// carries it.
pub(crate) fn tool_stream_for(base_url: &str, stream: bool) -> Option<bool> {
    (stream && serves_zai(base_url)).then_some(true)
}

/// `(major, minor)` of a GLM model id, vendor prefix and case ignored:
/// `glm-5.3-flash` is `(5, 3)`, `z-ai/GLM-5` is `(5, 0)`. `None` for anything
/// that is not a GLM id.
pub(crate) fn glm_version(model: &str) -> Option<(u32, u32)> {
    let bare = super::qwen::bare_model_id(model);
    let rest = bare.strip_prefix("glm-")?;
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = num.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// GLM-5.3 and later cannot turn thinking off: `thinking.type: disabled` is
/// rejected on the standard channel.
fn thinking_always_on(version: (u32, u32)) -> bool {
    version >= (5, 3)
}

/// What a request should carry for `thinking`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GlmThinking {
    pub enabled: bool,
    /// `true` when the user configured thinking off on a model that cannot
    /// honour it; the caller logs it once so the setting is not silently
    /// ignored.
    pub off_ignored: bool,
}

impl GlmThinking {
    /// The top-level `thinking` object for the wire.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        if self.enabled {
            serde_json::json!({ "type": "enabled", "clear_thinking": false })
        } else {
            serde_json::json!({ "type": "disabled" })
        }
    }
}

/// The `thinking` object for a GLM-5.x request on z.ai, or `None` when the
/// request is not one (other hosts, GLM-4.x, non-GLM ids) so the field is
/// omitted and nothing changes for them.
///
/// `configured_enable_thinking` is the provider's `enable_thinking`. It is
/// honoured on GLM-5.0 to 5.2, which still accept `disabled`; on 5.3+ it is
/// reported as ignored rather than sent, because the endpoint rejects it.
pub(crate) fn thinking_for(
    base_url: &str,
    model: &str,
    configured_enable_thinking: Option<bool>,
) -> Option<GlmThinking> {
    if !serves_zai(base_url) {
        return None;
    }
    let version = glm_version(model)?;
    if version < (5, 0) {
        return None;
    }
    let wants_off = configured_enable_thinking == Some(false);
    if wants_off && !thinking_always_on(version) {
        return Some(GlmThinking {
            enabled: false,
            off_ignored: false,
        });
    }
    Some(GlmThinking {
        enabled: true,
        off_ignored: wants_off,
    })
}
