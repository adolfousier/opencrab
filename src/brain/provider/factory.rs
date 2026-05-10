//! Provider Factory
//!
//! Creates providers based on config.toml settings.
//!
//! ## Registry Pattern
//!
//! All built-in providers are registered in the `REGISTRATIONS` array below.
//! Adding a new provider requires only:
//! 1. Adding a `ProviderRegistration` entry to `REGISTRATIONS`
//! 2. Adding the corresponding field to `ProviderConfigs` in `config/types.rs`
//! 3. Writing a `try_create_*` function
//!
//! The factory functions (`provider_enabled`,
//! `create_provider_by_name`, `create_fallback`, `active_provider_vision`)
//! all iterate the registry automatically.

use super::qwen::{qwen_body_transform, qwen_extra_headers};
use super::{
    Provider,
    anthropic::AnthropicProvider,
    claude_cli::ClaudeCliProvider,
    codex_cli::CodexCliProvider,
    custom_openai_compatible::{BodyTransformFn, OpenAIProvider},
    gemini::GeminiProvider,
    opencode_cli::OpenCodeCliProvider,
};
use crate::config::{Config, ProviderConfig};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

// ── Provider Registry ───────────────────────────────────────────

/// Type alias for async factory functions stored in the registry.
type ProviderFactoryFn = Box<
    dyn Fn(&Config) -> Pin<Box<dyn Future<Output = Result<Option<Arc<dyn Provider>>>> + Send + '_>>
        + Send
        + Sync,
>;

/// Type alias for sync factory functions (used by `sync_factory`).
type SyncProviderFactoryFn = fn(&Config) -> Result<Option<Arc<dyn Provider>>>;

/// Wrap a synchronous factory function into the async registry type.
fn sync_factory(f: SyncProviderFactoryFn) -> ProviderFactoryFn {
    Box::new(move |config| Box::pin(async move { f(config) }))
}

/// Registration entry for a built-in provider.
struct ProviderRegistration {
    /// Display name shown in pickers and logs (e.g. "Anthropic").
    display_name: &'static str,
    /// Session ID used for restoration (e.g. "anthropic").
    session_id: &'static str,
    /// Alternative session IDs (e.g. ["claude-cli", "claude_cli"]).
    aliases: &'static [&'static str],
    /// Check if this provider is enabled in config.
    is_enabled: fn(&Config) -> bool,
    /// Try to create the provider instance.
    factory: ProviderFactoryFn,
    /// Extract the provider config for vision/model lookups.
    config_field: fn(&Config) -> Option<&ProviderConfig>,
}

/// All built-in providers in priority order.
///
/// **IMPORTANT:** This array must stay in sync with `PROVIDER_NAMES`.
/// The index is used by `provider_enabled()`.
static REGISTRATIONS: LazyLock<Vec<ProviderRegistration>> = LazyLock::new(|| {
    vec![
        ProviderRegistration {
            display_name: "Claude CLI",
            session_id: "claude-cli",
            aliases: &["claude_cli"],
            is_enabled: |c| c.providers.claude_cli.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_claude_cli),
            config_field: |c| c.providers.claude_cli.as_ref(),
        },
        ProviderRegistration {
            display_name: "OpenCode CLI",
            session_id: "opencode-cli",
            aliases: &["opencode_cli"],
            is_enabled: |c| c.providers.opencode_cli.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_opencode_cli),
            config_field: |c| c.providers.opencode_cli.as_ref(),
        },
        ProviderRegistration {
            display_name: "Codex CLI",
            session_id: "codex-cli",
            aliases: &["codex_cli"],
            is_enabled: |c| c.providers.codex_cli.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_codex_cli),
            config_field: |c| c.providers.codex_cli.as_ref(),
        },
        ProviderRegistration {
            display_name: "OpenCode",
            session_id: "opencode",
            aliases: &["opencode_api"],
            is_enabled: |c| c.providers.opencode.as_ref().is_some_and(|p| p.enabled),
            factory: Box::new(|config| Box::pin(try_create_opencode(config))),
            config_field: |c| c.providers.opencode.as_ref(),
        },
        ProviderRegistration {
            display_name: "Qwen",
            session_id: "qwen",
            aliases: &[],
            is_enabled: |c| c.providers.qwen.as_ref().is_some_and(|p| p.enabled),
            factory: Box::new(|config| Box::pin(try_create_qwen(config))),
            config_field: |c| c.providers.qwen.as_ref(),
        },
        ProviderRegistration {
            display_name: "Anthropic",
            session_id: "anthropic",
            aliases: &[],
            is_enabled: |c| c.providers.anthropic.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_anthropic),
            config_field: |c| c.providers.anthropic.as_ref(),
        },
        ProviderRegistration {
            display_name: "OpenAI",
            session_id: "openai",
            aliases: &[],
            is_enabled: |c| c.providers.openai.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_openai),
            config_field: |c| c.providers.openai.as_ref(),
        },
        ProviderRegistration {
            display_name: "GitHub Copilot",
            session_id: "github",
            aliases: &[],
            is_enabled: |c| c.providers.github.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_github),
            config_field: |c| c.providers.github.as_ref(),
        },
        ProviderRegistration {
            display_name: "Google Gemini",
            session_id: "gemini",
            aliases: &[],
            is_enabled: |c| c.providers.gemini.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_gemini),
            config_field: |c| c.providers.gemini.as_ref(),
        },
        ProviderRegistration {
            display_name: "OpenRouter",
            session_id: "openrouter",
            aliases: &[],
            is_enabled: |c| c.providers.openrouter.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_openrouter),
            config_field: |c| c.providers.openrouter.as_ref(),
        },
        ProviderRegistration {
            display_name: "Minimax",
            session_id: "minimax",
            aliases: &[],
            is_enabled: |c| c.providers.minimax.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_minimax),
            config_field: |c| c.providers.minimax.as_ref(),
        },
        ProviderRegistration {
            display_name: "z.ai GLM",
            session_id: "zhipu",
            aliases: &[],
            is_enabled: |c| c.providers.zhipu.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_zhipu),
            config_field: |c| c.providers.zhipu.as_ref(),
        },
        ProviderRegistration {
            display_name: "Ollama",
            session_id: "ollama",
            aliases: &[],
            is_enabled: |c| c.providers.ollama.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_ollama),
            config_field: |c| c.providers.ollama.as_ref(),
        },
        ProviderRegistration {
            display_name: "Custom",
            session_id: "custom",
            aliases: &[],
            is_enabled: |c| c.providers.active_custom().is_some(),
            factory: sync_factory(try_create_custom),
            config_field: |_| None, // Custom uses active_custom() instead
        },
    ]
});

