//! Qwen (DashScope / Model Studio) thinking-knob resolution, per family.
//!
//! DashScope qwen models expose two different thinking controls and each
//! family reads only one of them:
//!
//! - **qwen3.8-max** — the tiered `reasoning_effort` ladder. Thinking is
//!   always on; `enable_thinking` is inert here and shipping it alongside a
//!   tier is a second competing knob.
//! - **older qwen hybrids** (qwen3.6-*, qwen3.7-*, qwen3-*, qwen-*) — the
//!   on/off `enable_thinking` switch only. They treat `reasoning_effort` as
//!   an opaque sampling override, so a configured value still passes through
//!   (that pass-through is #691 and must not regress).
//!
//! Sending the wrong knob is not a no-op: the inert field is ignored while the
//! knob the model actually reads goes unset, so thinking silently runs at the
//! server default instead of what the user configured.
//!
//! Separately, every DashScope request carries `preserve_thinking: true` so
//! reasoning carries across turns instead of being re-derived from scratch
//! each time. See [`preserve_thinking_for`].
//!
//! Mirrors `DashScopeOpenAICompatibleProvider.buildQwenEffortConfig` /
//! `dropConflictingThinkingKnobs` in qwen-code. Same shape as
//! [`super::kimi_reasoning`], which solves this per-family problem for the
//! Kimi models.

/// A qwen family, distinguished by which thinking knob it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QwenFamily {
    /// qwen3.8-max — reads the tiered `reasoning_effort` ladder.
    TieredEffort,
    /// Older qwen hybrids — read the on/off `enable_thinking` switch.
    HybridThinking,
}

/// Model-id prefix for the tiered-effort family. Matches qwen-code's
/// `isTieredEffortWireModel`, which is likewise a plain prefix test so
/// dated and `-preview` suffixes are covered without enumerating them.
const TIERED_EFFORT_PREFIX: &str = "qwen3.8-max";

/// The canonical disable for the tiered family. `enable_thinking = false` is
/// the documented escape hatch, and on a model that reads only
/// `reasoning_effort` it has to be translated rather than dropped, or the
/// user's explicit "off" would silently become "server default".
const EFFORT_DISABLED: &str = "none";

/// Effort tier applied to the tiered family when nothing is configured. The
/// top of the ladder is the vendor's recommended setting for this family, so
/// an unconfigured install gets it rather than whatever the endpoint happens
/// to fall back to. An explicit `reasoning_effort` in config still wins.
const DEFAULT_TIERED_EFFORT: &str = "xhigh";

/// Classify a model id, or `None` when it is not a qwen model at all.
pub(crate) fn family(model: &str) -> Option<QwenFamily> {
    let m = model.to_ascii_lowercase();
    if m.starts_with(TIERED_EFFORT_PREFIX) {
        Some(QwenFamily::TieredEffort)
    } else if m.starts_with("qwen") {
        Some(QwenFamily::HybridThinking)
    } else {
        None
    }
}

/// The thinking fields to put on an outgoing request body. Each is `None`
/// when that field must not be sent at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct QwenThinkingKnobs {
    /// Top-level `reasoning_effort`.
    pub(crate) reasoning_effort: Option<String>,
    /// Top-level `enable_thinking`.
    pub(crate) enable_thinking: Option<bool>,
}

/// Resolve the configured thinking settings into the exact fields the active
/// model reads, dropping the one it does not.
///
/// `configured_effort` is the provider's `reasoning_effort` setting and
/// `configured_enable_thinking` its `enable_thinking` setting; either may be
/// unset.
///
/// - **Tiered family** — the effort tier ships alone. A co-present
///   `enable_thinking` is dropped, except that an explicit `false` becomes
///   `reasoning_effort = "none"`. With nothing configured the tier defaults
///   to [`DEFAULT_TIERED_EFFORT`] rather than being omitted, so an
///   unconfigured install still gets the vendor's recommended setting.
/// - **Hybrid family** — `enable_thinking` ships, defaulting to on (every
///   Model Studio catalogue entry for these declares thinking enabled). A
///   configured effort still passes through untouched: the model does not
///   read it, so it is an opaque override rather than a competing knob.
/// - **Not a qwen model** — nothing; the caller's existing fields stand.
pub(crate) fn resolve(
    model: &str,
    configured_effort: Option<&str>,
    configured_enable_thinking: Option<bool>,
) -> QwenThinkingKnobs {
    match family(model) {
        None => QwenThinkingKnobs::default(),
        Some(QwenFamily::TieredEffort) => {
            if configured_enable_thinking == Some(false) {
                return QwenThinkingKnobs {
                    reasoning_effort: Some(EFFORT_DISABLED.to_string()),
                    enable_thinking: None,
                };
            }
            QwenThinkingKnobs {
                reasoning_effort: Some(
                    configured_effort
                        .unwrap_or(DEFAULT_TIERED_EFFORT)
                        .to_string(),
                ),
                enable_thinking: None,
            }
        }
        Some(QwenFamily::HybridThinking) => QwenThinkingKnobs {
            reasoning_effort: configured_effort.map(str::to_string),
            enable_thinking: Some(configured_enable_thinking.unwrap_or(true)),
        },
    }
}

/// Whether this request should carry `preserve_thinking: true`.
///
/// True for any DashScope-hosted target. Reasoning models there need the flag
/// for multi-turn reasoning continuity: without it the model re-derives its
/// reasoning every turn instead of carrying it forward, which shows up as
/// drifting and invented intermediate steps on long sessions.
///
/// Gated on the host rather than the model name so a locally served qwen GGUF
/// (llama.cpp, LM Studio, Ollama) is untouched — those take thinking through
/// `chat_template_kwargs`, not through DashScope request fields.
pub(crate) fn preserve_thinking_for(base_url: &str) -> bool {
    super::qwen::is_dashscope_host(base_url)
}
