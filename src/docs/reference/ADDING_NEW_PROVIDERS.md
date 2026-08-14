# Adding New Providers

This document describes how to add a new AI provider to OpenCrabs.

## Overview

OpenCrabs supports multiple AI providers through a registry-based architecture. Each provider is configured via `config.toml` and appears in the onboarding UI.

## Provider Architecture

### 1. Config (`src/config/types.rs`)

Add provider config field to `ProviderConfigs` struct (search for `pub struct ProviderConfigs`):

```rust
pub struct ProviderConfigs {
    // ... existing providers
    pub yourprovider: Option<ProviderConfig>,  // NEW
}
```

The `ProviderConfig` struct already has all the fields you need:

```rust
pub struct ProviderConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub models: Vec<String>,                      // For providers without /models endpoint
    pub vision_model: Option<String>,             // Vision-capable model override
    pub generation_model: Option<String>,         // Image generation model override
    pub context_window: Option<u32>,              // Context window size in tokens
    pub endpoint_type: Option<String>,            // For providers with multiple API modes
    pub voice: Option<String>,                    // TTS voice name (voice providers only)
    pub model: Option<String>,                    // TTS/image model override (voice/image providers)
    pub enable_thinking: Option<bool>,            // Thinking-mode switch for reasoning models
    pub cache_enabled: Option<bool>,              // Response caching (OpenRouter)
    pub cache_ttl: Option<u32>,                   // Cache TTL in seconds
    pub use_max_completion_tokens: Option<bool>,  // Force max_completion_tokens field
}
```

> Verify the field set against the live struct in `src/config/types.rs` (search for `pub struct ProviderConfig`) before relying on this list. New fields are added over time. A chat provider only needs `enabled`, `api_key`, `base_url`, `default_model`, and optionally `models`/`context_window`/`endpoint_type`/`enable_thinking`; the rest are for vision/image/TTS/cache features.

### 2. Provider Factory (`src/brain/provider/factory.rs`)

The factory uses a **registry pattern**. All providers are registered in the `REGISTRATIONS` array (LazyLock<Vec<ProviderRegistration>>).

#### Step 1: Add to REGISTRATIONS array (search for `static REGISTRATIONS`)

```rust
static REGISTRATIONS: LazyLock<Vec<ProviderRegistration>> = LazyLock::new(|| {
    vec![
        // ... existing providers
        ProviderRegistration {
            display_name: "YourProvider",
            session_id: "yourprovider",
            aliases: &[],  // Alternative session IDs for backward compatibility
            is_enabled: |c| c.providers.yourprovider.as_ref().is_some_and(|p| p.enabled),
            factory: sync_factory(try_create_yourprovider),  // or Box::new for async
            config_field: |c| c.providers.yourprovider.as_ref(),
        },
    ]
});
```

**Priority order matters!** Providers are tried in array order when multiple are enabled. CLI-based providers (Claude CLI, OpenCode CLI, Codex CLI, etc.) are placed first because they don't require API keys and should take precedence when available. API-based providers follow.

**Note:** The onboarding UI (`PROVIDERS` array) uses a different order optimized for user experience (API providers first, CLI providers later). The factory order is about runtime priority; the onboarding order is about UI presentation.

#### Step 2: Add to PROVIDER_NAMES array (search for `pub const PROVIDER_NAMES`)

```rust
pub const PROVIDER_NAMES: &[&str] = &[
    // ... existing providers
    "YourProvider",
];
```

**Must stay in sync with REGISTRATIONS array order!**

#### Step 3: Write the factory function

For most providers (OpenAI-compatible):

```rust
fn try_create_yourprovider(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let yourprovider_config = match &config.providers.yourprovider {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let Some(api_key) = &yourprovider_config.api_key else {
        tracing::warn!("YourProvider enabled but API key missing — check keys.toml");
        return Ok(None);
    };

    let base_url = yourprovider_config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.yourprovider.com/v1/chat/completions".to_string());

    let base_url = if base_url.contains("/chat/completions") {
        base_url
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    };

    tracing::info!("Using YourProvider at: {}", base_url);
    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), base_url).with_name("yourprovider"),
        yourprovider_config,
    );
    Ok(Some(Arc::new(provider)))
}
```

