//! Qwen (DashScope) header + body-transform helpers.
//!
//! OpenCrabs talks to Qwen through the standard OpenAI-compatible DashScope
//! endpoint with a regular API key (DashScope or Alibaba Coding Plan). This
//! file only contains the wire-level fingerprinting logic that the factory
//! layers on top of `OpenAIProvider`:
//!
//! - `qwen_extra_headers()` — the four `X-DashScope-*` / `User-Agent`
//!   headers that qwen-cli emits. Matching them keeps us out of the
//!   stricter rate-limit bucket the gateway applies to unknown clients.
//! - `qwen_body_transform()` — the DashScope cache-control + metadata
//!   shape that qwen-cli posts alongside the OpenAI-compatible request.
//!
//! The older OAuth device flow, token manager, multi-account rotation, and
//! `~/.qwen/oauth_creds.json` sync were removed when Alibaba discontinued
//! Qwen OAuth — the `portal.qwen.ai` endpoint now only accepts DashScope /
//! Coding Plan API keys, and the CLI itself dropped the OAuth option.

// ── DashScope headers ─────────────────────────────────────────────────────

/// Version string sent in `User-Agent` and `X-DashScope-UserAgent`.
/// Must stay `QwenCode/<semver>` — the gateway validates the prefix.
const QWEN_CLI_VERSION: &str = "0.14.0";

/// Node-style arch token for `User-Agent` / `X-DashScope-UserAgent`.
/// qwen-cli uses Node's `process.arch` values directly.
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "arm",
        "x86" => "ia32",
        other => other,
    }
}

/// Platform tuple baked into `User-Agent` and `X-DashScope-UserAgent`.
/// qwen-cli constructs these as `${process.platform}; ${process.arch}`
/// where `platform` is `darwin` / `linux` / `win32`.
fn user_agent_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    format!("{}; {}", os, node_arch())
}

/// Extra headers sent with every DashScope request.
///
/// These are the **exact four** headers that
/// `DashScopeOpenAICompatibleProvider.buildHeaders()` emits in
/// `@qwen-code/qwen-code`:
///
/// ```text
/// User-Agent: QwenCode/<version> (<platform>; <arch>)
/// X-DashScope-CacheControl: enable
/// X-DashScope-UserAgent: QwenCode/<version> (<platform>; <arch>)
/// X-DashScope-AuthType: qwen-oauth
/// ```
///
/// The gateway fingerprints the full header set; sending any additional
/// `x-stainless-*` / SDK telemetry headers drops us into a tighter
/// rate-limit bucket, so we stick to these four.
pub fn qwen_extra_headers() -> Vec<(String, String)> {
    let ua = format!("QwenCode/{} ({})", QWEN_CLI_VERSION, user_agent_platform());
    vec![
        ("User-Agent".to_string(), ua.clone()),
        ("X-DashScope-CacheControl".to_string(), "enable".to_string()),
        ("X-DashScope-UserAgent".to_string(), ua),
        ("X-DashScope-AuthType".to_string(), "qwen-oauth".to_string()),
    ]
}

// ── DashScope body shape ──────────────────────────────────────────────────

/// Stable per-process session id, mirroring qwen-cli's `metadata.sessionId`.
/// DashScope tracks per-session quota, so reusing one id across the process
/// lifetime keeps us in a single bucket instead of looking like a fresh
/// client on every request.
pub(crate) fn qwen_session_id() -> &'static str {
    use std::sync::OnceLock;
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

/// Per-request id, mirroring qwen-cli's `metadata.promptId`. qwen-cli uses
/// a short hex string; we use 13 hex chars derived from a random u64.
pub(crate) fn qwen_prompt_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut x = nanos as u64 ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{:013x}", x & 0x000F_FFFF_FFFF_FFFF)
}