/// Provider names in priority order, derived from REGISTRATIONS.
pub const PROVIDER_NAMES: &[&str] = &[
    "Claude CLI",
    "OpenCode CLI",
    "Codex CLI",
    "OpenCode",
    "Qwen",
    "Anthropic",
    "OpenAI",
    "GitHub Copilot",
    "Google Gemini",
    "OpenRouter",
    "Minimax",
    "z.ai GLM",
    "Ollama",
    "Custom",
];

/// Whether a provider is enabled in config, by index matching PROVIDER_NAMES.
fn provider_enabled(config: &Config, idx: usize) -> bool {
    REGISTRATIONS
        .get(idx)
        .is_some_and(|reg| (reg.is_enabled)(config))
}

/// All built-in provider session_ids in priority order.
/// Used for cross-checking TUI provider ids against the factory registry.
pub fn provider_session_ids() -> Vec<&'static str> {
    REGISTRATIONS.iter().map(|r| r.session_id).collect()
}

// ── Local URL detection & thinking transform ────────────────────

/// Detect whether a base URL points to a local inference server
/// (llama.cpp, MLX, LM Studio, Ollama, etc.). Used to gate behaviours
/// that only make sense for self-hosted backends — specifically the
/// `chat_template_kwargs` injection for local Qwen, which cloud Qwen
/// (DashScope) rejects because it sets the reasoning flag server-side.
///
/// Matches loopback (`localhost`, `127.0.0.1`, `::1`, `0.0.0.0`),
/// mDNS (`*.local`), and the three RFC1918 private-network ranges
/// (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`). Public hosts
/// always return false so production endpoints keep their existing
/// request shape.
pub(crate) fn is_local_base_url(url: &str) -> bool {
    let after_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host_and_port = after_scheme.split(['/', '?']).next().unwrap_or("");
    // IPv6 addresses are bracketed: `[::1]:1234`. Strip brackets first so
    // the port-split logic doesn't mangle the address.
    let bare = if let Some(rest) = host_and_port.strip_prefix('[') {
        rest.split_once(']').map(|(h, _)| h).unwrap_or(rest)
    } else {
        host_and_port
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_and_port)
    };
    let bare = bare.to_ascii_lowercase();
    if bare == "localhost"
        || bare == "127.0.0.1"
        || bare == "0.0.0.0"
        || bare == "::1"
        || bare.ends_with(".local")
        || bare.starts_with("192.168.")
        || bare.starts_with("10.")
    {
        return true;
    }
    // 172.16.0.0 – 172.31.255.255
    let mut parts = bare.split('.');
    if parts.next() == Some("172")
        && let Some(second) = parts.next()
        && let Ok(n) = second.parse::<u8>()
        && (16..=31).contains(&n)
    {
        return true;
    }
    false
}

/// Build a body transform that injects
/// `chat_template_kwargs: {"enable_thinking": X}` into every request sent
/// to a local llama.cpp / MLX / LM Studio / Ollama server. Mirrors what
/// `llama-server --jinja --chat-template-kwargs '{"enable_thinking":true}'`
/// does — the flags Unsloth Studio launches with — so embedded GGUF/MLX
/// chat templates render `<tool_call>` tags and reasoning blocks the way
/// the model was trained on.
///
/// Generic across thinking-capable models (Qwen3, Kimi-K2, DeepSeek-R1
/// variants, etc.) — we inject when opted in, regardless of the model
/// name, because the mechanism is a llama.cpp jinja feature, not a
/// model-specific one. Callers only install this transform when the
/// user set `enable_thinking` in config (opt-in), so local models whose
/// template doesn't accept the variable stay untouched by default.
fn local_thinking_body_transform(enable: bool) -> BodyTransformFn {
    Arc::new(move |mut body: serde_json::Value| {
        if let Some(obj) = body.as_object_mut()
            && !obj.contains_key("chat_template_kwargs")
        {
            obj.insert(
                "chat_template_kwargs".to_string(),
                serde_json::json!({ "enable_thinking": enable }),
            );
        }
        body
    })
}