For async factory functions (e.g., OAuth-based providers):

```rust
async fn try_create_yourprovider(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    // ... async logic
}

// In REGISTRATIONS:
factory: Box::new(|config| Box::pin(try_create_yourprovider(config))),
```

### 3. Onboarding UI (`src/tui/onboarding/types.rs`)

Add provider to `PROVIDERS` array (around line 147):

```rust
pub const PROVIDERS: &[ProviderInfo] = &[
    // ... existing providers
    ProviderInfo {
        id: "yourprovider",  // Must match session_id in factory
        name: "YourProvider",
        models: &[],  // Empty = fetched from API at runtime
        key_label: "API Key",
        help_lines: &[
            "Get key from yourprovider.com",
            "Or paste from your dashboard",
        ],
    },
];
```

**ProviderInfo fields:**
- `id`: Canonical provider ID matching `session_id` in factory (empty string for Custom)
- `name`: Display name shown in UI
- `models`: Static model list (empty = fetched from `/v1/models` endpoint)
- `key_label`: Label for API key input field
- `help_lines`: Help text shown below the input

**Order matters for UI presentation!** The onboarding UI shows providers in this order. Place your provider logically:
- **API-based providers** (require API keys) go first — these are the most common
- **CLI-based providers** (use local subprocesses) go in the middle
- **Custom OpenAI-Compatible** is always last (dynamic, uses runtime names)

