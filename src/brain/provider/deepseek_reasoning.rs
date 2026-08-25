//! DeepSeek thinking-knob resolution.
//!
//! DeepSeek is not a built-in provider: it is reached only through a custom
//! OpenAI-compatible entry whose `base_url` or model id names it. So both the
//! host check and the model check have to recognise it, the same way the qwen
//! path does for gateways that re-serve those models (#1040).
//!
//! Its controls are its own, and are NOT the DashScope ones:
//!
//! - `thinking: {"type": "enabled" | "disabled"}` at the TOP LEVEL of the
//!   request body, not inside `extra_body`.
//! - `reasoning_effort` on a `low | high | max` ladder. There is no `xhigh`;
//!   that rung belongs to qwen3.8-max.
//! - Off is not a rung. It is `thinking: {"type": "disabled"}` with no effort
//!   field at all, so `off` never reaches the wire as an effort value.
//!
//! Sending DashScope's `enable_thinking` to a DeepSeek endpoint is not a
//! no-op with a different name: the field is ignored, the knob DeepSeek reads
//! goes unset, and thinking silently runs at the server default instead of
//! what the user configured. That is the same failure the qwen families had.

/// The effort ladder DeepSeek accepts on the wire.
const WIRE_EFFORTS: [&str; 3] = ["low", "high", "max"];

/// Applied when a DeepSeek target has no configured effort. `high` is the
/// vendor default, so an unconfigured install lands there rather than on
/// whatever the endpoint happens to choose. An explicit value always wins.
const DEFAULT_EFFORT: &str = "high";

/// What a request should carry for thinking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeepSeekThinking {
    /// Value for the top-level `thinking.type` field.
    pub enabled: bool,
    /// Value for `reasoning_effort`. `None` when thinking is disabled, since
    /// an effort alongside a disabled toggle is a second competing knob.
    pub effort: Option<&'static str>,
}

/// Hosts operated by DeepSeek. Only used to recognise a DeepSeek target when
/// the model id does not say so; deliberately NOT the sole gate, because the
/// same models are served by other OpenAI-compatible gateways.
fn is_deepseek_host(base_url: &str) -> bool {
    base_url.to_ascii_lowercase().contains("deepseek")
}

/// Does this model id name a DeepSeek model?
///
/// Checked on the vendor-stripped, lowercased id so a namespaced
/// `deepseek/deepseek-v4-pro` or `SomeVendor/DeepSeek-V4-Flash` is recognised
/// rather than missed for carrying a prefix.
fn is_deepseek_model(model: &str) -> bool {
    super::qwen::bare_model_id(model).starts_with("deepseek")
}

/// Is this request bound for a DeepSeek model?
///
/// Either signal is enough: the id names it, or the endpoint does. A local
/// runtime is excluded, which is the same carve-out the qwen path makes:
/// llama.cpp and MLX serving a DeepSeek GGUF accept neither knob.
pub(crate) fn serves_deepseek(base_url: &str, model: &str) -> bool {
    (is_deepseek_model(model) || is_deepseek_host(base_url))
        && !super::factory::is_local_base_url(base_url)
}

/// Resolve the configured knobs into what the wire should carry.
///
/// `configured_effort` is the provider's `reasoning_effort` and
/// `configured_enable_thinking` its `enable_thinking`. The latter is
/// DashScope's spelling, but a user who set it meant "thinking off", and
/// honouring it here is what keeps an explicit off from silently becoming
/// the server default.
pub(crate) fn resolve(
    configured_effort: Option<&str>,
    configured_enable_thinking: Option<bool>,
) -> DeepSeekThinking {
    let effort = configured_effort.map(str::trim).unwrap_or_default();
    let lowered = effort.to_ascii_lowercase();

    // An explicit off, by either spelling, disables thinking outright.
    let disabled_by_effort = matches!(lowered.as_str(), "off" | "none" | "disabled");
    if configured_enable_thinking == Some(false) || disabled_by_effort {
        return DeepSeekThinking {
            enabled: false,
            effort: None,
        };
    }

    // A rung DeepSeek does not have (qwen's `xhigh`, a typo, anything else)
    // must not ride to the wire, or the request is rejected for a value the
    // user could have set on a different provider entirely. Fall to the
    // vendor default and keep thinking on.
    let effort = WIRE_EFFORTS
        .iter()
        .find(|rung| **rung == lowered)
        .copied()
        .unwrap_or(DEFAULT_EFFORT);

    DeepSeekThinking {
        enabled: true,
        effort: Some(effort),
    }
}

/// Context capacity DeepSeek documents for its current models. Used only when
/// the user configured none, so an entry added without a `context_window` gets
/// the vendor's figure instead of the generic fallback. An explicit value in
/// config always wins.
pub(crate) const DEFAULT_CONTEXT_WINDOW: u32 = 1_000_000;

// No output cap is defined here on purpose. When the caller sets no
// `max_tokens` we send no cap at all, so DeepSeek applies its own default;
// hard-coding the vendor's current figure would override that default rather
// than match it, and would go stale the day they change it.

/// The vendor default context window for a DeepSeek model, or `None` when the
/// model is not one. Deliberately keyed on the model rather than the endpoint:
/// a gateway serving DeepSeek alongside other families must not hand this
/// figure to the others.
pub(crate) fn default_context_window(model: &str) -> Option<u32> {
    is_deepseek_model(model).then_some(DEFAULT_CONTEXT_WINDOW)
}
