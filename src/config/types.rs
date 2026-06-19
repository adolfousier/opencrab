//! Configuration types, defaults, loading, and validation.

use super::crabrace::CrabraceConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Flag set when Config::load() recovered from a last-known-good snapshot.
static CONFIG_RECOVERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Flag set when Config::load() mechanically repaired a broken config file
/// in place (e.g. closed an unterminated array) and re-loaded it.
static CONFIG_AUTOFIXED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Unknown top-level keys found in config.toml (possible typos).
static CONFIG_TYPO_WARNINGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Mutex protecting read-modify-write cycles on config.toml / keys.toml.
/// Without this, concurrent `write_key` calls can race: one reads while
/// another is mid-write, gets a partial/empty file, parses it as empty,
/// and overwrites the real config with an empty table.
pub static CONFIG_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Crabrace integration configuration
    #[serde(default)]
    pub crabrace: CrabraceConfig,

    /// Database configuration
    #[serde(default)]
    pub database: DatabaseConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Debug options
    #[serde(default)]
    pub debug: DebugConfig,

    /// LLM provider configurations
    #[serde(default)]
    pub providers: ProviderConfigs,

    /// Messaging channel integrations
    #[serde(default)]
    pub channels: ChannelsConfig,

    /// Agent behaviour configuration
    #[serde(default)]
    pub agent: AgentConfig,

    /// Daemon mode configuration (systemd / launchd service)
    #[serde(default)]
    pub daemon: DaemonConfig,

    /// A2A (Agent-to-Agent) protocol gateway configuration
    #[serde(default, alias = "gateway")]
    pub a2a: A2aConfig,

    /// Image generation and vision configuration
    #[serde(default)]
    pub image: ImageConfig,

    /// Cron job defaults
    #[serde(default)]
    pub cron: CronConfig,

    /// Memory / embedding configuration
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Brain-file behaviour: read-time empty-section stripping and other
    /// per-file knobs. Optional — defaults preserve historical behaviour
    /// where strip-on-load was off.
    #[serde(default)]
    pub brain: BrainConfig,

    /// Browser configuration for browser_navigate and browser_click tools.
    /// When `cdp_endpoint` is set, connects to an existing Chromium instance
    /// instead of spawning a new one. Useful for sharing a single browser
    /// across multiple profiles to save memory.
    #[serde(default)]
    pub browser: BrowserConfig,
}

/// Brain-file behaviour configuration. Issue #164 added read-time stripping
/// of empty header stubs (`## Header` with no body) so the LLM never sees
/// dead sections, plus a per-file line cap so `sync_templates` cannot
/// silently grow a file past the user's budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    /// Strip header stubs from brain-file reads. Default true. Writes are
    /// never affected — disk stays authoritative; only the loaded view is
    /// filtered.
    #[serde(default = "default_strip_empty_sections")]
    pub strip_empty_sections: bool,

    /// Per-file line caps for `sync_templates`. When a merged file would
    /// exceed its cap, the sync BAILS instead of writing — the user sees
    /// a warning naming the file, the current and upstream line counts,
    /// and the top-3 largest new sections that would have been added.
    /// Empty map means no cap configured beyond `default_brain_file_cap`.
    /// Issue #164 fix 2.
    #[serde(default)]
    pub caps: std::collections::BTreeMap<String, usize>,

    /// Fallback cap applied to any brain file not explicitly listed in
    /// `caps`. Default 500 lines per the issue's recommended budget.
    #[serde(default = "default_brain_file_cap")]
    pub default_cap: usize,
}

fn default_true() -> bool {
    true
}

fn default_strip_empty_sections() -> bool {
    true
}

fn default_brain_file_cap() -> usize {
    500
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            strip_empty_sections: default_strip_empty_sections(),
            caps: std::collections::BTreeMap::new(),
            default_cap: default_brain_file_cap(),
        }
    }
}

impl BrainConfig {
    /// Resolve the line cap for a specific filename. Looks up `caps` first,
    /// falls back to `default_cap`. Filenames are matched exactly (case
    /// sensitive) so `TOOLS.md` and `tools.md` are distinct entries.
    pub fn cap_for(&self, filename: &str) -> usize {
        self.caps.get(filename).copied().unwrap_or(self.default_cap)
    }
}

/// Browser configuration for browser_navigate and browser_click tools.
///
/// When `cdp_endpoint` is set, the browser manager connects to an existing
/// Chromium instance via Chrome DevTools Protocol instead of spawning a new
/// one. This allows multiple profiles to share a single browser, saving
/// significant memory (each Chromium instance uses ~250-300MB).
///
/// Example in config.toml:
/// ```toml
/// [browser]
/// cdp_endpoint = "http://localhost:9222"
/// ```
///
/// To start a standalone Chromium with CDP enabled:
/// ```bash
/// chromium --remote-debugging-port=9222 --headless --no-sandbox
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserConfig {
    /// CDP endpoint for an existing Chromium instance with remote debugging
    /// enabled. When set, the browser manager connects to this endpoint instead
    /// of spawning a new browser, so multiple profiles can share one Chromium.
    ///
    /// Prefer the `http://host:port` form — the manager queries `/json/version`
    /// to discover the real devtools websocket URL. A bare `ws://host:port` is
    /// also accepted (normalized to `http://` internally); a full
    /// `ws://host:port/devtools/browser/<id>` URL is used as-is.
    ///
    /// Example: "http://localhost:9222"
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
}

/// Daemon mode configuration (systemd / launchd service).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Health check HTTP port. When set, `opencrabs daemon` binds a tiny HTTP
    /// server on `0.0.0.0:<port>` that responds to `GET /health` with 200 OK.
    /// Useful for systemd watchdog, uptime monitors, and external health probes.
    #[serde(default)]
    pub health_port: Option<u16>,
}

/// A2A (Agent-to-Agent) protocol gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aConfig {
    /// Whether the A2A gateway is enabled (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// Bind address (default: "127.0.0.1")
    #[serde(default = "default_a2a_bind")]
    pub bind: String,

    /// Gateway port (default: 18790)
    #[serde(default = "default_a2a_port")]
    pub port: u16,

    /// Allowed CORS origins — must be set explicitly, no cross-origin requests allowed by default
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Optional API key for authenticating incoming A2A requests (Bearer token).
    /// If set, all JSON-RPC requests must include `Authorization: Bearer <key>`.
    /// If unset, no authentication is required (suitable for loopback-only use).
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_a2a_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_a2a_port() -> u16 {
    18790
}

impl Default for A2aConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_a2a_bind(),
            port: default_a2a_port(),
            allowed_origins: vec![],
            api_key: None,
        }
    }
}

/// Messaging channel integrations configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub whatsapp: WhatsAppConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub trello: TrelloConfig,
    #[serde(default)]
    pub signal: SignalConfig,
    #[serde(default)]
    pub google_chat: GoogleChatConfig,
    #[serde(default)]
    pub imessage: IMessageConfig,
}

/// When the bot should respond to messages in group channels.
/// DMs always get a response regardless of this setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RespondTo {
    /// Respond to all messages from allowed users
    All,
    /// Only respond to direct messages, ignore group channels entirely
    DmOnly,
    /// Only respond when @mentioned (or replied-to on Telegram)
    #[default]
    Mention,
}

/// Deserialize `allowed_users` from either a TOML integer array (legacy) or string array.
fn deser_users_compat<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Int(i64),
        Str(String),
    }
    Vec::<NumOrStr>::deserialize(d).map(|v| {
        v.into_iter()
            .map(|x| match x {
                NumOrStr::Int(n) => n.to_string(),
                NumOrStr::Str(s) => s,
            })
            .collect()
    })
}

/// Telegram channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    /// Allowlisted Telegram user IDs (numeric). Accepts int or string arrays.
    #[serde(default, deserialize_with = "deser_users_compat")]
    pub allowed_users: Vec<String>,
    /// Restrict bot to specific channel IDs. Empty = all channels. DMs always pass.
    #[serde(default)]
    pub allowed_channels: Vec<String>,
    /// When the bot should respond: "all", "dm_only", or "mention" (default)
    #[serde(default)]
    pub respond_to: RespondTo,
    /// Idle session timeout in hours for non-owner sessions.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
    /// Send structured replies as native Telegram rich messages (Bot API 10.1:
    /// tables, headings, lists, math). Off by default — rich messages are
    /// unreadable on Telegram Web and older clients (they show a "not supported"
    /// placeholder with no fallback). Enable only when the audience is on
    /// current mobile/desktop clients; otherwise the universal HTML rendering
    /// (which works on every client) is used.
    #[serde(default)]
    pub rich_messages: bool,
    /// Silently ignore /start commands from non-allowed users in group chats.
    /// When true (default), the bot does NOT reply with user ID in groups.
    /// Users who need their ID can DM the bot instead.
    #[serde(default = "default_true")]
    pub silence_group_start: bool,
}