**Note:** This order is independent of the factory `REGISTRATIONS` array order. The factory prioritizes CLI providers first (they don't need API keys), while onboarding shows API providers first (better UX for most users).

### 3.5 Provider Name Registry (`src/utils/providers.rs`) — REQUIRED for built-ins

This is the easiest step to forget, and skipping it silently breaks name resolution (the `/models` picker, footer display, session restore, config lookup). A new built-in provider MUST be added in two places here:

1. **`KNOWN_PROVIDERS`** (search for `pub const KNOWN_PROVIDERS`) — add a `ProviderMeta` entry with the canonical id, display name, aliases, and config section. `find_provider_meta` / `normalize_provider_name` / `display_name` all read this.

2. **`config_for()`** (search for `pub fn config_for`) — add an explicit match arm mapping your provider id to its config field:

```rust
Some("yourprovider") => providers.yourprovider.as_ref(),
```

Without the `config_for` arm, `provider_config_models()` and friends return `None` for your provider, so the `/models` picker shows no models and the footer can't resolve the pair.

> Custom OpenAI-compatible providers are exempt: they resolve through the `custom:` prefix path, not these arms.

### 3.6 Key Merge (`src/config/types.rs` `merge_provider_keys`) — REQUIRED for built-ins

API keys live in `keys.toml`, separate from `config.toml`. `merge_provider_keys()` folds them into the in-memory `ProviderConfig` at load time. A new built-in provider needs two edits here:

1. The keys-source mapping array (search for `("providers.minimax"` to find it) — add `("providers.yourprovider", keys.yourprovider.as_ref())`.
2. The merge body (search for `pub(crate) fn merge_provider_keys`) — add an arm that copies the key (and any auto-enable logic, like `qwen`/`opencode` do) into `config.providers.yourprovider`.

Skip this and your provider's `api_key` stays `None` even with a valid `keys.toml`, so the factory's `try_create_*` bails with "API key missing".

> Custom providers are merged generically (the `custom` loop), so they need no edit here.

### 4. Provider Implementation

Most providers use `OpenAIProvider` from `src/brain/provider/custom_openai_compatible.rs`. This handles:
- Streaming responses
- Tool call parsing
- Token usage tracking
- Rate limiting
- Error handling

For providers with special requirements (native APIs, subprocess-based, OAuth), create a new file in `src/brain/provider/`:

The `Provider` trait lives in `src/brain/provider/trait.rs`. Most methods have default
implementations; a native provider must implement these **7 required (non-defaulted)** methods
(verify against the trait, since it evolves):

```rust
// src/brain/provider/yourprovider.rs
use async_trait::async_trait;

#[async_trait]
impl Provider for YourProvider {
    // Non-streaming completion.
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> { /* ... */ }

    // Streaming completion — the hot path. Returns a stream of StreamEvent.
    async fn stream(&self, request: LLMRequest) -> Result<ProviderStream> { /* ... */ }

    fn name(&self) -> &str { "yourprovider" }
    fn default_model(&self) -> &str { /* ... */ }
    fn supported_models(&self) -> Vec<String> { /* ... */ }
    fn context_window(&self, model: &str) -> Option<u32> { /* ... */ }
    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 { /* ... */ }
}
```

Useful defaulted methods you may want to override: `supports_streaming`, `supports_tools`,
`supports_vision`, `fetch_models` (live `/models`), `cli_handles_tools`, `cli_manages_context`,
`base_url`, `calculate_cost_with_cache`. Almost all real providers wrap `OpenAIProvider` instead
of implementing this trait directly — only write a native impl for a genuinely non-OpenAI API.

### 5. Model Fetching

Providers are categorized by how they get their model list:

#### API Fetch (Automatic)
Providers with `/v1/models` endpoint get models fetched automatically:
- Leave `models: &[]` empty in `PROVIDERS` array
- The onboarding UI calls `fetch_models_from_endpoint()` from `src/brain/provider/model_fetch.rs`
- Works with any OpenAI-compatible endpoint

#### Config-Based (Manual)
Providers without `/v1/models` endpoint:
- Add `models: Vec<String>` to config.toml
- List models in `PROVIDERS` array in onboarding types

Example config.toml:
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1"
default_model = "your-model-name"
models = ["your-model-name", "your-model-v2", "your-model-lite"]
```

### 6. API Keys

API keys are stored in `~/.opencrabs/keys.toml` (chmod 600):

```toml
[providers.yourprovider]
api_key = "your-api-key-here"
```

The config loader automatically merges keys.toml into the ProviderConfig at runtime.

## Config.toml Examples

### For providers with API model fetch:
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1/chat/completions"
default_model = "your-model-name"
```

### For providers WITHOUT API model fetch:
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1"
default_model = "your-model-name"
models = ["your-model-name", "your-model-v2"]
```

### With vision model override:
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1"
default_model = "your-model-name"
vision_model = "your-vision-model"  # Used when images are present
```

### With thinking mode (reasoning models):
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1"
default_model = "your-reasoning-model"
enable_thinking = true  # Enable hybrid reasoning mode
```

### With forced use of max_completion_tokens instead of max_tokens:
```toml
[providers.yourprovider]
enabled = true
base_url = "https://api.yourprovider.com/v1"
default_model = "your-reasoning-model"
use_max_completion_tokens = true
```

## Provider Requirements (Mandatory)

All new providers MUST implement the following to ensure full functionality.

> **Reference Implementation:** See `src/brain/provider/custom_openai_compatible.rs` for the OpenAI-compatible implementation that handles streaming, tool calls, and token usage.

### 1. Streaming Support

Use `stream_options: { include_usage: true }` in the request body:

```rust
openai_request.stream_options = Some(StreamOptions { include_usage: true });
```

Parse chunks and accumulate tool call arguments across chunks (may arrive partially).

### 2. Tool Calls

- Support tool call streaming (arguments may come in multiple chunks)
- Accumulate arguments until valid JSON received
- Log granular tool call events with `[TOOL_PARSE]` prefix
- Handle both `delta` and `message` fields in chunks (some providers send final tool_calls in `message`)

```rust
// Example: accumulate arguments
let args = &tc_item.function.arguments;
let args_trimmed = args.trim();
let is_valid_json = !args_trimmed.is_empty() 
    && args_trimmed != "{}"
    && serde_json::from_str::<serde_json::Value>(args).is_ok();

if !is_valid_json {
    tracing::warn!("[TOOL_PARSE] ⚠️ Tool '{}' args INCOMPLETE, skipping emit", name);
    continue; // Wait for next chunk with complete data
}
```

### 3. Token Usage (Critical)

Extract `usage` field from the final chunk:

```rust
// Add usage field to stream chunk struct
struct OpenAIStreamChunk {
    id: String,
    choices: Vec<OpenAIStreamChoice>,
    usage: Option<OpenAIUsage>,  // MUST HAVE
}

// Extract and emit usage
if let Some(ref usage) = chunk.usage {
    let finish_reason = chunk.choices.first().and_then(|c| c.finish_reason.as_ref());
    if finish_reason.is_some() {
        let input_tokens = usage.prompt_tokens.unwrap_or(0);
        let output_tokens = usage.completion_tokens.unwrap_or(0);
        tracing::info!("[STREAM_USAGE] Final chunk usage: input={}, output={}", input_tokens, output_tokens);
        
        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: TokenUsage { input_tokens, output_tokens },
        }));
    }
}
```

### 4. Error Handling

- Graceful degradation on parse failures
- Don't discard accumulated data on errors
- Log parse errors with `[STREAM_PARSE]` prefix

### 5. Provider Struct Requirements

Ensure OpenAIUsage fields are optional to handle missing data:

```rust
#[derive(Debug, Clone, Deserialize)]
struct OpenAIUsage {
    #[serde(rename = "prompt_tokens")]
    prompt_tokens: Option<u32>,
    #[serde(rename = "completion_tokens")]
    completion_tokens: Option<u32>,
}
```

## Testing

### Unit Tests

Add tests in `src/tests/` directory:

```rust
// src/tests/yourprovider_test.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yourprovider_creation() {
        // Test provider creation with valid config
    }

    #[test]
    fn test_yourprovider_missing_key() {
        // Test graceful handling of missing API key
    }
}
```

Remember to register the new test module in `src/tests/mod.rs` (`pub mod yourprovider_test;`) —
this project keeps every test as a file under `src/tests/`, never as an inline `#[cfg(test)] mod`.