// ── Public factory functions ────────────────────────────────────

/// Create a provider based on config.toml
/// No hardcoded priority - providers are enabled/disabled in config
pub async fn create_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    let (provider, warning) = create_provider_with_warning(config).await?;
    if let Some(msg) = &warning {
        tracing::warn!("{}", msg);
    }
    Ok(provider)
}

/// Like `create_provider` but returns a warning message when a fallback was used.
/// The caller (TUI) should surface this to the user instead of printing to stderr.
pub async fn create_provider_with_warning(
    config: &Config,
) -> Result<(Arc<dyn Provider>, Option<String>)> {
    let mut primary: Option<Arc<dyn Provider>> = None;
    let mut failed_name: Option<&str> = None;
    let mut warning: Option<String> = None;

    // Try enabled providers in priority order
    for (i, reg) in REGISTRATIONS.iter().enumerate() {
        if !provider_enabled(config, i) {
            continue;
        }

        match (reg.factory)(config).await {
            Ok(Some(provider)) => {
                if let Some(failed) = failed_name {
                    let msg = format!(
                        "{} failed to initialize — fell back to {}. Run /onboard:provider to reconfigure.",
                        failed, reg.display_name
                    );
                    tracing::warn!("{}", msg);
                    warning = Some(msg);
                }
                tracing::info!("Using enabled provider: {}", reg.display_name);
                primary = Some(provider);
                break;
            }
            Ok(None) => {
                tracing::warn!(
                    "{} enabled but could not be created (missing API key?)",
                    reg.display_name
                );
                if failed_name.is_none() {
                    failed_name = Some(reg.display_name);
                }
            }
            Err(e) => {
                tracing::error!("{} provider error: {}", reg.display_name, e);
                if failed_name.is_none() {
                    failed_name = Some(reg.display_name);
                }
            }
        }
    }

    // Build fallback chain if configured (user-defined in config.toml)
    let fallback_providers = if let Some(fallback) = &config.providers.fallback
        && fallback.enabled
    {
        let chain = fallback_chain(fallback);
        let mut providers = Vec::new();
        for name in &chain {
            match create_fallback(config, name).await {
                Ok(p) => {
                    tracing::info!("Fallback provider '{}' ready", name);
                    providers.push(p);
                }
                Err(e) => {
                    tracing::warn!("Fallback provider '{}' skipped: {}", name, e);
                }
            }
        }
        providers
    } else {
        Vec::new()
    };

    match primary {
        Some(provider) => {
            if fallback_providers.is_empty() {
                Ok((provider, warning))
            } else {
                tracing::info!(
                    "Wrapping primary provider with {} fallback(s)",
                    fallback_providers.len()
                );
                Ok((
                    Arc::new(super::FallbackProvider::new_with_health(
                        provider,
                        fallback_providers,
                    )),
                    warning,
                ))
            }
        }
        None => {
            // No primary — try fallbacks as primary candidates
            if let Some(first) = fallback_providers.into_iter().next() {
                tracing::warn!("No primary provider enabled, using first fallback");
                Ok((first, warning))
            } else {
                tracing::info!("No provider configured, using placeholder provider");
                Ok((Arc::new(super::PlaceholderProvider), warning))
            }
        }
    }
}

/// Create a provider by name, ignoring the `enabled` flag.
/// Used for per-session provider restoration without toggling disk config.
/// Accepts names like "anthropic", "openai", "minimax", "openrouter", or "custom:<name>".
pub async fn create_provider_by_name(config: &Config, name: &str) -> Result<Arc<dyn Provider>> {
    // Custom entries take precedence over built-in names. If the user
    // created a custom provider literally named "opencode" / "anthropic"
    // / anything that collides with a built-in id, the custom entry wins.
    if !name.starts_with("custom:")
        && config
            .providers
            .custom
            .as_ref()
            .is_some_and(|m| m.contains_key(name))
        && let Some(p) = try_create_custom_by_name(config, name)?
    {
        return Ok(p);
    }

    // Try built-in registry by session_id or alias
    for reg in REGISTRATIONS.iter() {
        if reg.session_id == name || reg.aliases.contains(&name) {
            let provider = (reg.factory)(config).await?.ok_or_else(|| {
                anyhow::anyhow!("{} not configured (missing API key)", reg.display_name)
            })?;
            return Ok(provider);
        }
    }

    // Try custom: prefix
    if let Some(custom_name) = name.strip_prefix("custom:") {
        return try_create_custom_by_name(config, custom_name)?
            .ok_or_else(|| anyhow::anyhow!("Custom provider '{}' not configured", custom_name));
    }

    // Try as a custom provider name directly (legacy sessions)
    try_create_custom_by_name(config, name)?
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", name))
}