/// Discord channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    /// Allowlisted Discord user IDs (numeric). Accepts int or string arrays.
    #[serde(default, deserialize_with = "deser_users_compat")]
    pub allowed_users: Vec<String>,
    /// Restrict bot to specific channel IDs. Empty = all channels.
    #[serde(default)]
    pub allowed_channels: Vec<String>,
    /// When the bot should respond: "all", "dm_only", or "mention" (default)
    #[serde(default)]
    pub respond_to: RespondTo,
    /// Idle session timeout in hours for non-owner sessions.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// Slack channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Bot token (xoxb-...)
    #[serde(default)]
    pub token: Option<String>,
    /// App-level token for Socket Mode (xapp-...)
    #[serde(default)]
    pub app_token: Option<String>,
    /// Allowlisted Slack user IDs (U12345678). Accepts int or string arrays.
    #[serde(default, deserialize_with = "deser_users_compat")]
    pub allowed_users: Vec<String>,
    /// Restrict bot to specific channel IDs. Empty = all channels.
    #[serde(default)]
    pub allowed_channels: Vec<String>,
    /// When the bot should respond: "all", "dm_only", or "mention" (default)
    #[serde(default)]
    pub respond_to: RespondTo,
    /// Idle session timeout in hours for non-owner sessions.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// WhatsApp channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhatsAppConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Allowlisted phone numbers (E.164 format: "+15551234567").
    /// Empty = accept messages from everyone (not recommended for business numbers).
    #[serde(default)]
    pub allowed_phones: Vec<String>,
    /// Idle session timeout in hours for non-owner sessions.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// Trello channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrelloConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Trello API Token
    #[serde(default)]
    pub token: Option<String>,
    /// Trello API Key (stored as app_token for keys.toml symmetry)
    #[serde(default)]
    pub app_token: Option<String>,
    /// Allowlisted Trello member IDs. Empty = respond to all members.
    #[serde(default, deserialize_with = "deser_users_compat")]
    pub allowed_users: Vec<String>,
    /// Board IDs to monitor for @mentions.
    /// Accepts the old `allowed_channels` key as an alias for migration compatibility.
    #[serde(default, alias = "allowed_channels")]
    pub board_ids: Vec<String>,
    /// Optional polling interval in seconds. Absent or 0 = no polling (tool-only mode).
    #[serde(default)]
    pub poll_interval_secs: Option<u64>,
    /// Idle session timeout in hours for non-owner sessions.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// Signal channel configuration (placeholder — not yet implemented)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignalConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Allowlisted phone numbers (E.164 format)
    #[serde(default)]
    pub allowed_phones: Vec<String>,
    /// Idle session timeout in hours.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// Google Chat channel configuration (placeholder — not yet implemented)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoogleChatConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    /// Allowlisted user IDs. Accepts int or string arrays.
    #[serde(default, deserialize_with = "deser_users_compat")]
    pub allowed_users: Vec<String>,
    /// Idle session timeout in hours.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// iMessage channel configuration (placeholder — not yet implemented)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IMessageConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Allowlisted phone numbers (E.164 format)
    #[serde(default)]
    pub allowed_phones: Vec<String>,
    /// Idle session timeout in hours.
    #[serde(default)]
    pub session_idle_hours: Option<f64>,
}

/// STT mode: API (Groq Whisper) or Local (whisper.cpp)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SttMode {
    #[default]
    Api,
    Local,
}

/// TTS mode: API (OpenAI) or Local (Piper)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TtsMode {
    #[default]
    Api,
    Local,
}

/// Runtime voice configuration — assembled from providers.stt / providers.tts.
/// NOT serialized to config file.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub stt_enabled: bool,
    pub stt_mode: SttMode,
    pub local_stt_model: String,
    pub stt_base_url: Option<String>,
    pub stt_model: Option<String>,
    pub stt_api_key: Option<String>,
    pub tts_enabled: bool,
    pub tts_mode: TtsMode,
    pub tts_voice: String,
    pub tts_model: String,
    pub tts_base_url: Option<String>,
    pub tts_api_key: Option<String>,
    pub local_tts_voice: String,
    pub stt_provider: Option<ProviderConfig>,
    pub tts_provider: Option<ProviderConfig>,
    pub voicebox_stt_enabled: bool,
    pub voicebox_stt_base_url: String,
    pub voicebox_tts_enabled: bool,
    pub voicebox_tts_base_url: String,
    pub voicebox_tts_profile_id: String,
    pub voicebox_tts_engine: String,
    /// User-defined STT fallback order. Empty means "use the default
    /// priority: voicebox → openai-compatible → groq → local". When the
    /// active provider fails (5xx, liveness probe error, unreachable),
    /// the dispatcher walks this list in order and tries each one that
    /// has the credentials/config it needs. Mirrors the
    /// completion-side `fallback_providers` chain so the user can
    /// codify "if my local voicebox is down, try Groq, then OpenAI".
    /// Values: `"voicebox"`, `"openai_compatible"`, `"groq"`, `"local"`.
    pub stt_fallback_chain: Vec<String>,
    /// User-defined TTS fallback order. Empty means "use the default
    /// priority: voicebox → openai-compatible → openai → local". Same
    /// semantics as `stt_fallback_chain` but for synthesis.
    /// Values: `"voicebox"`, `"openai_compatible"`, `"openai"`, `"local"`.
    pub tts_fallback_chain: Vec<String>,
}

fn default_local_stt_model() -> String {
    "local-tiny".to_string()
}
fn default_tts_voice() -> String {
    "echo".to_string()
}
fn default_tts_model() -> String {
    "gpt-4o-mini-tts".to_string()
}
fn default_local_tts_voice() -> String {
    "ryan".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            stt_enabled: false,
            stt_mode: SttMode::default(),
            local_stt_model: default_local_stt_model(),
            stt_base_url: None,
            stt_model: None,
            stt_api_key: None,
            tts_enabled: false,
            tts_mode: TtsMode::default(),
            tts_voice: default_tts_voice(),
            tts_model: default_tts_model(),
            tts_base_url: None,
            tts_api_key: None,
            local_tts_voice: default_local_tts_voice(),
            stt_provider: None,
            tts_provider: None,
            voicebox_stt_enabled: false,
            voicebox_stt_base_url: default_voicebox_url(),
            voicebox_tts_enabled: false,
            voicebox_tts_base_url: default_voicebox_url(),
            voicebox_tts_profile_id: String::new(),
            voicebox_tts_engine: String::new(),
            stt_fallback_chain: Vec::new(),
            tts_fallback_chain: Vec::new(),
        }
    }
}

/// Image generation and vision configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageConfig {
    #[serde(default)]
    pub generation: ImageGenerationConfig,
    #[serde(default)]
    pub vision: ImageVisionConfig,
}

/// Image generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_image_model")]
    pub model: String,
    /// Loaded from keys.toml at runtime, never serialized to config.toml
    #[serde(skip, default)]
    pub api_key: Option<String>,
}

impl Default for ImageGenerationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_image_model(),
            api_key: None,
        }
    }
}

/// Image vision configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageVisionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_image_model")]
    pub model: String,
    /// Loaded from keys.toml at runtime, never serialized to config.toml
    #[serde(skip, default)]
    pub api_key: Option<String>,
}

impl Default for ImageVisionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_image_model(),
            api_key: None,
        }
    }
}

pub fn default_image_model() -> String {
    "gemini-3.1-flash-image-preview".to_string()
}