### Existing tests that may need updating

These enumerate or count the provider set, so they can fail (or have a stale comment) when you
add a provider — check them:
- `src/tests/provider_registry_test.rs` — asserts the built-in provider set from `ProviderConfigs`.
- `src/tests/provider_factory_regression_test.rs` — verifies the factory wiring (note: its header
  comment hardcodes a provider count, keep it accurate).
- `src/tests/provider_config_regression_test.rs` — checks `KNOWN_PROVIDERS` entries.
- `src/tests/provider_sync_test.rs` — provider/model sync across surfaces.

> There is currently **no compile-time check** that a `ProviderConfigs` field has a matching
> `REGISTRATIONS` entry. `bedrock` and `vertex` exist as config fields with no factory registration,
> no `KNOWN_PROVIDERS` entry, and no onboarding entry — i.e. dead, non-functional stubs. Don't add a
> provider that way: wire ALL the touchpoints above, or leave the field out.

### Integration Tests

Test the full flow:
1. Add provider to config.toml
2. Add API key to keys.toml
3. Run `/onboard:provider` and verify it appears in the list
4. Select the provider and verify model fetching works
5. Send a message and verify streaming works
6. Test tool calls and verify they parse correctly

## Checklist

Before submitting a PR:

Code wiring (all REQUIRED for a built-in provider):
- [ ] Added field to `ProviderConfigs` in `src/config/types.rs`
- [ ] Added to `REGISTRATIONS` array in `src/brain/provider/factory.rs`
- [ ] Added to `PROVIDER_NAMES` array (same order/length as REGISTRATIONS)
- [ ] Wrote `try_create_<provider>` factory function (wrap `OpenAIProvider` via `configure_openai_compatible` unless native)
- [ ] Added to `KNOWN_PROVIDERS` in `src/utils/providers.rs`
- [ ] Added a `config_for()` match arm in `src/utils/providers.rs`
- [ ] Added the keys-source mapping entry + `merge_provider_keys` arm in `src/config/types.rs`
- [ ] Added to `PROVIDERS` array in `src/tui/onboarding/types.rs`