/// Try to create a specific named custom provider (ignores enabled flag).
fn try_create_custom_by_name(config: &Config, name: &str) -> Result<Option<Arc<dyn Provider>>> {
    let customs = match &config.providers.custom {
        Some(map) => map,
        None => return Ok(None),
    };

    let custom_config = match customs.get(name) {
        Some(cfg) => cfg.clone(),
        None => {
            tracing::warn!("Custom provider '{}' not found in config", name);
            return Ok(None);
        }
    };

    // API key is optional for local providers (LM Studio, Ollama, etc.)
    let api_key = custom_config.api_key.clone().unwrap_or_default();

    // base_url is REQUIRED. Silently defaulting to localhost:1234 (LM Studio)
    // produced channel-side errors like "failed to connect to localhost:1234"
    // whenever a session had a stale/missing custom provider entry — the user
    // saw the bot trying to reach a server they weren't running. Refuse to
    // construct the provider without an explicit base_url so the caller can
    // fall through to the next option (e.g. the global active provider).
    let Some(mut base_url) = custom_config.base_url.clone() else {
        tracing::warn!(
            "Custom provider '{}' has no base_url configured — skipping (run /onboard:provider)",
            name
        );
        return Ok(None);
    };

    if !base_url.contains("/chat/completions") {
        base_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    }

    // Redact the key for logs but confirm merger ran. An empty key here
    // means keys.toml wasn't merged into this custom entry — the request
    // will then go out with `Bearer ` (empty) and always 401/403. Surface
    // that loudly instead of silently constructing a provider that can't
    // authenticate.
    let key_status = if api_key.is_empty() {
        "MISSING".to_string()
    } else if api_key == "__EXISTING_KEY__" {
        "SENTINEL(__EXISTING_KEY__ never merged)".to_string()
    } else {
        format!("present (len={})", api_key.len())
    };
    if api_key.is_empty() || api_key == "__EXISTING_KEY__" {
        tracing::warn!(
            "Custom provider '{}' being constructed without a real api_key ({}). \
             Requests will fail auth. Check keys.toml has [providers.custom.{}] api_key = \"...\" \
             and that the key isn't the literal sentinel string.",
            name,
            key_status,
            name,
        );
    }
    tracing::info!(
        "Creating custom provider '{}' at: {} (api_key: {})",
        name,
        base_url,
        key_status,
    );
    let mut builder =
        OpenAIProvider::with_base_url(api_key.clone(), base_url.clone()).with_name(name);
    if is_local_base_url(&base_url) {
        let enable = custom_config.enable_thinking.unwrap_or(true);
        builder = builder.with_body_transform(local_thinking_body_transform(enable));
    }
    let provider = configure_openai_compatible(builder, &custom_config);
    Ok(Some(Arc::new(provider)))
}

/// Build ordered fallback chain: `providers` array first, legacy `provider` as last resort.
pub(crate) fn fallback_chain(fallback: &crate::config::FallbackProviderConfig) -> Vec<String> {
    let mut chain: Vec<String> = fallback.providers.clone();
    // Append legacy single `provider` if set and not already in the list
    if let Some(ref legacy) = fallback.provider
        && !chain.iter().any(|p| p == legacy)
    {
        chain.push(legacy.clone());
    }
    chain
}

/// Create fallback provider
async fn create_fallback(config: &Config, fallback_type: &str) -> Result<Arc<dyn Provider>> {
    // Custom entries take precedence over built-in names. If the user
    // created a custom provider literally named "opencode" / "ollama" /
    // anything that collides with a built-in id, the custom entry wins.
    // (Same priority as create_provider_by_name.)
    if !fallback_type.starts_with("custom:")
        && config
            .providers
            .custom
            .as_ref()
            .is_some_and(|m| m.contains_key(fallback_type))
    {
        tracing::info!("Using fallback: Custom '{}'", fallback_type);
        return try_create_custom_by_name(config, fallback_type)?
            .ok_or_else(|| anyhow::anyhow!("Custom provider '{}' not configured", fallback_type));
    }

    // Try custom: prefix
    if let Some(custom_name) = fallback_type.strip_prefix("custom:") {
        tracing::info!("Using fallback: Custom '{}'", custom_name);
        return try_create_custom_by_name(config, custom_name)?
            .ok_or_else(|| anyhow::anyhow!("Custom provider '{}' not configured", custom_name));
    }

    // Try built-in registry
    for reg in REGISTRATIONS.iter() {
        if reg.session_id == fallback_type || reg.aliases.contains(&fallback_type) {
            tracing::info!("Using fallback: {}", reg.display_name);
            return (reg.factory)(config)
                .await?
                .ok_or_else(|| anyhow::anyhow!("{} not configured", reg.display_name));
        }
    }

    Err(anyhow::anyhow!(
        "Unknown fallback provider: {}",
        fallback_type
    ))
}