/// Agent behaviour configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Approval policy: "ask", "auto-session", "auto-always"
    #[serde(default = "default_approval_policy")]
    pub approval_policy: String,

    /// Maximum concurrent tool calls
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,

    /// Context window limit in tokens (default: 200000)
    #[serde(default = "default_context_limit")]
    pub context_limit: u32,

    /// Max output tokens for API calls (default: 65536)
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Default provider for spawned sub-agents (e.g., "openrouter", "anthropic", "custom:lmstudio").
    /// If unset, sub-agents inherit the parent session's active provider.
    #[serde(default)]
    pub subagent_provider: Option<String>,

    /// Default model for spawned sub-agents (e.g., "claude-sonnet-4-6").
    /// Only used when subagent_provider is set.
    #[serde(default)]
    pub subagent_model: Option<String>,

    /// Auto-install new releases on startup without prompting (default: true).
    /// When false, the user is shown an update prompt dialog instead.
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,

    /// Override provider for autonomous RSI self-improvement cycles (e.g. "zhipu", "minimax").
    /// RSI runs on its own provider chain so it never competes with chat or sub-agents for quota.
    /// When set, RSI jobs use this provider instead of the session's active one.
    #[serde(default)]
    pub self_improvement_provider: Option<String>,

    /// Override model for RSI self-improvement cycles. Only used when self_improvement_provider is set.
    /// Prefer cheap, fast models for autonomous analysis — results are deterministic.
    #[serde(default)]
    pub self_improvement_model: Option<String>,

    /// Suppress the agent's playful post-compaction narration. Default
    /// `false` (= keep the personality moments). When true, the
    /// compaction-recovery prompts switch to a silent-continuation
    /// variant that tells the model to resume without acknowledging
    /// the compaction at all.
    ///
    /// Why default fun: users have specifically called out post-
    /// compaction one-liners as something they enjoy and forward to
    /// friends — emergent character per-language (e.g. Russian мат in
    /// frustration moments) generates the "this thing has personality"
    /// signal that's hard to fake. The flag exists for formal /
    /// corporate / customer-facing deployments where dropping mid-
    /// session profanity would be inappropriate.
    #[serde(default)]
    pub silent_compaction: bool,

    /// Lazy tool-schema loading. **On by default.** A request ships only the
    /// CORE tool schemas (~4k tokens) plus `tool_search`, instead of all ~95
    /// (~20k counted in every request's input); the agent calls `tool_search`
    /// to discover and activate extended tools on demand. Set
    /// `lazy_tools = false` to restore the old behaviour (all tool schemas in
    /// every request).
    #[serde(default = "default_lazy_tools")]
    pub lazy_tools: bool,

    /// Redact sensitive data (API keys, tokens, passwords, IPs) from tool
    /// outputs and display. **On by default** for safety. Set to `false`
    /// during sysadmin/devops work where seeing IPs, tokens, and passwords
    /// is necessary. When false, the agent will still warn about secrets
    /// in logs but won't redact them from display.
    #[serde(default = "default_redact_sensitive_data")]
    pub redact_sensitive_data: bool,
}

fn default_lazy_tools() -> bool {
    true
}

fn default_redact_sensitive_data() -> bool {
    true
}

fn default_approval_policy() -> String {
    "auto-always".to_string()
}

fn default_max_concurrent() -> u32 {
    4
}

fn default_context_limit() -> u32 {
    200_000
}

fn default_max_tokens() -> u32 {
    65536
}

fn default_auto_update() -> bool {
    true
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            approval_policy: default_approval_policy(),
            max_concurrent: default_max_concurrent(),
            context_limit: default_context_limit(),
            max_tokens: default_max_tokens(),
            subagent_provider: None,
            subagent_model: None,
            auto_update: default_auto_update(),
            self_improvement_provider: None,
            self_improvement_model: None,
            silent_compaction: false,
            lazy_tools: default_lazy_tools(),
            redact_sensitive_data: default_redact_sensitive_data(),
        }
    }
}

/// Cron job default settings.
///
/// When a cron job has no `provider` or `model` set, these defaults are used
/// instead of the system's active provider. Useful for routing cron jobs to
/// cheaper providers while keeping the interactive session on a premium one.
///
/// Example in config.toml:
/// ```toml
/// [cron]
/// default_provider = "minimax"
/// default_model = "MiniMax-M2.7"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronConfig {
    /// Default provider for cron jobs without an explicit provider
    #[serde(default)]
    pub default_provider: Option<String>,

    /// Default model for cron jobs without an explicit model
    #[serde(default)]
    pub default_model: Option<String>,
}

/// OpenAI-compatible embedding provider configuration.
///
/// When set, embeddings are generated via an HTTP API call instead of the
/// local GGUF model (embeddinggemma-300M). This eliminates the ~300MB model
/// download and ~2.9GB RAM overhead of llama.cpp.
///
/// Supports any OpenAI-compatible `/v1/embeddings` endpoint:
/// OpenAI, Ollama, LM Studio, localai, etc.
///
/// Example in config.toml:
/// ```toml
/// [memory.embedding]
/// url = "https://api.openai.com/v1"
/// model = "text-embedding-3-small"
/// # api_key loaded from keys.toml: [providers.memory_embedding] api_key = "sk-..."
/// # dimensions = 1536   # auto-detected from first API response if unset
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingConfig {
    /// OpenAI-compatible API base URL (e.g. "https://api.openai.com/v1").
    /// The `/embeddings` path is appended automatically.
    #[serde(default)]
    pub url: Option<String>,

    /// Embedding model name (e.g. "text-embedding-3-small", "nomic-embed-text").
    #[serde(default)]
    pub model: Option<String>,

    /// API key for the embedding endpoint.
    /// Also loaded from keys.toml under `[providers.memory_embedding]`.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Embedding vector dimensions.
    /// Auto-detected from the first API response if unset.
    /// Local GGUF model always produces 768-dim vectors.
    #[serde(default)]
    pub dimensions: Option<usize>,
}

/// Memory / embedding configuration.
///
/// Controls whether vector embeddings are enabled for semantic memory search.
/// When disabled, only FTS5 (keyword) search is used.
///
/// Automatically set to `vector_enabled = false` when running on a VPS or
/// system with < 2GB RAM.
///
/// When `vector_enabled = true`, embeddings can be generated either:
/// - **Locally**: via embeddinggemma-300M GGUF model (default, no config needed)
/// - **Via API**: by configuring `[memory.embedding]` with an OpenAI-compatible endpoint
///
/// Example in config.toml:
/// ```toml
/// [memory]
/// vector_enabled = true
///
/// [memory.embedding]
/// url = "https://api.openai.com/v1"
/// model = "text-embedding-3-small"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Whether vector embeddings are enabled (default: true on desktop, false on VPS)
    #[serde(default = "default_vector_enabled")]
    pub vector_enabled: bool,

    /// OpenAI-compatible embedding provider. When set, embeddings are generated
    /// via API instead of the local GGUF model. Eliminates ~300MB download + ~2.9GB RAM.
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,
}

const fn default_vector_enabled() -> bool {
    true
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            vector_enabled: default_vector_enabled(),
            embedding: None,
        }
    }
}

impl MemoryConfig {
    /// Detect whether we're running on a VPS/cloud instance.
    ///
    /// Heuristics:
    /// - `/proc/1/cgroup` contains "container" or cloud provider strings
    /// - `/sys/class/dmi/id/product_name` contains cloud vendor names
    /// - Total system RAM is below 2GB
    /// - No display server detected (no DISPLAY/WAYLAND_DISPLAY env vars)
    fn is_vps() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Check /sys/class/dmi/id/product_name for cloud vendor strings
            if let Ok(product) = std::fs::read_to_string("/sys/class/dmi/id/product_name") {
                let product = product.to_lowercase();
                let cloud_vendors = [
                    "droplet",
                    "digitalocean",
                    "ec2",
                    "amazon",
                    "gce",
                    "google compute",
                    "kvm",
                    "vultr",
                    "linode",
                    "akamai",
                    "azure",
                    "hyper-v",
                    "oracle",
                    "oci",
                ];
                for vendor in &cloud_vendors {
                    if product.contains(vendor) {
                        return true;
                    }
                }
            }
            // Check for container environment
            if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup")
                && (cgroup.contains("docker")
                    || cgroup.contains("containerd")
                    || cgroup.contains("kubepods"))
            {
                return true;
            }