Config + docs:
- [ ] Added a `[providers.<name>]` section to `config.toml.example`
- [ ] Added a `[providers.<name>]` key placeholder to `keys.toml.example`
- [ ] (Optional) Added a `[providers.<name>]` pricing block to `usage_pricing.toml`/example
- [ ] Updated the "Current Provider List" at the bottom of THIS doc (both orders)

Tests:
- [ ] Added unit tests in `src/tests/` (creation with valid config, graceful missing-key handling)
- [ ] Updated `provider_factory_regression_test.rs` / `provider_registry_test.rs` / `provider_config_regression_test.rs` if they assert a provider count or enumerate the set
- [ ] `cargo test --all-features` green; `cargo clippy --all-features --lib --bins --tests -- -D warnings` clean

Manual verification:
- [ ] Tested with valid config and API key (streaming + a tool call)
- [ ] Tested graceful handling of missing API key
- [ ] Verified model fetching / `/models` picker shows the provider's models

## Common Patterns

### Local Providers (No API Key)

For local providers (Ollama, LM Studio, etc.):

```rust
fn try_create_yourprovider(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let yourprovider_config = match &config.providers.yourprovider {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let base_url = yourprovider_config
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434/v1/chat/completions".to_string());

    // API key is optional for local providers
    let api_key = yourprovider_config.api_key.clone().unwrap_or_default();

    let mut builder = OpenAIProvider::with_base_url(api_key, base_url.clone())
        .with_name("yourprovider");
    
    // Add thinking flag for local servers
    if is_local_base_url(&base_url) {
        let enable = yourprovider_config.enable_thinking.unwrap_or(true);
        builder = builder.with_body_transform(local_thinking_body_transform(enable));
    }
    
    let provider = configure_openai_compatible(builder, yourprovider_config);
    Ok(Some(Arc::new(provider)))
}
```

### OAuth-Based Providers

For providers using OAuth device flow (GitHub Copilot, Codex):

```rust
async fn try_create_yourprovider(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let yourprovider_config = match &config.providers.yourprovider {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let oauth_token = yourprovider_config.api_key.clone().filter(|k| !k.is_empty());
    let Some(oauth_token) = oauth_token else {
        tracing::warn!("YourProvider enabled but no OAuth token found");
        return Ok(None);
    };

    // Create token manager with background refresh
    let manager = Arc::new(YourTokenManager::new(oauth_token));
    manager.clone().start_background_refresh();

    // Build token_fn closure
    let mgr_clone = manager.clone();
    let token_fn: super::custom_openai_compatible::TokenFn =
        Arc::new(move || mgr_clone.get_cached_token());

    let base_url = yourprovider_config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.yourprovider.com/v1/chat/completions".to_string());

    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url("oauth-managed".to_string(), base_url)
            .with_name("yourprovider")
            .with_token_fn(token_fn),
        yourprovider_config,
    );
    Ok(Some(Arc::new(provider)))
}
```

### Providers with Multiple Endpoints

For providers with different API modes (e.g., z.ai GLM with "api" and "coding" endpoints):