/// Returns `(api_key, base_url, vision_model)` for the first active provider
/// that has a `vision_model` configured. Used to register the provider-native
/// `analyze_image` tool when Gemini vision isn't set up.
pub fn active_provider_vision(config: &Config) -> Option<(String, String, String)> {
    // Get the ACTUAL active provider — don't iterate all providers,
    // otherwise MiniMax wins just because it's listed before OpenRouter.
    let (name, _) = config.providers.active_provider_and_model();

    // Custom entries take precedence over built-in names. If the user
    // created a custom provider literally named "opencode" / "ollama" /
    // anything that collides with a built-in id, the custom entry wins.
    let custom_cfg = if !name.starts_with("custom:")
        && config
            .providers
            .custom
            .as_ref()
            .is_some_and(|m| m.contains_key(name.as_str()))
    {
        config
            .providers
            .custom
            .as_ref()
            .and_then(|m| m.get(&name))
            .cloned()
    } else if let Some(custom_name) = name.strip_prefix("custom:") {
        config
            .providers
            .custom
            .as_ref()
            .and_then(|m| m.get(custom_name))
            .cloned()
    } else {
        None
    };

    // Try built-in registry
    let native_cfg: Option<&ProviderConfig> = REGISTRATIONS
        .iter()
        .find(|reg| reg.session_id == name || reg.aliases.contains(&name.as_str()))
        .and_then(|reg| (reg.config_field)(config));

    // Custom wins if it exists (same priority as create_provider_by_name)
    let active_cfg = custom_cfg.as_ref().or(native_cfg);

    if let Some(cfg) = active_cfg
        && let (Some(api_key), Some(vision_model)) = (&cfg.api_key, &cfg.vision_model)
    {
        let base_url = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());
        let base_url = if base_url.contains("/chat/completions") {
            base_url
        } else {
            format!("{}/chat/completions", base_url.trim_end_matches('/'))
        };
        return Some((api_key.clone(), base_url, vision_model.clone()));
    }
    // No provider-native vision — let cli/ui.rs fall back to Gemini if configured
    None
}

// ── Individual provider factory functions ───────────────────────

/// Try to create GitHub Copilot provider if configured.
/// Uses OAuth token to exchange for short-lived Copilot API tokens.
fn try_create_github(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    use super::copilot::{COPILOT_CHAT_URL, CopilotTokenManager, copilot_extra_headers};

    let github_config = match &config.providers.github {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let oauth_token = github_config.api_key.clone().filter(|k| !k.is_empty());

    let Some(oauth_token) = oauth_token else {
        tracing::warn!(
            "GitHub Copilot enabled but no OAuth token found. \
             Run /onboard:provider to authenticate."
        );
        return Ok(None);
    };

    // Create the token manager — background task does the initial + recurring refresh
    let manager = Arc::new(CopilotTokenManager::new(oauth_token));
    manager.clone().start_background_refresh();

    // Build a token_fn closure that reads the cached Copilot token
    let mgr_clone = manager.clone();
    let token_fn: super::custom_openai_compatible::TokenFn =
        Arc::new(move || mgr_clone.get_cached_token());

    let base_url = github_config
        .base_url
        .clone()
        .unwrap_or_else(|| COPILOT_CHAT_URL.to_string());

    tracing::info!("Using GitHub Copilot at: {}", base_url);

    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url("copilot-managed".to_string(), base_url)
            .with_name("GitHub Copilot")
            .with_token_fn(token_fn)
            .with_extra_headers(copilot_extra_headers()),
        github_config,
    );
    Ok(Some(Arc::new(provider)))
}

/// Default DashScope OpenAI-compatible chat-completions URL (China region).
/// Users can override via `[providers.qwen].base_url` for Singapore
/// (`dashscope-intl`), US (`dashscope-us`), or Coding Plan
/// (`coding.dashscope.aliyuncs.com/v1`).
const QWEN_DEFAULT_DASHSCOPE_URL: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";

/// Try to create the **Qwen** provider backed by a DashScope API key.
///
/// OAuth and multi-account rotation were removed after Alibaba discontinued
/// Qwen OAuth. The provider now behaves like any other OpenAI-compatible
/// key-based provider (zhipu, openai, …): `api_key` + `base_url` +
/// `default_model` from `[providers.qwen]`.
async fn try_create_qwen(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let qwen_config = match &config.providers.qwen {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let Some(api_key) = qwen_config.api_key.as_ref().filter(|k| !k.is_empty()) else {
        tracing::warn!("Qwen enabled but no API key configured — run /onboard:provider");
        return Ok(None);
    };

    let base_url = qwen_config
        .base_url
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| QWEN_DEFAULT_DASHSCOPE_URL.to_string());
    let base_url = if base_url.contains("/chat/completions") {
        base_url
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    };

    tracing::info!("Using Qwen (DashScope) at: {}", base_url);

    let qwen_limiter = Arc::clone(&super::rate_limiter::QWEN_OAUTH_LIMITER);

    let enable_thinking = qwen_config.enable_thinking;
    let builder = OpenAIProvider::with_base_url(api_key.clone(), base_url)
        .with_name("qwen")
        .with_extra_headers(qwen_extra_headers())
        .with_body_transform(Arc::new(move |body| {
            let mut body = qwen_body_transform(body);
            if let Some(enable) = enable_thinking
                && let Some(obj) = body.as_object_mut()
            {
                obj.insert(
                    "enable_thinking".to_string(),
                    serde_json::Value::Bool(enable),
                );
            }
            body
        }))
        .with_rate_limiter(qwen_limiter);

    let provider = configure_openai_compatible(builder, qwen_config);
    Ok(Some(Arc::new(provider)))
}