/// Vision-capable model identifiers recognized by qwen-cli's
/// `DashScopeOpenAICompatibleProvider.isVisionModel()`: exact match on
/// `coder-model`, or prefix on `qwen-vl`, `qwen3-vl-plus`, `qwen3.5-plus`.
pub(crate) fn is_vision_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    if m == "coder-model" {
        return true;
    }
    for prefix in ["qwen-vl", "qwen3-vl-plus", "qwen3.5-plus"] {
        if m.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Normalize an OpenAI `content` field to the array form qwen-cli uses
/// when attaching cache control. Mirrors `normalizeContentToArray`.
fn normalize_content_to_array(content: &serde_json::Value) -> Vec<serde_json::Value> {
    match content {
        serde_json::Value::String(s) => {
            vec![serde_json::json!({ "type": "text", "text": s })]
        }
        serde_json::Value::Array(arr) => arr.clone(),
        _ => Vec::new(),
    }
}

/// Heuristic: does this custom provider look like a Qwen / Alibaba target
/// that honours `cache_control: {type: "ephemeral"}` markers?
///
/// Triggers on EITHER signal:
/// - `base_url` contains a known Qwen / Alibaba host
///   (`dashscope`, `aliyun`, `aliyuncs`, `dialagram`)
/// - `model` name starts with `qwen` (strict prefix — we deliberately do
///   not match `tongyi`, `q3`, etc., to avoid false positives; users on
///   non-standard aliases can rename their model setting)
///
/// Adding `cache_control` markers to a non-Qwen backend is benign — OpenAI,
/// Gemini OpenAI-compat, Groq, DeepSeek, Cerebras, Together and others
/// silently ignore unknown JSON fields per the chat-completions spec.
/// Worst case: ~30 bytes of wasted JSON per request.
pub fn looks_like_qwen_target(base_url: &str, model: &str) -> bool {
    let model_match = model.to_ascii_lowercase().starts_with("qwen");
    is_dashscope_host(base_url) || model_match
}

/// Strict host-only form of [`looks_like_qwen_target`]: is this base_url a
/// DashScope / Alibaba Model Studio endpoint?
///
/// Used for request fields that are DashScope-specific wire contracts
/// (`preserve_thinking`, the family-gated thinking knobs) rather than the
/// benign `cache_control` markers. A locally served qwen GGUF matches on
/// model name but is not a DashScope endpoint, and must not receive them.
pub fn is_dashscope_host(base_url: &str) -> bool {
    let url = base_url.to_ascii_lowercase();
    url.contains("dashscope")
        || url.contains("aliyun")
        || url.contains("aliyuncs")
        || url.contains("bailian")
        || url.contains("dialagram")
}

/// Apply `cache_control: {type: "ephemeral"}` to the LAST part of the
/// content array. Mirrors `addCacheControlToContentArray`.
fn add_cache_control_to_content(content: &serde_json::Value) -> serde_json::Value {
    let mut arr = normalize_content_to_array(content);
    if let Some(last) = arr.last_mut()
        && let Some(obj) = last.as_object_mut()
    {
        obj.insert(
            "cache_control".to_string(),
            serde_json::json!({ "type": "ephemeral" }),
        );
    }
    serde_json::Value::Array(arr)
}

/// Cache-control the FIRST content part instead of the last. Used for a system
/// message split into `[stable_brain, runtime_suffix]` so the breakpoint sits
/// after the byte-stable brain and the volatile suffix stays uncached (#658).
fn add_cache_control_to_first_part(content: &serde_json::Value) -> serde_json::Value {
    let mut arr = normalize_content_to_array(content);
    if let Some(first) = arr.first_mut()
        && let Some(obj) = first.as_object_mut()
    {
        obj.insert(
            "cache_control".to_string(),
            serde_json::json!({ "type": "ephemeral" }),
        );
    }
    serde_json::Value::Array(arr)
}

/// Rewrite a serialized OpenAI chat-completions body into the exact dialect
/// that qwen-cli's `DashScopeOpenAICompatibleProvider.buildRequest` emits.
///
/// Transforms applied:
///   1. **Cache control** — `addDashScopeCacheControl(request, stream ? "all" : "system_only")`:
///      system message (if any) gets `cache_control: {type: "ephemeral"}`
///      on its last content part; when streaming, the LAST message
///      (regardless of role) and the LAST tool also get the same tag.
///   2. **metadata** — `{sessionId, promptId}` added at the top level.
///   3. **vl_high_resolution_images: true** — added only when the model
///      is in the vision list.
///   4. No field stripping. `temperature`, `top_p`, `tool_choice`, etc.
///      pass through. DashScope's fingerprint expects these to be present
///      when the client supplies them. `max_tokens` is never synthesized.
pub fn qwen_body_transform(mut body: serde_json::Value) -> serde_json::Value {
    let obj = match body.as_object_mut() {
        Some(o) => o,
        None => return body,
    };

    let is_streaming = obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    // ── 1. Cache control on messages ────────────────────────────────────
    if let Some(serde_json::Value::Array(messages)) = obj.get_mut("messages") {
        let msg_count = messages.len();
        if msg_count > 0 {
            let system_idx = messages
                .iter()
                .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"));
            let last_idx = msg_count - 1;

            for (i, msg) in messages.iter_mut().enumerate() {
                let should_cache = (Some(i) == system_idx) || (is_streaming && i == last_idx);
                if !should_cache {
                    continue;
                }
                let Some(msg_obj) = msg.as_object_mut() else {
                    continue;
                };
                let content = match msg_obj.get("content") {
                    Some(c) if !c.is_null() => c.clone(),
                    _ => continue,
                };
                // A system message split into [stable_brain, runtime_suffix]
                // caches on the FIRST part so the volatile suffix stays outside
                // the breakpoint (#658); everything else caches the last part.
                let is_split_system = Some(i) == system_idx
                    && content.as_array().map(|a| a.len() >= 2).unwrap_or(false);
                let new_content = if is_split_system {
                    add_cache_control_to_first_part(&content)
                } else {
                    add_cache_control_to_content(&content)
                };
                msg_obj.insert("content".to_string(), new_content);
            }
        }
    }

    // ── 2. Metadata ─────────────────────────────────────────────────────
    obj.insert(
        "metadata".to_string(),
        serde_json::json!({
            "sessionId": qwen_session_id(),
            "promptId": qwen_prompt_id(),
        }),
    );

    // ── 3. vl_high_resolution_images (only for vision models) ───────────
    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if is_vision_model(&model) {
        obj.insert(
            "vl_high_resolution_images".to_string(),
            serde_json::Value::Bool(true),
        );
    }

    // ── 4. Cache control on LAST tool (streaming only) ──────────────────
    if is_streaming
        && let Some(serde_json::Value::Array(tools)) = obj.get_mut("tools")
        && let Some(last) = tools.last_mut()
        && let Some(tool_obj) = last.as_object_mut()
    {
        tool_obj.insert(
            "cache_control".to_string(),
            serde_json::json!({ "type": "ephemeral" }),
        );
    }

    body
}
