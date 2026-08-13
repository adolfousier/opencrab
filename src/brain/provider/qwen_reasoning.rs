//! Qwen (DashScope / Model Studio) reasoning wire contracts.
//!
//! Every DashScope request carries `preserve_thinking: true` so reasoning
//! carries across turns instead of being re-derived from scratch each time.
//! See [`preserve_thinking_for`].

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