/// Try to create OpenRouter provider if configured
fn try_create_openrouter(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let openrouter_config = match &config.providers.openrouter {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let Some(api_key) = &openrouter_config.api_key else {
        tracing::warn!("OpenRouter enabled but API key missing — check keys.toml");
        return Ok(None);
    };

    let base_url = openrouter_config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://openrouter.ai/api/v1/chat/completions".to_string());

    let model = openrouter_config
        .default_model
        .clone()
        .unwrap_or_else(|| "openai/gpt-4o".to_string());

    let is_free = model.ends_with(":free");

    tracing::info!(
        "Using OpenRouter at: {} (model={}, free={})",
        base_url,
        model,
        is_free
    );
    let mut provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), base_url)
            .with_name("openrouter")
            .with_default_model(model.clone())
            .with_extra_headers(vec![
                ("X-Title".to_string(), "Open Crabs".to_string()),
                (
                    "HTTP-Referer".to_string(),
                    "https://opencrabs.com".to_string(),
                ),
            ]),
        openrouter_config,
    );

    // Proactive pacing for :free models — per-model rate limiter shared across
    // all provider instances. Enforces 4s between requests per model (~15 req/min,
    // safely under OpenRouter's 20 req/min window with 25% headroom).
    if is_free {
        let limiter = super::rate_limiter::OPENROUTER_FREE_LIMITERS.get(&model);
        provider = provider.with_rate_limiter(limiter);
        tracing::info!("Rate limiter attached for :free model: {}", model);
    }

    Ok(Some(Arc::new(provider)))
}

/// Try to create Minimax provider if configured
fn try_create_minimax(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let minimax_config = match &config.providers.minimax {
        Some(cfg) => {
            tracing::debug!(
                "Minimax config: enabled={}, has_key={}",
                cfg.enabled,
                cfg.api_key.is_some()
            );
            cfg
        }
        None => return Ok(None),
    };

    let Some(api_key) = &minimax_config.api_key else {
        tracing::warn!("Minimax enabled but API key missing — check keys.toml");
        return Ok(None);
    };

    // MiniMax requires specific endpoint path, not just /v1
    let base_url = minimax_config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.minimax.io/v1".to_string());

    // Append correct path if not already present
    let full_url = if base_url.contains("minimax.io") && !base_url.contains("/text/") {
        format!("{}/text/chatcompletion_v2", base_url.trim_end_matches('/'))
    } else {
        base_url
    };

    tracing::info!("Using Minimax at: {}", full_url);
    let mut provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), full_url).with_name("minimax"),
        minimax_config,
    );

    // MiniMax M2.7/M2.5 doesn't support vision — default to MiniMax-Text-01 in-memory.
    // Do NOT write to config here — this runs inside ConfigWatcher callbacks and
    // writing triggers another reload → infinite loop → crash.
    if minimax_config.vision_model.is_none() {
        provider = provider.with_vision_model("MiniMax-Text-01".to_string());
    }

    Ok(Some(Arc::new(provider)))
}

/// Try to create z.ai GLM provider if configured
/// Supports two endpoint types: "api" (general) or "coding" (coding-specific)
fn try_create_zhipu(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let zhipu_config = match &config.providers.zhipu {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let Some(api_key) = &zhipu_config.api_key else {
        tracing::warn!("z.ai GLM enabled but API key missing — check keys.toml");
        return Ok(None);
    };

    // Determine base URL based on endpoint_type
    // API endpoint: https://api.z.ai/api/paas/v4
    // Coding endpoint: https://api.z.ai/api/coding/paas/v4
    let base_url = match zhipu_config.endpoint_type.as_deref() {
        Some("coding") => "https://api.z.ai/api/coding/paas/v4/chat/completions",
        _ => "https://api.z.ai/api/paas/v4/chat/completions",
    };

    tracing::info!(
        "Using z.ai GLM at: {} (endpoint_type: {:?})",
        base_url,
        zhipu_config.endpoint_type
    );
    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), base_url.to_string()).with_name("zhipu"),
        zhipu_config,
    );
    Ok(Some(Arc::new(provider)))
}