            // Check system RAM — if less than 2GB, likely a small VPS
            if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal:") {
                        // MemTotal is in kB
                        if let Some(kb_str) = line.split_whitespace().nth(1)
                            && let Ok(kb) = kb_str.parse::<u64>()
                            && {
                                let gb = kb / 1_048_576; // kB to GB
                                gb < 2
                            }
                        {
                            return true;
                        }
                        break;
                    }
                }
            }

            // No display server — likely headless server
            let has_display =
                std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();
            if !has_display {
                return true;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Non-Linux (macOS, Windows) — assume desktop, not VPS
        }

        false
    }

    /// Auto-apply VPS defaults if detected and config doesn't already have [memory] section.
    /// Returns true if config was modified.
    pub fn auto_apply_vps_defaults() -> bool {
        if !Self::is_vps() {
            return false;
        }

        // Check if [memory] section already exists in config.toml
        let config_path = opencrabs_home().join("config.toml");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            // If user already has a [memory] section, don't override
            if content.contains("[memory]") {
                return false;
            }
        }

        // Append [memory] section to config.toml
        tracing::info!(
            "VPS/cloud detected — disabling vector embeddings for memory search (FTS-only mode)"
        );

        let append = "\n# Auto-configured: VPS/cloud detected\n\
                      # Local vector embeddings disabled to save RAM (~2.9GB).\n\
                      # FTS5 keyword search still works. WIP: OpenAI-compatible\n\
                      # embedding through API coming soon.\n\
                      [memory]\n\
                      vector_enabled = false\n";

        let _ = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, append.as_bytes()));

        true
    }
}

/// Debug configuration options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DebugConfig {
    /// Enable LSP debug logging
    #[serde(default)]
    pub debug_lsp: bool,

    /// Enable profiling
    #[serde(default)]
    pub profiling: bool,
}

/// Canonical defaults for the Xiaomi (opencrabs × xiaomi collab) provider,
/// applied when `config.toml` has no `[providers.xiaomi]` section.
///
/// Xiaomi is keyless during the free collab window — the proxy supplies the
/// key server-side — so a config that predates the provider (or a fresh
/// `/evolve` that never appended the section) still gets a working, selectable
/// Xiaomi with zero manual edits. Without this, `try_create_xiaomi` returned
/// `Ok(None)` and `/models` showed "No models available" even though the picker
/// listed Xiaomi as keyless-available (#194), and `default_model()` would have
/// reported the `"MISSING_MODEL"` sentinel. The keyless time-gate stays in
/// `try_create_xiaomi`, so a synthesized section is harmless after the cutoff —
/// it only supplies model metadata.
pub fn xiaomi_provider_defaults() -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        default_model: Some("mimo-v2.5-pro".to_string()),
        models: [
            "mimo-v2.5-pro",
            "mimo-v2-pro",
            "mimo-v2.5",
            "mimo-v2-omni",
            "mimo-v2-flash",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // MiMo v2.5 is multimodal, so analyze_image routes to it natively
        // (via ProviderVisionTool) instead of needing a Gemini key. Falls back
        // to Gemini at call time if the proxy ever rejects image content.
        vision_model: Some("mimo-v2.5-pro".to_string()),
        // Cap at 200k even though MiMo advertises ~1M: quality degrades past
        // ~200-300k, and OpenCrabs already provides effectively-infinite memory
        // via transparent compaction, so the extra window buys nothing but
        // worse responses. Users can raise it manually if they really want it.
        context_window: Some(200_000),
        ..Default::default()
    }
}

/// serde field-default for [`ProviderConfigs::xiaomi`] — materializes the
/// canonical keyless section when the TOML omits `[providers.xiaomi]`.
fn default_xiaomi_provider() -> Option<ProviderConfig> {
    Some(xiaomi_provider_defaults())
}

/// LLM Provider configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfigs {
    /// Anthropic configuration
    #[serde(default)]
    pub anthropic: Option<ProviderConfig>,

    /// OpenAI configuration (official API)
    #[serde(default)]
    pub openai: Option<ProviderConfig>,

    /// OpenRouter configuration
    #[serde(default)]
    pub openrouter: Option<ProviderConfig>,

    /// Minimax configuration
    #[serde(default)]
    pub minimax: Option<ProviderConfig>,

    /// z.ai GLM configuration (supports API and Coding endpoints)
    #[serde(default)]
    pub zhipu: Option<ProviderConfig>,

    /// Xiaomi MiMo configuration (opencrabs x xiaomi collab). OpenAI-compatible.
    /// During the free collab window the key is supplied server-side by our
    /// proxy, so this needs no api_key; after the cutoff a user key is required.
    /// Defaults to the canonical keyless section when the TOML omits it, so
    /// configs predating the provider still get a working Xiaomi (#194).
    #[serde(default = "default_xiaomi_provider")]
    pub xiaomi: Option<ProviderConfig>,

    /// Named custom OpenAI-compatible providers (e.g. [providers.custom.ollama])
    #[serde(default, deserialize_with = "deserialize_custom_providers")]
    pub custom: Option<BTreeMap<String, ProviderConfig>>,

    /// GitHub Copilot configuration (uses OAuth device flow token)
    #[serde(default)]
    pub github: Option<ProviderConfig>,

    /// Google Gemini configuration
    #[serde(default)]
    pub gemini: Option<ProviderConfig>,

    /// Claude CLI (Max subscription) — direct subprocess, no proxy needed
    #[serde(default)]
    pub claude_cli: Option<ProviderConfig>,

    /// OpenCode CLI — direct subprocess, access to opencode's free models
    #[serde(default)]
    pub opencode_cli: Option<ProviderConfig>,

    /// Codex CLI (ChatGPT/Codex subscription) — direct subprocess, no API key needed
    #[serde(default)]
    pub codex_cli: Option<ProviderConfig>,

    /// Codex OAuth — native device-code flow, stores tokens in ~/.opencrabs/auth/codex.json
    #[serde(default)]
    pub codex: Option<ProviderConfig>,

    /// OpenCode API — native provider for Go and Zen plans (opencode.ai)
    #[serde(default)]
    pub opencode: Option<ProviderConfig>,

    /// Qwen (DashScope OpenAI-compatible) — standard API-key provider.
    #[serde(default)]
    pub qwen: Option<ProviderConfig>,

    /// Ollama — local or cloud (api.ollama.com). Auto-detects local models via /api/tags.
    #[serde(default)]
    pub ollama: Option<ProviderConfig>,

    /// AWS Bedrock configuration
    #[serde(default)]
    pub bedrock: Option<ProviderConfig>,

    /// VertexAI configuration
    #[serde(default)]
    pub vertex: Option<ProviderConfig>,

    /// STT (Speech-to-Text) provider configurations
    #[serde(default)]
    pub stt: Option<SttProviders>,

    /// TTS (Text-to-Speech) provider configurations
    #[serde(default)]
    pub tts: Option<TtsProviders>,

    /// Web search provider configurations
    #[serde(default)]
    pub web_search: Option<WebSearchProviders>,

    /// Image provider configurations (e.g. [providers.image.gemini])
    #[serde(default)]
    pub image: Option<ImageProviders>,

    /// Fallback provider configuration (under [providers.fallback] in config)
    #[serde(default)]
    pub fallback: Option<FallbackProviderConfig>,
}

impl ProviderConfigs {
    /// Get the first enabled custom provider (name + config)
    pub fn active_custom(&self) -> Option<(&str, &ProviderConfig)> {
        self.custom
            .as_ref()?
            .iter()
            .find(|(_, cfg)| cfg.enabled)
            .map(|(name, cfg)| (name.as_str(), cfg))
    }

    /// Get a specific custom provider by name (case-insensitive, normalized)
    pub fn custom_by_name(&self, name: &str) -> Option<&ProviderConfig> {
        let normalized = normalize_toml_key(name);
        self.custom.as_ref()?.get(&normalized)
    }

