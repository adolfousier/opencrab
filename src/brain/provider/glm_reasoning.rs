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