/// Try to create Ollama provider if configured.
/// Supports both local (localhost:11434, no API key) and cloud (api.ollama.com, optional key).
fn try_create_ollama(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let ollama_config = match &config.providers.ollama {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    // Default to local Ollama if no base_url specified
    let base_url = ollama_config
        .base_url
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://localhost:11434/v1/chat/completions".to_string());

    let base_url = if base_url.contains("/chat/completions") {
        base_url
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    };

    // API key is optional for local Ollama
    let api_key = ollama_config.api_key.clone().unwrap_or_default();

    tracing::info!(
        "Using Ollama at: {} (has_key={})",
        base_url,
        !api_key.is_empty()
    );

    let mut builder = OpenAIProvider::with_base_url(api_key, base_url.clone()).with_name("ollama");
    if is_local_base_url(&base_url) {
        let enable = ollama_config.enable_thinking.unwrap_or(true);
        builder = builder.with_body_transform(local_thinking_body_transform(enable));
    }
    let provider = configure_openai_compatible(builder, ollama_config);
    Ok(Some(Arc::new(provider)))
}

/// Try to create Custom OpenAI-compatible provider if configured.
/// Picks the first enabled named custom provider from the map.
fn try_create_custom(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let (name, custom_config) = match config.providers.active_custom() {
        Some((n, c)) => (n.to_string(), c.clone()),
        None => {
            tracing::warn!("Custom provider requested but no active custom provider found");
            return Ok(None);
        }
    };

    // API key is optional for local providers (LM Studio, Ollama, etc.)
    let api_key = custom_config.api_key.clone().unwrap_or_default();

    // Same rationale as try_create_custom_by_name: refuse to silently default
    // to localhost:1234 so Discord / other channels don't hit a dead
    // LM Studio URL the user never configured.
    let Some(mut base_url) = custom_config.base_url.clone() else {
        tracing::warn!(
            "Custom provider '{}' has no base_url configured — skipping (run /onboard:provider)",
            name
        );
        return Ok(None);
    };

    // Auto-append /chat/completions if missing — all OpenAI-compatible APIs need it
    if !base_url.contains("/chat/completions") {
        base_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    }

    tracing::info!(
        "Using Custom OpenAI-compatible '{}' at: {} (has_key={})",
        name,
        base_url,
        !api_key.is_empty()
    );
    let mut builder = OpenAIProvider::with_base_url(api_key, base_url.clone()).with_name(&name);
    if is_local_base_url(&base_url) {
        let enable = custom_config.enable_thinking.unwrap_or(true);
        builder = builder.with_body_transform(local_thinking_body_transform(enable));
    }
    let provider = configure_openai_compatible(builder, &custom_config);
    Ok(Some(Arc::new(provider)))
}

/// Configure OpenAI-compatible provider with custom model
fn configure_openai_compatible(
    mut provider: OpenAIProvider,
    config: &ProviderConfig,
) -> OpenAIProvider {
    tracing::debug!(
        "configure_openai_compatible: default_model = {:?}",
        config.default_model
    );
    if let Some(model) = &config.default_model {
        tracing::info!("Using custom default model: {}", model);
        provider = provider.with_default_model(model.clone());
    }
    if let Some(vm) = &config.vision_model {
        tracing::info!("Vision model configured: {}", vm);
        provider = provider.with_vision_model(vm.clone());
    }
    if let Some(cw) = config.context_window {
        tracing::info!("Context window configured: {} tokens", cw);
        provider = provider.with_context_window(cw);
    }
    if !config.models.is_empty() {
        tracing::debug!(
            "Loaded {} configured models for provider",
            config.models.len()
        );
        provider = provider.with_models(config.models.clone());
    }
    // OpenRouter response caching
    if let Some(cache) = config.cache_enabled {
        provider = provider.with_cache_enabled(cache);
        if cache {
            tracing::info!("OpenRouter response caching enabled");
        }
    }
    if let Some(ttl) = config.cache_ttl {
        provider = provider.with_cache_ttl(ttl);
        tracing::info!("OpenRouter cache TTL: {}s", ttl);
    }
    provider
}

/// Try to create OpenAI provider if configured
fn try_create_openai(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let openai_config = match &config.providers.openai {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    // Local LLM (LM Studio, Ollama, etc.) - has base_url but NO api_key
    if let Some(base_url) = &openai_config.base_url
        && openai_config.api_key.is_none()
    {
        tracing::info!("Using local LLM at: {}", base_url);
        let mut builder = OpenAIProvider::local(base_url.clone()).with_name("openai");
        if is_local_base_url(base_url) {
            let enable = openai_config.enable_thinking.unwrap_or(true);
            builder = builder.with_body_transform(local_thinking_body_transform(enable));
        }
        let provider = configure_openai_compatible(builder, openai_config);
        return Ok(Some(Arc::new(provider)));
    }

    // Official OpenAI API - has api_key
    if let Some(api_key) = &openai_config.api_key {
        tracing::info!("Using OpenAI provider");
        let provider = configure_openai_compatible(
            OpenAIProvider::new(api_key.clone()).with_name("openai"),
            openai_config,
        );
        return Ok(Some(Arc::new(provider)));
    }

    tracing::warn!("OpenAI enabled but no API key and no base_url — check keys.toml");
    Ok(None)
}

/// Try to create Gemini provider if configured
fn try_create_gemini(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let gemini_config = match &config.providers.gemini {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let api_key = match &gemini_config.api_key {
        Some(key) if !key.is_empty() => key.clone(),
        _ => {
            tracing::warn!("Gemini enabled but API key missing — check keys.toml");
            return Ok(None);
        }
    };

    let model = gemini_config
        .default_model
        .clone()
        .unwrap_or_else(|| "gemini-2.0-flash".to_string());

    tracing::info!("Using Gemini provider with model: {}", model);
    Ok(Some(Arc::new(
        GeminiProvider::new(api_key).with_model(model),
    )))
}

/// Try to create Claude CLI provider if configured and binary is available.
fn try_create_claude_cli(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let cli_config = match &config.providers.claude_cli {
        Some(cfg) if cfg.enabled => cfg,
        _ => return Ok(None),
    };

    match ClaudeCliProvider::new() {
        Ok(mut provider) => {
            if let Some(model) = &cli_config.default_model {
                provider = provider.with_default_model(model.clone());
            }
            tracing::info!("Using Claude CLI provider (Max subscription, no API key needed)");
            Ok(Some(Arc::new(provider)))
        }
        Err(e) => {
            tracing::warn!("Claude CLI enabled but binary not found: {}", e);
            Ok(None)
        }
    }
}

/// Try to create OpenCode CLI provider if configured and binary is available.
fn try_create_opencode_cli(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let cli_config = match &config.providers.opencode_cli {
        Some(cfg) if cfg.enabled => cfg,
        _ => return Ok(None),
    };

    match OpenCodeCliProvider::new() {
        Ok(mut provider) => {
            if let Some(model) = &cli_config.default_model {
                provider = provider.with_default_model(model.clone());
            }
            tracing::info!("Using OpenCode CLI provider (free models, no API key needed)");
            Ok(Some(Arc::new(provider)))
        }
        Err(e) => {
            tracing::warn!("OpenCode CLI enabled but binary not found: {}", e);
            Ok(None)
        }
    }
}

/// Try to create Codex CLI provider if configured and binary is available.
fn try_create_codex_cli(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let cli_config = match &config.providers.codex_cli {
        Some(cfg) if cfg.enabled => cfg,
        _ => return Ok(None),
    };

    match CodexCliProvider::new() {
        Ok(mut provider) => {
            if let Some(model) = &cli_config.default_model {
                provider = provider.with_default_model(model.clone());
            }
            tracing::info!(
                "Using Codex CLI provider (ChatGPT/Codex subscription, no API key needed)"
            );
            Ok(Some(Arc::new(provider)))
        }
        Err(e) => {
            tracing::warn!("Codex CLI enabled but binary not found: {}", e);
            Ok(None)
        }
    }
}

/// Try to create OpenCode API provider if configured (Go/Zen plans)
async fn try_create_opencode(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let opencode_config = match &config.providers.opencode {
        Some(cfg) if cfg.enabled => cfg,
        _ => return Ok(None),
    };

    let Some(api_key) = &opencode_config.api_key else {
        tracing::warn!("OpenCode enabled but API key missing — check keys.toml");
        return Ok(None);
    };

    let base_url = opencode_config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://opencode.ai/zen/go/v1/chat/completions".to_string());

    let model = opencode_config
        .default_model
        .clone()
        .unwrap_or_else(|| "qwen3.6-plus".to_string());

    tracing::info!("Using OpenCode API at: {} (model={})", base_url, model);

    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), base_url)
            .with_name("opencode")
            .with_default_model(model.clone()),
        opencode_config,
    );

    Ok(Some(Arc::new(provider)))
}