    /// Single source of truth for built-in provider iteration. Both
    /// `active_provider_and_model` (factory routing) and
    /// `resolve_provider_from_config` (display) walk this list, so adding a
    /// new provider field above only needs ONE new entry here — no more
    /// hardcoded if-else ladders silently omitting providers (the bug that
    /// hid `opencode`, `ollama`, `bedrock`, `vertex` from the TUI display
    /// for months).
    ///
    /// Tuple shape: `(session_id, display_name, requires_api_key, &Option<ProviderConfig>)`.
    /// `requires_api_key=false` for CLI providers where `enabled=true`
    /// alone activates them (claude-cli, opencode-cli, codex-cli, codex
    /// OAuth — the latter stores tokens in `~/.opencrabs/auth/`).
    ///
    /// Priority order matches what `factory::create_provider` would pick:
    /// CLI providers first (free, no key), then API providers, with custom
    /// providers handled separately by the caller via `active_custom()`.
    fn provider_registry(
        &self,
    ) -> [(&'static str, &'static str, bool, Option<&ProviderConfig>); 17] {
        [
            // Xiaomi MiMo (opencrabs x xiaomi collab) — the default. Keyless
            // (requires_api_key = false) during the free window: the proxy
            // supplies the key. First so a fresh, key-less install lands on it.
            ("xiaomi", "Xiaomi", false, self.xiaomi.as_ref()),
            // CLI providers — enabled flag alone is enough
            ("claude-cli", "Claude CLI", false, self.claude_cli.as_ref()),
            (
                "opencode-cli",
                "OpenCode CLI",
                false,
                self.opencode_cli.as_ref(),
            ),
            ("codex-cli", "Codex CLI", false, self.codex_cli.as_ref()),
            ("codex", "Codex OAuth", false, self.codex.as_ref()),
            // OpenCode API — OAuth-backed but registered as a regular provider
            ("opencode", "OpenCode", false, self.opencode.as_ref()),
            // API providers — require api_key in addition to enabled
            ("qwen", "Qwen", true, self.qwen.as_ref()),
            ("minimax", "Minimax", true, self.minimax.as_ref()),
            ("zhipu", "z.ai GLM", true, self.zhipu.as_ref()),
            ("openrouter", "OpenRouter", true, self.openrouter.as_ref()),
            ("anthropic", "Anthropic", true, self.anthropic.as_ref()),
            ("openai", "OpenAI", true, self.openai.as_ref()),
            ("github", "GitHub Copilot", true, self.github.as_ref()),
            ("gemini", "Google Gemini", true, self.gemini.as_ref()),
            ("ollama", "Ollama", false, self.ollama.as_ref()),
            ("bedrock", "AWS Bedrock", true, self.bedrock.as_ref()),
            ("vertex", "Google Vertex", true, self.vertex.as_ref()),
        ]
    }

    /// Return `(provider_name, default_model)` for the currently active provider,
    /// using the same priority order as `factory::create_provider`.
    ///
    /// Walks `provider_registry()` in priority order and returns the first
    /// entry that is enabled and (if `requires_api_key`) has an API key.
    /// Falls through to the first active custom provider, otherwise
    /// `("none", "none")`.
    pub fn active_provider_and_model(&self) -> (String, String) {
        for (id, _display, requires_api_key, cfg) in self.provider_registry() {
            if let Some(c) = cfg
                && c.enabled
                && (!requires_api_key || c.api_key.is_some())
            {
                let model = c
                    .default_model
                    .clone()
                    .unwrap_or_else(|| "(default)".to_string());
                return (id.to_string(), model);
            }
        }
        if let Some((name, cfg)) = self.active_custom() {
            let model = cfg
                .default_model
                .clone()
                .unwrap_or_else(|| "(default)".to_string());
            return (format!("custom:{}", name), model);
        }
        ("none".to_string(), "none".to_string())
    }
}

/// Custom deserializer that handles both old flat format `[providers.custom]`
/// and new named map format `[providers.custom.<name>]`.
fn deserialize_custom_providers<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<BTreeMap<String, ProviderConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    let value: Option<toml::Value> = Option::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };

    // Check if there are nested tables (named providers like [providers.custom.nvidia])
    // alongside top-level keys (flat format like [providers.custom] with enabled/api_key).
    // If both exist, extract the flat keys as "default" and parse named tables separately.
    let table = match value.as_table() {
        Some(t) => t,
        None => return Ok(None),
    };

    let flat_keys = ["enabled", "api_key", "base_url", "default_model", "models"];
    let has_flat = flat_keys.iter().any(|k| table.contains_key(*k));
    let has_named = table.values().any(|v| v.is_table());

    if has_flat && has_named {
        // Mixed: flat "default" provider + named providers in same section
        let mut map = BTreeMap::new();
        let mut flat_table = toml::map::Map::new();
        for key in &flat_keys {
            if let Some(v) = table.get(*key) {
                flat_table.insert(key.to_string(), v.clone());
            }
        }
        let default_cfg: ProviderConfig = toml::Value::Table(flat_table)
            .try_into()
            .map_err(de::Error::custom)?;
        map.insert("default".to_string(), default_cfg);
        for (name, val) in table {
            if flat_keys.contains(&name.as_str()) {
                continue;
            }
            if val.is_table() {
                let cfg: ProviderConfig = val.clone().try_into().map_err(de::Error::custom)?;
                map.insert(normalize_toml_key(name), cfg);
            }
        }
        Ok(Some(map))
    } else if has_flat {
        // Pure flat format — wrap as "default"
        let config: ProviderConfig = toml::Value::Table(table.clone())
            .try_into()
            .map_err(de::Error::custom)?;
        let mut map = BTreeMap::new();
        map.insert("default".to_string(), config);
        Ok(Some(map))
    } else {
        // Pure named map format — normalize keys on load
        let raw: BTreeMap<String, ProviderConfig> = toml::Value::Table(table.clone())
            .try_into()
            .map_err(de::Error::custom)?;
        let map: BTreeMap<String, ProviderConfig> = raw
            .into_iter()
            .map(|(k, v)| (normalize_toml_key(&k), v))
            .collect();
        Ok(if map.is_empty() { None } else { Some(map) })
    }
}

/// Fallback provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FallbackProviderConfig {
    /// Enable fallback
    #[serde(default)]
    pub enabled: bool,

    /// Legacy: single fallback provider type (backwards compat)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Ordered list of fallback provider names — tried in sequence on failure.
    /// Each name must match a configured provider (e.g. "anthropic", "openrouter").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<String>,
}

/// STT (Speech-to-Text) provider configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SttProviders {
    /// Groq STT configuration ([providers.stt.groq])
    #[serde(default)]
    pub groq: Option<ProviderConfig>,

    /// Local whisper.cpp STT configuration ([providers.stt.local])
    #[serde(default)]
    pub local: Option<LocalSttConfig>,

    /// OpenAI-compatible STT configuration ([providers.stt.openai_compatible])
    #[serde(default)]
    pub openai_compatible: Option<OpenaiCompatibleSttConfig>,

    /// Voicebox STT configuration ([providers.stt.voicebox])
    #[serde(default)]
    pub voicebox: Option<VoiceboxSttConfig>,

    /// User-defined STT fallback order. Empty/None means "use the default
    /// priority". Each value names a provider: `"voicebox"`,
    /// `"openai_compatible"`, `"groq"`, or `"local"`. When the active
    /// provider fails the dispatcher walks this list in order and tries
    /// each entry that has the credentials/config it needs.
    ///
    /// Mirrors the completion-side `fallback_providers` chain — use it
    /// to codify "if my local voicebox is down, try Groq, then OpenAI"
    /// without having to manually swap providers in the TUI on every
    /// outage.
    #[serde(default)]
    pub fallback_chain: Option<Vec<String>>,
}

/// OpenAI-compatible STT configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenaiCompatibleSttConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Base URL (e.g. "http://localhost:11434" or "https://api.groq.com/openai")
    #[serde(default)]
    pub base_url: Option<String>,
    /// Model name (e.g. "whisper-large-v3-turbo")
    #[serde(default)]
    pub model: Option<String>,
    /// API key
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Voicebox STT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceboxSttConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Base URL (e.g. "http://localhost:8000")
    #[serde(default = "default_voicebox_url")]
    pub base_url: String,
}

impl Default for VoiceboxSttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_voicebox_url(),
        }
    }
}

fn default_voicebox_url() -> String {
    "http://localhost:8000".to_string()
}

/// Local STT (whisper.cpp) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSttConfig {
    /// Whether local STT is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Model preset (e.g. "local-tiny", "local-base", "local-small", "local-medium")
    #[serde(default = "default_local_stt_model")]
    pub model: String,
}

impl Default for LocalSttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_local_stt_model(),
        }
    }
}