```rust
fn try_create_yourprovider(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let yourprovider_config = match &config.providers.yourprovider {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    let Some(api_key) = &yourprovider_config.api_key else {
        tracing::warn!("YourProvider enabled but API key missing");
        return Ok(None);
    };

    // Determine base URL based on endpoint_type
    let base_url = match yourprovider_config.endpoint_type.as_deref() {
        Some("coding") => "https://api.yourprovider.com/coding/v4/chat/completions",
        _ => "https://api.yourprovider.com/api/v4/chat/completions",
    };

    tracing::info!(
        "Using YourProvider at: {} (endpoint_type: {:?})",
        base_url,
        yourprovider_config.endpoint_type
    );
    
    let provider = configure_openai_compatible(
        OpenAIProvider::with_base_url(api_key.clone(), base_url.to_string())
            .with_name("yourprovider"),
        yourprovider_config,
    );
    Ok(Some(Arc::new(provider)))
}
```

## Troubleshooting

### Provider not appearing in onboarding

- Check `PROVIDERS` array in `src/tui/onboarding/types.rs`
- Verify `id` matches `session_id` in factory

### Provider enabled but not used

- Check `REGISTRATIONS` array order (priority matters)
- Verify `is_enabled` closure returns true
- Check logs for "YourProvider enabled but could not be created"

### Model fetching fails

- Verify endpoint supports `/v1/models`
- Check API key is valid
- Look for `[model_fetch]` log messages

### Streaming fails

- Verify `stream_options: { include_usage: true }` is set
- Check provider returns `usage` in final chunk
- Look for `[STREAM_PARSE]` or `[TOOL_PARSE]` log messages

## References

- Factory + all OpenAI-compatible `try_create_*` functions: `src/brain/provider/factory.rs`
- Config types: `src/config/types.rs` (`ProviderConfigs`, `ProviderConfig`, `merge_provider_keys`)
- Name registry: `src/utils/providers.rs` (`KNOWN_PROVIDERS`, `config_for`)
- Onboarding types: `src/tui/onboarding/types.rs`
- OpenAI-compatible provider + builder: `src/brain/provider/custom_openai_compatible.rs` (`OpenAIProvider`)
- Provider trait: `src/brain/provider/trait.rs`
- Model fetching: `src/brain/provider/model_fetch.rs`
- Examples — OpenAI-compatible (just `try_create_*` in factory.rs): `minimax`, `zhipu`, `ollama`, `openrouter`. Separate files for native/quirky APIs: `qwen.rs`, `gemini.rs`, `anthropic.rs`, `copilot.rs`, `codex_oauth.rs`. (Note: there is no `ollama.rs` — Ollama is OpenAI-compatible and lives in factory.rs.)

## Current Provider List (as of v0.3.36)

> This list drifts. The source of truth is the `REGISTRATIONS` array in `factory.rs` (runtime
> order) and the `PROVIDERS` array in `onboarding/types.rs` (UI order). Re-verify against those
> two arrays. Note: `bedrock` and `vertex` have `ProviderConfigs` fields but are NOT registered,
> so they are not in either list.

### Factory Order (Runtime Priority)
1. Claude CLI (claude-cli)
2. OpenCode CLI (opencode-cli)
3. Codex CLI (codex-cli)
4. Codex (codex)
5. OpenCode (opencode)
6. Qwen (qwen)
7. Anthropic (anthropic)
8. OpenAI (openai)
9. GitHub Copilot (github)
10. Google Gemini (gemini)
11. OpenRouter (openrouter)
12. Minimax (minimax)
13. z.ai GLM (zhipu)
14. Ollama (ollama)
15. Custom (custom)

### Onboarding Order (UI Presentation)
1. Anthropic (anthropic)
2. OpenAI (openai)
3. GitHub Copilot (github)
4. Google Gemini (gemini)
5. OpenRouter (openrouter)
6. Minimax (minimax)
7. z.ai GLM (zhipu)
8. Claude CLI (claude-cli)
9. OpenCode CLI (opencode-cli)
10. Codex CLI (codex-cli)
11. Codex (codex)
12. OpenCode (opencode)
13. Qwen (qwen)
14. Ollama (ollama)
15. Custom OpenAI-Compatible (custom)