/// Try to create Anthropic provider if configured
fn try_create_anthropic(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let anthropic_config = match &config.providers.anthropic {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let api_key = match &anthropic_config.api_key {
        Some(key) => key.clone(),
        None => {
            tracing::warn!("Anthropic enabled but API key missing — check keys.toml");
            return Ok(None);
        }
    };

    let mut provider = AnthropicProvider::new(api_key);

    if let Some(model) = &anthropic_config.default_model {
        tracing::info!("Using custom default model: {}", model);
        provider = provider.with_default_model(model.clone());
    }

    tracing::info!("Using Anthropic provider");

    Ok(Some(Arc::new(provider)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ProviderConfig, ProviderConfigs};

    #[tokio::test]
    async fn test_create_provider_with_anthropic() {
        let config = Config {
            providers: ProviderConfigs {
                anthropic: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("test-key".to_string()),
                    base_url: None,
                    default_model: None,
                    models: vec![],
                    vision_model: None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config).await;
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.name(), "anthropic");
    }

    #[tokio::test]
    async fn test_create_provider_with_minimax() {
        let config = Config {
            providers: ProviderConfigs {
                minimax: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("test-key".to_string()),
                    base_url: Some("https://api.minimax.io/v1".to_string()),
                    default_model: Some("MiniMax-M2.7".to_string()),
                    models: vec![],
                    vision_model: None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_minimax_takes_priority() {
        let config = Config {
            providers: ProviderConfigs {
                openai: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("openai-key".to_string()),
                    base_url: None,
                    default_model: None,
                    models: vec![],
                    vision_model: None,
                    ..Default::default()
                }),
                minimax: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("minimax-key".to_string()),
                    base_url: Some("https://api.minimax.io/v1".to_string()),
                    default_model: None,
                    models: vec![],
                    vision_model: None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_provider_no_credentials() {
        let config = Config {
            providers: ProviderConfigs::default(),
            ..Default::default()
        };

        // No credentials → PlaceholderProvider (app starts, shows onboarding)
        let result = create_provider(&config).await;
        assert!(result.is_ok());
    }
}