/// TTS (Text-to-Speech) provider configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TtsProviders {
    /// OpenAI TTS configuration ([providers.tts.openai])
    #[serde(default)]
    pub openai: Option<ProviderConfig>,

    /// Local Piper TTS configuration ([providers.tts.local])
    #[serde(default)]
    pub local: Option<LocalTtsConfig>,

    /// OpenAI-compatible TTS configuration ([providers.tts.openai_compatible])
    #[serde(default)]
    pub openai_compatible: Option<OpenaiCompatibleTtsConfig>,

    /// Voicebox TTS configuration ([providers.tts.voicebox])
    #[serde(default)]
    pub voicebox: Option<VoiceboxTtsConfig>,

    /// User-defined TTS fallback order. Empty/None means "use the default
    /// priority". Each value names a provider: `"voicebox"`,
    /// `"openai_compatible"`, `"openai"`, or `"local"`. When the active
    /// provider fails the dispatcher walks this list in order and tries
    /// each entry that has the credentials/config it needs.
    ///
    /// Mirrors the STT-side `fallback_chain` so the user can codify
    /// "if my local voicebox is down, try OpenAI TTS, then Piper" in
    /// one place.
    #[serde(default)]
    pub fallback_chain: Option<Vec<String>>,
}

/// OpenAI-compatible TTS configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenaiCompatibleTtsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Base URL (e.g. "http://localhost:11434")
    #[serde(default)]
    pub base_url: Option<String>,
    /// Model name (e.g. "gpt-4o-mini-tts")
    #[serde(default)]
    pub model: Option<String>,
    /// Voice name (e.g. "echo")
    #[serde(default)]
    pub voice: Option<String>,
    /// API key
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Voicebox TTS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceboxTtsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Base URL (e.g. "http://localhost:8000")
    #[serde(default = "default_voicebox_url")]
    pub base_url: String,
    /// Voice profile ID for synthesis
    #[serde(default)]
    pub profile_id: String,
    /// TTS engine (e.g. "kokoro", "qwen", "qwen_custom_voice")
    #[serde(default)]
    pub engine: String,
}

impl Default for VoiceboxTtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_voicebox_url(),
            profile_id: String::new(),
            engine: String::new(),
        }
    }
}

/// Local TTS (Piper) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTtsConfig {
    /// Whether local TTS is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Piper voice name (default: "ryan")
    #[serde(default = "default_local_tts_voice")]
    pub voice: String,
}

impl Default for LocalTtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            voice: default_local_tts_voice(),
        }
    }
}

/// Web Search provider configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebSearchProviders {
    /// EXA search configuration
    #[serde(default)]
    pub exa: Option<ProviderConfig>,

    /// Brave search configuration
    #[serde(default)]
    pub brave: Option<ProviderConfig>,
}

/// Image provider configurations (e.g. Gemini for generation/vision)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageProviders {
    /// Google Gemini image configuration
    #[serde(default)]
    pub gemini: Option<ProviderConfig>,
}

/// Individual provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    /// Provider enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// API key (will be loaded from env or secrets)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// API base URL override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Default model to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,

    /// Available models for this provider (can be updated at runtime)
    #[serde(default)]
    pub models: Vec<String>,

    /// Vision-capable model to use when the default model doesn't support images.
    /// When set and images are present, the provider swaps to this model for that
    /// request only (e.g. `vision_model = "MiniMax-Text-01"` for MiniMax M2.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_model: Option<String>,

    /// Image-generation model override for this provider.
    ///
    /// Wins over the global `image.generation.model` when the active
    /// session's provider has it set. Lets users point `generate_image`
    /// at an alternative without leaving the TUI — e.g.
    /// `generation_model = "imagen-4.0-generate-001"` on the Gemini
    /// provider, or `generation_model = "black-forest-labs/flux-1.1-pro"`
    /// on an OpenRouter / OpenAI-compatible provider that exposes the
    /// `/v1/images/generations` endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_model: Option<String>,

    /// Context window size in tokens for this provider's model.
    /// Used by auto-compaction to know when to summarize history.
    /// Essential for custom/local providers whose models aren't recognized by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,

    /// Endpoint type for providers with multiple API modes (e.g. zhipu: "api" or "coding")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_type: Option<String>,

    /// TTS voice name (e.g. "echo") — only used by TTS providers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,

    /// TTS model override (e.g. "gpt-4o-mini-tts") — only used by TTS providers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Thinking-mode switch for reasoning-capable models.
    ///
    /// Two different pathways honour this flag:
    /// - **DashScope Qwen** (`[providers.qwen]`) — inserted at the top
    ///   level of the request body so the gateway enables Qwen3's hybrid
    ///   reasoning mode. Unset / false keeps the model in fast mode.
    /// - **Local providers** (custom providers whose `base_url` points at
    ///   `localhost`, `*.local`, or an RFC1918 private IP — i.e. a
    ///   self-hosted llama.cpp / MLX / LM Studio / Ollama server) —
    ///   wrapped into `chat_template_kwargs: {"enable_thinking": X}`,
    ///   matching what `llama-server --jinja --chat-template-kwargs`
    ///   does. For local providers the default is `true` (Unsloth's
    ///   default behaviour — letting Qwen/Kimi/DeepSeek templates render
    ///   `<tool_call>` tags correctly); set `enable_thinking = false` in
    ///   the custom provider config to force non-thinking fast mode.
    ///
    /// Cloud providers that aren't Qwen ignore this flag entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_thinking: Option<bool>,

    /// OpenRouter response caching — add `X-OpenRouter-Cache: true` header
    /// to eligible requests. Cached identical requests return in milliseconds
    /// with zero tokens billed. Only effective for OpenRouter endpoints.
    /// Default: false (opt-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_enabled: Option<bool>,

    /// Cache TTL in seconds for OpenRouter response caching (1-86400).
    /// Default: 300 (5 minutes). Only used when cache_enabled is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ttl: Option<u32>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to SQLite database file
    #[serde(default = "default_db_path")]
    pub path: PathBuf,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

fn default_db_path() -> PathBuf {
    opencrabs_home().join("opencrabs.db")
}

/// Expand leading `~` or `~/` in a path to the actual home directory.
fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(rest) = p.strip_prefix("~") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else {
        p.to_path_buf()
    }
}

/// Canonical base directory for the active profile.
///
/// - Default profile: `~/.opencrabs/`
/// - Named profile: `~/.opencrabs/profiles/<name>/`
///
/// Selection priority: `set_active_profile()` > `OPENCRABS_PROFILE` env > default.
pub fn opencrabs_home() -> PathBuf {
    let p = super::profile::resolve_profile_home();
    if !p.exists()
        && let Err(e) = std::fs::create_dir_all(&p)
    {
        tracing::error!("Failed to create opencrabs home directory {p:?}: {e}");
    }
    p
}

/// Daily backup of a config file. One copy per day, keeps `max_days` days.
///
/// Names backups `file.YYYY-MM-DD.bak`. If today's backup already exists,
/// skips (avoids overwriting a clean daily snapshot with mid-day edits).
/// Prunes backups older than `max_days`. Silently ignores errors — backup
/// failure must never block a config write.
pub fn daily_backup(path: &Path, max_days: usize) {
    if !path.exists() {
        return;
    }
    let parent = match path.parent() {
        Some(p) => p,
        None => return,
    };
    let stem = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let today_backup = parent.join(format!("{stem}.{today}.bak"));

    // Skip if today's backup already exists (preserve the day's first snapshot)
    if today_backup.exists() {
        return;
    }

    // Create today's backup
    if let Err(e) = fs::copy(path, &today_backup) {
        tracing::warn!("Failed to back up {} before write: {e}", path.display());
        return;
    }
    tracing::debug!("Daily backup: {}", today_backup.display());

    // Prune old backups beyond max_days
    let prefix = format!("{stem}.");
    let suffix = ".bak";
    if let Ok(entries) = fs::read_dir(parent) {
        let mut backups: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.ends_with(suffix) && name != stem {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        backups.sort();
        backups.reverse(); // newest first
        for old in backups.iter().skip(max_days) {
            let _ = fs::remove_file(parent.join(old));
            tracing::debug!("Pruned old backup: {old}");
        }
    }
}

/// Snapshot current config + keys as "last known good".
///
/// Called after a successful provider response proves the config works.
/// On config parse failure, `Config::load()` falls back to these files.
/// Silently ignores errors — must never block normal operation.
pub fn save_last_good_config() {
    let home = opencrabs_home();

    let config_path = home.join("config.toml");
    let keys_path_src = home.join("keys.toml");
    let config_good = home.join("config.last_good.toml");
    let keys_good = home.join("keys.last_good.toml");

    if config_path.exists() {
        // NEVER snapshot a config that doesn't parse. A raw copy of a broken
        // config.toml poisons the last-good snapshot and defeats recovery
        // entirely — the whole point of the snapshot is to be loadable.
        if let Err(e) = Config::load_from_path(&config_path) {
            tracing::warn!("Refusing last-good snapshot: config.toml does not parse: {e}");
            return;
        }
        if let Err(e) = fs::copy(&config_path, &config_good) {
            tracing::debug!("Failed to save last-good config: {e}");
        }
    }
    if keys_path_src.exists()
        && let Err(e) = fs::copy(&keys_path_src, &keys_good)
    {
        tracing::debug!("Failed to save last-good keys: {e}");
    }
}

/// Try loading config from last-known-good snapshot.
///
/// Returns None if no snapshot exists or if it also fails to parse.
pub fn load_last_good_config() -> Option<Config> {
    let home = opencrabs_home();
    let config_good = home.join("config.last_good.toml");

    if !config_good.exists() {
        return None;
    }

    tracing::warn!("Attempting recovery from last-known-good config");

    // Load base config from the good snapshot
    let mut config = match Config::load_from_path(&config_good) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Last-good config also failed: {e}");
            return None;
        }
    };

    // Try loading keys from good snapshot
    let keys_good = home.join("keys.last_good.toml");
    if keys_good.exists()
        && let Ok(content) = fs::read_to_string(&keys_good)
        && let Ok(keys) = toml::from_str::<KeysFile>(&content)
    {
        config.providers = merge_provider_keys(config.providers, keys.providers);
        config.channels = merge_channel_keys(config.channels, keys.channels);
    }

    tracing::warn!("Recovered config from last-known-good snapshot");
    Some(config)
}

/// Get path to keys.toml - separate file for sensitive API keys
pub fn keys_path() -> PathBuf {
    opencrabs_home().join("keys.toml")
}

/// Read the RAW set of custom provider names from config.toml — no merge,
/// no keys.toml fallback. Used by `cleanup_keys_custom_providers` to break
/// the circular dependency where `Config::load()` (the loader) re-creates
/// missing config entries from keys.toml itself, which then made the
/// "orphan in keys.toml" check pass and skip removal.
///
/// Returns an empty set on any read / parse failure — the cleanup path
/// treats "can't read config" as "nothing in config", which means it
/// won't remove anything destructively from keys.toml.
pub(crate) fn raw_config_custom_provider_names() -> std::collections::HashSet<String> {
    use toml_edit::DocumentMut;
    let path = Config::system_config_path().unwrap_or_else(|| opencrabs_home().join("config.toml"));
    let Ok(content) = std::fs::read_to_string(&path) else {
        return std::collections::HashSet::new();
    };
    let Ok(doc) = content.parse::<DocumentMut>() else {
        return std::collections::HashSet::new();
    };
    doc.as_table()
        .get("providers")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get("custom"))
        .and_then(|t| t.as_table())
        .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
        .unwrap_or_default()
}

/// Save API keys to keys.toml using merge (preserves existing keys).
/// Only writes non-empty api_key values; never deletes other providers' keys.
pub fn save_keys(keys: &ProviderConfigs) -> Result<()> {
    // Merge each provider key individually via write_secret_key (read-modify-write)
    let providers: &[(&str, Option<&ProviderConfig>)] = &[
        ("providers.anthropic", keys.anthropic.as_ref()),
        ("providers.openai", keys.openai.as_ref()),
        ("providers.openrouter", keys.openrouter.as_ref()),
        ("providers.minimax", keys.minimax.as_ref()),
        ("providers.gemini", keys.gemini.as_ref()),
    ];

    for (section, provider) in providers {
        if let Some(p) = provider
            && let Some(key) = &p.api_key
            && !key.is_empty()
        {
            write_secret_key(section, "api_key", key)?;
        }
    }

    // Handle custom providers (flat "default" and named)
    if let Some(customs) = &keys.custom {
        for (name, p) in customs {
            if let Some(key) = &p.api_key
                && !key.is_empty()
            {
                let section = if name == "default" {
                    "providers.custom".to_string()
                } else {
                    format!("providers.custom.{}", name)
                };
                write_secret_key(&section, "api_key", key)?;
            }
        }
    }

    tracing::info!("Saved API keys to: {:?}", keys_path());
    Ok(())
}

/// Write a single key-value pair into keys.toml at the given dotted section path.
///
/// Equivalent to `Config::write_key` but targets `~/.opencrabs/keys.toml`.
/// Use for persisting secrets (tokens, API keys) that must not go into config.toml.
///
/// Normalize a TOML section key: lowercase, replace dots/underscores/spaces
/// with hyphens, strip non-alphanumeric chars (except hyphen).
/// e.g. "Qwen_2.5_4B" → "qwen-2-5-4b", "My Provider" → "my-provider"
pub fn normalize_toml_key(key: &str) -> String {
    key.trim()
        .to_lowercase()
        .replace(['.', '_', ' '], "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// # Example
/// ```no_run
/// # fn main() -> anyhow::Result<()> {
/// use opencrabs::config::write_secret_key;
/// write_secret_key("channels.telegram", "token", "123456:ABC...")?;
/// // results in keys.toml: [channels.telegram] token = "123456:ABC..."
/// # Ok(())
/// # }
/// ```
pub fn write_secret_key(section: &str, key: &str, value: &str) -> Result<()> {
    use toml_edit::DocumentMut;

    // Sanitize: strip carriage returns, take only first token (reject pasted URLs/junk after key)
    let value = value.split(['\r', '\n']).next().unwrap_or("").trim();
    if value.is_empty() {
        return Ok(()); // Don't write empty values
    }

    // Hold lock for entire read-modify-write to prevent races
    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let path = keys_path();

    let mut doc: DocumentMut = if path.exists() {
        fs::read_to_string(&path)?.parse()?
    } else {
        DocumentMut::new()
    };

    // Normalize custom provider names (e.g. "Qwen_2.5_4B" → "qwen-2-5-4b")
    let parts: Vec<String> = section
        .split('.')
        .enumerate()
        .map(|(i, p)| {
            if i >= 2 && section.starts_with("providers.custom") {
                normalize_toml_key(p)
            } else {
                p.to_string()
            }
        })
        .collect();

    // Navigate/create nested tables
    let mut current = doc.as_table_mut();
    for part in &parts {
        if current.get(part.as_str()).is_none() {
            current.insert(part, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        current = current
            .get_mut(part.as_str())
            .context("section not found after insert")?
            .as_table_mut()
            .with_context(|| format!("'{}' is not a table", part))?;
    }
    current.insert(key, toml_edit::value(value));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    daily_backup(&path, 7);
    fs::write(&path, doc.to_string())?;
    tracing::info!("Wrote secret key [{section}].{key}");
    Ok(())
}

/// Keys file structure (keys.toml) - contains sensitive keys and tokens
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeysFile {
    #[serde(default)]
    pub providers: ProviderConfigs,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub a2a: Option<KeysA2a>,
    #[serde(default)]
    pub image: Option<ImageKeys>,
}

/// Image keys section in keys.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageKeys {
    pub api_key: Option<String>,
}

/// A2A keys section in keys.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeysA2a {
    pub api_key: Option<String>,
}

/// Load API keys from keys.toml
/// This file should be chmod 600 for security
fn load_keys_from_file() -> Result<KeysFile> {
    let keys_path = keys_path();
    if !keys_path.exists() {
        return Ok(KeysFile::default());
    }

    tracing::trace!("Loading keys from: {:?}", keys_path);
    let content = std::fs::read_to_string(&keys_path)?;
    let keys: KeysFile = toml::from_str(&content)?;
    Ok(keys)
}

/// Merge API keys from keys.toml into existing provider configs
/// Keys from keys.toml override values in config.toml
pub(crate) fn merge_provider_keys(
    mut base: ProviderConfigs,
    keys: ProviderConfigs,
) -> ProviderConfigs {
    // Guard: never merge the sentinel placeholder that /models uses internally
    let is_real_key = |k: &str| !k.is_empty() && k != "__EXISTING_KEY__";

    // Merge each provider's api_key if present in keys
    if let Some(k) = keys.anthropic
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.anthropic.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(k) = keys.openai
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.openai.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(k) = keys.openrouter
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.openrouter.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    tracing::trace!(
        "merge_provider_keys: minimax keys present={}, base present={}",
        keys.minimax.is_some(),
        base.minimax.is_some()
    );
    if let Some(k) = keys.minimax
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.minimax.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    // Xiaomi: keyless during the free collab window, but if a user supplies
    // their own key (e.g. after the cutoff) merge it like any other provider.
    if let Some(k) = keys.xiaomi
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.xiaomi.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(k) = keys.gemini
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.gemini.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(k) = keys.github
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.github.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    // Merge zhipu
    if let Some(k) = keys.zhipu
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.zhipu.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    // Merge qwen (DashScope API key). Auto-enable + create the entry if
    // keys.toml has a key but config.toml doesn't — the user authenticated
    // through onboarding and wants Qwen on.
    if let Some(k) = keys.qwen
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.qwen.get_or_insert_with(|| ProviderConfig {
            enabled: true,
            ..Default::default()
        });
        entry.api_key = Some(key);
        if entry.default_model.is_none() && k.default_model.is_some() {
            entry.default_model = k.default_model;
        }
        if entry.base_url.is_none() && k.base_url.is_some() {
            entry.base_url = k.base_url;
        }
    }
    // Merge opencode (Go/Zen plan API key). Same auto-enable logic as
    // qwen — `/models` writes the key under `[providers.opencode]` in
    // keys.toml, and without this merge the runtime config never sees
    // it (factory.rs reports "API key missing" and the picker's
    // selection silently fails to take effect).
    if let Some(k) = keys.opencode
        && let Some(key) = k.api_key
        && is_real_key(&key)
    {
        let entry = base.opencode.get_or_insert_with(|| ProviderConfig {
            enabled: true,
            ..Default::default()
        });
        entry.api_key = Some(key);
        if entry.default_model.is_none() && k.default_model.is_some() {
            entry.default_model = k.default_model;
        }
        if entry.base_url.is_none() && k.base_url.is_some() {
            entry.base_url = k.base_url;
        }
    }
    // Merge custom provider keys. Both config.toml and keys.toml go through
    // deserialize_custom_providers which normalizes keys via normalize_toml_key,
    // so names should match exactly (e.g. "opencodeiolo-qwen").
    if let Some(custom_keys) = keys.custom {
        let base_customs = base.custom.get_or_insert_with(BTreeMap::default);
        for (name, key_cfg) in custom_keys {
            if let Some(key) = key_cfg.api_key
                && is_real_key(&key)
            {
                use std::collections::btree_map::Entry;
                match base_customs.entry(name.clone()) {
                    Entry::Occupied(mut occupied) => {
                        tracing::trace!(
                            "merge_provider_keys: merging api_key for custom '{}'",
                            name
                        );
                        occupied.get_mut().api_key = Some(key);
                    }
                    Entry::Vacant(vacant) => {
                        // Key exists in keys.toml but not in config.toml.
                        // Create a minimal entry so the provider can be constructed.
                        tracing::trace!(
                            "merge_provider_keys: custom '{}' has key in keys.toml but no config.toml entry — creating minimal entry",
                            name
                        );
                        vacant.insert(ProviderConfig {
                            api_key: Some(key),
                            base_url: key_cfg.base_url,
                            default_model: key_cfg.default_model,
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }
    // Also handle STT/TTS keys
    if let Some(stt) = keys.stt
        && let Some(groq) = stt.groq
        && let Some(key) = groq.api_key
    {
        let base_stt = base.stt.get_or_insert_with(SttProviders::default);
        let entry = base_stt.groq.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(tts) = keys.tts
        && let Some(openai) = tts.openai
        && let Some(key) = openai.api_key
    {
        let base_tts = base.tts.get_or_insert_with(TtsProviders::default);
        let entry = base_tts.openai.get_or_insert_with(ProviderConfig::default);
        entry.api_key = Some(key);
    }
    if let Some(ws) = keys.web_search {
        let base_ws = base
            .web_search
            .get_or_insert_with(WebSearchProviders::default);
        if let Some(exa) = ws.exa
            && let Some(key) = exa.api_key
            && !key.is_empty()
        {
            let entry = base_ws.exa.get_or_insert_with(ProviderConfig::default);
            entry.api_key = Some(key);
        }
        if let Some(brave) = ws.brave
            && let Some(key) = brave.api_key
            && !key.is_empty()
        {
            let entry = base_ws.brave.get_or_insert_with(ProviderConfig::default);
            entry.api_key = Some(key);
        }
    }
    // Merge image provider keys (e.g. [providers.image.gemini])
    if let Some(img) = keys.image {
        let base_img = base.image.get_or_insert_with(ImageProviders::default);
        if let Some(gemini) = img.gemini
            && let Some(key) = gemini.api_key
            && !key.is_empty()
        {
            let entry = base_img.gemini.get_or_insert_with(ProviderConfig::default);
            entry.api_key = Some(key);
        }
    }
    // Summarise custom-provider key merge at INFO so "auth errors on
    // startup" always have a ground-truth log to correlate with: how
    // many customs exist, which have real keys, which don't.
    if let Some(ref customs) = base.custom {
        let total = customs.len();
        let with_key = customs
            .values()
            .filter(|c| {
                c.api_key
                    .as_ref()
                    .is_some_and(|k| !k.is_empty() && k != "__EXISTING_KEY__")
            })
            .count();
        let missing: Vec<&str> = customs
            .iter()
            .filter(|(_, c)| {
                !c.api_key
                    .as_ref()
                    .is_some_and(|k| !k.is_empty() && k != "__EXISTING_KEY__")
            })
            .map(|(n, _)| n.as_str())
            .collect();
        tracing::trace!(
            "merge_provider_keys: custom providers loaded = {} ({} with real api_key); \
             providers missing a real key: {:?}",
            total,
            with_key,
            missing,
        );
    }
    base
}

/// Merge channel tokens from keys.toml into existing channels config
/// Tokens from keys.toml override values in config.toml
fn merge_channel_keys(mut base: ChannelsConfig, keys: ChannelsConfig) -> ChannelsConfig {
    // Telegram
    if let Some(ref token) = keys.telegram.token
        && !token.is_empty()
    {
        base.telegram.token = Some(token.clone());
    }

    // Discord
    if let Some(ref token) = keys.discord.token
        && !token.is_empty()
    {
        base.discord.token = Some(token.clone());
    }

    // Slack
    if let Some(ref token) = keys.slack.token
        && !token.is_empty()
    {
        base.slack.token = Some(token.clone());
    }
    if let Some(ref app_token) = keys.slack.app_token
        && !app_token.is_empty()
    {
        base.slack.app_token = Some(app_token.clone());
    }

    // WhatsApp uses QR-code pairing stored in session.db — no token to merge.

    // Trello (app_token = API Key, token = API Token)
    if let Some(ref app_token) = keys.trello.app_token
        && !app_token.is_empty()
    {
        base.trello.app_token = Some(app_token.clone());
    }
    if let Some(ref token) = keys.trello.token
        && !token.is_empty()
    {
        base.trello.token = Some(token.clone());
    }

    base
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log to file
    #[serde(default)]
    pub file: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: None,
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            crabrace: CrabraceConfig::default(),
            database: DatabaseConfig {
                path: default_db_path(),
            },
            logging: LoggingConfig {
                level: default_log_level(),
                file: None,
            },
            debug: DebugConfig::default(),
            providers: ProviderConfigs::default(),
            channels: ChannelsConfig::default(),
            agent: AgentConfig::default(),
            daemon: DaemonConfig::default(),
            a2a: A2aConfig::default(),
            image: ImageConfig::default(),
            cron: CronConfig::default(),
            memory: MemoryConfig::default(),
            brain: BrainConfig::default(),
            browser: BrowserConfig::default(),
        }
    }
}

mod loader;
pub use loader::*;
