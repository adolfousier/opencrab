//! Shared slash-command handlers for channel platforms (Telegram, Discord, Slack).
//!
//! Each channel handler calls [`handle_command`] before forwarding to the agent.
//! If the message is a known command, the channel renders the response directly.

use uuid::Uuid;

use crate::brain::agent::AgentService;
use crate::config::Config;
use crate::db::repository::SessionListOptions;
use crate::services::SessionService;

/// Sync the channel agent's provider for a specific session.
///
/// If the session has its own provider/model stored, restore that — so each
/// channel keeps its own provider independently of the TUI or other channels.
/// Only falls back to the global config if the session has no provider set.
pub async fn sync_provider_for_session(
    agent: &AgentService,
    session_id: Uuid,
    session_provider: Option<&str>,
    session_model: Option<&str>,
) {
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "sync_provider_for_session[{}]: Config::load failed: {} — skipping sync",
                session_id,
                e
            );
            return;
        }
    };

    // If the session has an explicit provider, restore it (ignoring global config)
    if let Some(sess_prov) = session_provider {
        let agent_provider = agent.provider_name_for_session(session_id);
        let agent_model = agent.provider_model_for_session(session_id);
        let sess_prov_norm = normalize_provider_name(sess_prov);
        let agent_prov_norm = normalize_provider_name(&agent_provider);
        let same_provider = provider_names_match(&sess_prov_norm, &agent_prov_norm);
        let same_model = session_model.is_none_or(|m| m == agent_model);

        if same_provider && same_model {
            tracing::debug!(
                "sync_provider_for_session[{}]: already on {}/{} — no swap needed",
                session_id,
                agent_provider,
                agent_model
            );
            return;
        }

        tracing::info!(
            "sync_provider_for_session[{}]: session wants {}/{}, agent currently on {}/{} — attempting restore",
            session_id,
            sess_prov,
            session_model.unwrap_or("<default>"),
            agent_provider,
            agent_model,
        );
        // Log whether the config actually has an api_key for this provider
        // to diagnose "Auth error on restart" issues.
        let has_key = config
            .providers
            .custom
            .as_ref()
            .and_then(|c| c.get(sess_prov))
            .and_then(|p| p.api_key.as_ref())
            .is_some_and(|k| !k.is_empty() && k != "__EXISTING_KEY__");
        tracing::info!(
            "sync_provider_for_session[{}]: create_provider_by_name('{}') — config has api_key: {}",
            session_id,
            sess_prov,
            has_key
        );

        match crate::brain::provider::factory::create_provider_by_name(&config, sess_prov).await {
            Ok(new_provider) => {
                tracing::info!(
                    "sync_provider_for_session[{}]: restored {}/{} (was {}/{})",
                    session_id,
                    sess_prov,
                    session_model.unwrap_or("<default>"),
                    agent_provider,
                    agent_model,
                );
                agent.swap_provider_for_session(session_id, new_provider);
                // Pin the saved model so display surfaces match what
                // tool_loop will use for the actual request.
                if let Some(m) = session_model {
                    agent.set_session_model(session_id, m.to_string());
                }
            }
            Err(e) => {
                // Session has a stored provider but we couldn't create it.
                // NEVER fall back to global/TUI provider — sessions are isolated.
                // Leave the agent on whatever provider it currently has.
                // The self-healing fallback chain in tool_loop will handle auth errors.
                tracing::warn!(
                    "sync_provider_for_session[{}]: create_provider_by_name('{}') failed: {} — keeping current provider (session isolation)",
                    session_id,
                    sess_prov,
                    e
                );
            }
        }
    } else {
        // Session has no stored provider — this is the ONLY case where global config applies
        tracing::debug!(
            "sync_provider_for_session[{}]: session has no stored provider — using global config",
            session_id
        );
        let (cfg_provider, cfg_model) = config.providers.active_provider_and_model();
        let agent_provider = agent.provider_name_for_session(session_id);
        let agent_model = agent.provider_model_for_session(session_id);

        let cfg_provider_norm = normalize_provider_name(&cfg_provider);
        let agent_provider_norm = normalize_provider_name(&agent_provider);
        let same_provider = provider_names_match(&cfg_provider_norm, &agent_provider_norm);

        if !same_provider || cfg_model != agent_model {
            match crate::brain::provider::create_provider(&config).await {
                Ok(new_provider) => {
                    tracing::info!(
                        "sync_provider_for_session[{}]: synced to config provider {} (was {})",
                        session_id,
                        cfg_provider,
                        agent_provider,
                    );
                    agent.swap_provider_for_session(session_id, new_provider);
                }
                Err(e) => {
                    tracing::warn!(
                        "sync_provider_for_session[{}]: create_provider(active config) failed: {}",
                        session_id,
                        e
                    );
                }
            }
        }
    }
}

/// Normalize provider names/aliases to stable IDs used by config.
pub(crate) fn normalize_provider_name(name: &str) -> String {
    crate::utils::providers::normalize_provider_name(name)
}

/// Compare normalized provider names, handling custom runtime names (`deepseek`).
pub(crate) fn provider_names_match(config_provider: &str, runtime_provider: &str) -> bool {
    config_provider == runtime_provider
        || config_provider
            .strip_prefix("custom:")
            .is_some_and(|name| name == runtime_provider)
}

/// Result of matching a channel message against known commands.
pub enum ChannelCommand {
    /// `/help` — formatted help text
    Help(String),
    /// `/usage` — formatted session/cost stats
    Usage(String),
    /// `/models` — provider picker (step 1: choose provider, step 2: choose model)
    Models(ProvidersResponse),
    /// `/new` — create a new session and switch to it
    NewSession,
    /// `/sessions` — list recent sessions to switch between
    Sessions(SessionsResponse),
    /// `/stop` — cancel the running agent task
    Stop,
    /// `/compact` — trigger context compaction via the agent
    Compact,
    /// `/doctor` — health check (no LLM needed)
    Doctor,
    /// `/evolve` — check for updates and install directly (no LLM needed)
    Evolve,
    /// `/rtk` — show RTK token savings statistics
    Rtk(String),
    /// User-defined command with action "prompt" — forward prompt text to the agent
    UserPrompt(String),
    /// User-defined command with action "system" — display text directly
    UserSystem(String),
    /// Not a recognised command — pass through to agent
    NotACommand,
}

/// Data for rendering a provider-picker on the channel platform.
pub struct ProvidersResponse {
    pub current_provider: String,
    pub current_model: String,
    /// Available providers (name, display label) that have API keys configured.
    pub providers: Vec<(String, String)>,
    /// Fallback text when platform buttons are unavailable.
    pub text: String,
}

/// Data for rendering a session-picker on the channel platform.
pub struct SessionsResponse {
    pub current_session_id: Uuid,
    /// (session_id, display_label)
    pub sessions: Vec<(Uuid, String)>,
    /// Fallback text when platform buttons are unavailable.
    pub text: String,
}

/// Data for rendering a model-picker after a provider is selected.
pub struct ModelsResponse {
    pub provider_name: String,
    pub current_model: String,
    pub models: Vec<String>,
    /// Fallback text when platform buttons are unavailable.
    pub text: String,
    /// When true, the provider has too many models for inline buttons (OpenRouter, custom).
    /// Channels should switch to default immediately and let the agent handle follow-up.
    pub agent_handled: bool,
}

/// Check if a message is a known channel command and return the response.
/// Commands that produce output are persisted to session history so they
/// appear in TUI and give the agent context about what happened.
pub async fn handle_command(
    text: &str,
    session_id: Uuid,
    agent: &AgentService,
    session_svc: &SessionService,
) -> ChannelCommand {
    let trimmed = text.trim();
    let result = match trimmed {
        "/compact" => ChannelCommand::Compact,
        "/doctor" => ChannelCommand::Doctor,
        "/evolve" => ChannelCommand::Evolve,
        "/help" => ChannelCommand::Help(format_help()),
        "/models" => ChannelCommand::Models(format_providers(agent)),
        "/new" => ChannelCommand::NewSession,
        "/rtk" => ChannelCommand::Rtk(format_rtk().await),
        "/sessions" => ChannelCommand::Sessions(format_sessions(session_id, session_svc).await),
        "/stop" => ChannelCommand::Stop,
        "/usage" => ChannelCommand::Usage(format_usage(session_id, agent, session_svc).await),
        _ if trimmed.starts_with('/') && !crate::utils::string::looks_like_file_path(trimmed) => {
            match_user_command(trimmed)
        }
        _ => ChannelCommand::NotACommand,
    };

    // Persist command + response to session history
    let response_text = match &result {
        ChannelCommand::Help(body) | ChannelCommand::Usage(body) => Some(body.clone()),
        ChannelCommand::Models(resp) => Some(resp.text.clone()),
        ChannelCommand::Sessions(resp) => Some(resp.text.clone()),
        ChannelCommand::NewSession => Some("New session started.".to_string()),
        ChannelCommand::Stop => Some("Operation stopped.".to_string()),
        ChannelCommand::UserSystem(body) => Some(body.clone()),
        ChannelCommand::Doctor => Some("Running health check...".to_string()),
        ChannelCommand::Evolve => Some("Checking for updates...".to_string()),
        ChannelCommand::Rtk(body) => Some(body.clone()),
        ChannelCommand::Compact | ChannelCommand::UserPrompt(_) | ChannelCommand::NotACommand => {
            None
        }
    };

    if let Some(response) = response_text {
        persist_command_to_history(agent, session_id, trimmed, &response).await;
    }

    result
}

/// Save the user command and bot response to session message history,
/// then notify TUI so it refreshes live.
async fn persist_command_to_history(
    agent: &AgentService,
    session_id: Uuid,
    command: &str,
    response: &str,
) {
    let msg_svc = crate::services::MessageService::new(agent.context().clone());
    if let Err(e) = msg_svc
        .create_message(session_id, "user".to_string(), command.to_string())
        .await
    {
        tracing::warn!("Failed to persist channel command to history: {}", e);
    }
    if let Err(e) = msg_svc
        .create_message(session_id, "assistant".to_string(), response.to_string())
        .await
    {
        tracing::warn!(
            "Failed to persist channel command response to history: {}",
            e
        );
    }
    // Notify TUI to reload session messages (same mechanism as agent responses)
    if let Some(tx) = agent.session_updated_tx() {
        let _ = tx.send(crate::brain::agent::ChannelSessionEvent::Updated(
            session_id,
        ));
    }
}

// ── User-defined commands ───────────────────────────────────────────────────

fn match_user_command(text: &str) -> ChannelCommand {
    let brain_path = crate::brain::BrainLoader::resolve_path();
    let loader = crate::brain::CommandLoader::from_brain_path(&brain_path);
    let commands = loader.load();
    let skills = crate::brain::skills::load_all_skills();
    match_user_command_inner(text, &commands, &skills)
}

pub(crate) fn match_user_command_inner(
    text: &str,
    commands: &[crate::brain::commands::UserCommand],
    skills: &[crate::brain::skills::Skill],
) -> ChannelCommand {
    // Split "/command args" into command name and optional args
    let (cmd_name, args) = text
        .split_once(' ')
        .map(|(c, a)| (c, a.trim()))
        .unwrap_or((text, ""));

    // 1. Explicit user-defined commands win — they're how a user overrides
    //    a built-in skill (rename, retarget, swap to action=system, etc.).
    if let Some(cmd) = commands.iter().find(|c| c.name == cmd_name) {
        let prompt = if args.is_empty() {
            cmd.prompt.clone()
        } else {
            format!("{} {}", cmd.prompt, args)
        };
        return match cmd.action.as_str() {
            "system" => ChannelCommand::UserSystem(prompt),
            _ => ChannelCommand::UserPrompt(prompt),
        };
    }

    // 2. Auto-registered skills — `/<name>` matches a SKILL.md slug, the body
    //    becomes the prompt. Args (anything after the first space) are
    //    appended so callers can pass extra context without writing a
    //    custom commands.toml wrapper.
    if let Some(skill) = skills.iter().find(|s| s.slash_name == cmd_name) {
        let prompt = if args.is_empty() {
            skill.body.clone()
        } else {
            format!("{}\n\n{}", skill.body, args)
        };
        return ChannelCommand::UserPrompt(prompt);
    }

    ChannelCommand::NotACommand
}

// ── /help ───────────────────────────────────────────────────────────────────

pub(crate) fn format_help() -> String {
    let mut lines = vec![
        "📖 *Available Commands*".to_string(),
        String::new(),
        "`/compact`  — Compact context (summarize & trim)".to_string(),
        "`/evolve`   — Download latest release & restart".to_string(),
        "`/help`     — Show this message".to_string(),
        "`/models`   — Switch AI model".to_string(),
        "`/new`      — Start a new session".to_string(),
        "`/rtk`      — Show RTK token savings statistics".to_string(),
        "`/sessions` — Switch between sessions".to_string(),
        "`/stop`     — Abort current operation".to_string(),
        "`/usage`    — Session token & cost stats".to_string(),
    ];

    // Append user-defined commands from commands.toml
    let brain_path = crate::brain::BrainLoader::resolve_path();
    let loader = crate::brain::CommandLoader::from_brain_path(&brain_path);
    let mut user_cmds = loader.load();
    if !user_cmds.is_empty() {
        user_cmds.sort_by(|a, b| a.name.cmp(&b.name));
        lines.push(String::new());
        lines.push("📌 *Custom Commands*".to_string());
        for cmd in &user_cmds {
            lines.push(format!("`{}`  — {}", cmd.name, cmd.description));
        }
    }

    lines.push(String::new());
    lines.push("🦀 Any other message is sent to OpenCrabs. 🦀".to_string());
    lines.join("\n")
}

// ── /rtk ────────────────────────────────────────────────────────────────────

#[cfg(feature = "rtk")]
async fn format_rtk() -> String {
    match tokio::process::Command::new("rtk")
        .arg("gain")
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if output.status.success() {
                format!("📊 *RTK Token Savings:*\n\n```\n{}\n```", stdout.trim())
            } else {
                format!("⚠️ RTK gain command failed:\n\n```\n{}\n```", stderr.trim())
            }
        }
        Err(e) => {
            format!("⚠️ Failed to run rtk gain: {}. Is RTK installed?", e)
        }
    }
}

#[cfg(not(feature = "rtk"))]
async fn format_rtk() -> String {
    "⚠️ RTK feature is not enabled. Rebuild with --features rtk to enable token savings tracking."
        .to_string()
}

// ── /usage ──────────────────────────────────────────────────────────────────

async fn format_usage(
    session_id: Uuid,
    agent: &AgentService,
    session_svc: &SessionService,
) -> String {
    use crate::usage::data::{DashboardData, Period, fmt_cost, fmt_tokens};

    let mut lines = vec!["📊 *Usage Dashboard*".to_string(), String::new()];

    // ── Current session (header) ─────────────────────────────────────
    let current_model = agent.provider_model();
    match session_svc.get_session(session_id).await {
        Ok(Some(session)) => {
            let name = session.title.as_deref().unwrap_or("Current Session");
            let model = session
                .model
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or(&current_model);
            let tokens = session.token_count;
            let cost = if session.total_cost > 0.0 {
                session.total_cost
            } else if tokens > 0 {
                estimate_cost(model, tokens as i64).unwrap_or(0.0)
            } else {
                0.0
            };
            lines.push(format!("*Current:* {}", name));
            lines.push(format!(
                "  `{}` · {} tok · ${:.4}",
                model,
                format_number(tokens as i64),
                cost
            ));
        }
        _ => {
            lines.push("*Current:* (session not found)".to_string());
        }
    }
    lines.push(String::new());

    // ── Period cards (Today + All-Time, both rendered inline) ────────
    // TUI /usage lets the user cycle T/W/M/A — for the channel dump
    // we show Today and All-Time since those are the two most useful
    // snapshots. Each block is the full five-card breakdown
    // (summary + by model + by tool + by project + by activity + daily
    // strip). Keeps parity with the dashboard without blowing past
    // Telegram's 4096-char limit for normal sessions.
    for period in [Period::Today, Period::AllTime] {
        let pool = session_svc.pool();
        let data = match DashboardData::fetch(&pool, period).await {
            Ok(d) => d,
            Err(e) => {
                lines.push(format!("*{}:* (failed to load: {})", period.label(), e));
                lines.push(String::new());
                continue;
            }
        };

        lines.push(format!("━━ *{}* ━━", period.label()));
        lines.push(format!(
            "{} tok · {} · {} sessions · {} calls",
            fmt_tokens(data.summary.total_tokens),
            fmt_cost(data.summary.total_cost),
            format_number(data.summary.session_count),
            format_number(data.summary.call_count),
        ));

        if !data.daily.is_empty() && period != Period::Today {
            lines.push(String::new());
            lines.push("*Daily:*".to_string());
            // Last 7 days of the window
            let window: Vec<_> = data.daily.iter().rev().take(7).collect();
            for d in window.iter().rev() {
                lines.push(format!(
                    "  {} · {} tok · {}",
                    d.date,
                    fmt_tokens(d.tokens),
                    fmt_cost(d.cost)
                ));
            }
        }

        if !data.models.is_empty() {
            lines.push(String::new());
            lines.push("*By Model:*".to_string());
            for m in data.models.iter().take(5) {
                let est = if m.estimated { " ~" } else { "" };
                lines.push(format!(
                    "  `{}` · {} tok · {}{}",
                    m.model,
                    fmt_tokens(m.tokens),
                    fmt_cost(m.cost),
                    est
                ));
            }
        }

        if !data.tools.is_empty() {
            lines.push(String::new());
            lines.push("*Core Tools:*".to_string());
            for t in data.tools.iter().take(5) {
                lines.push(format!(
                    "  `{}` · {} calls",
                    t.tool_name,
                    format_number(t.call_count)
                ));
            }
        }

        if !data.projects.is_empty() {
            lines.push(String::new());
            lines.push("*By Project:*".to_string());
            for p in data.projects.iter().take(5) {
                lines.push(format!(
                    "  `{}` · {} · {} sessions",
                    p.project,
                    fmt_cost(p.cost),
                    p.sessions
                ));
            }
        }

        if !data.activities.is_empty() {
            lines.push(String::new());
            lines.push("*By Activity:*".to_string());
            for a in data.activities.iter().take(5) {
                lines.push(format!(
                    "  {} · {} · {} turns · {:.0}% one-shot",
                    a.category,
                    fmt_cost(a.cost),
                    a.turns,
                    a.one_shot_pct,
                ));
            }
        }

        lines.push(String::new());
    }

    lines.join("\n")
}

fn estimate_cost(model: &str, token_count: i64) -> Option<f64> {
    crate::usage::pricing::PricingConfig::load()
        .ok()
        .and_then(|cfg| cfg.estimate_cost(model, token_count))
}

pub(crate) fn format_number(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ── /sessions ──────────────────────────────────────────────────────────────

async fn format_sessions(
    current_session_id: Uuid,
    session_svc: &SessionService,
) -> SessionsResponse {
    let sessions = session_svc
        .list_sessions(SessionListOptions {
            include_archived: false,
            limit: Some(10),
            offset: 0,
        })
        .await
        .unwrap_or_default();

    let mut text_lines = vec!["📂 *Sessions*".to_string(), String::new()];
    let mut items = Vec::new();

    for s in &sessions {
        let title = s.title.as_deref().unwrap_or("Untitled");
        let marker = if s.id == current_session_id {
            " ✓"
        } else {
            ""
        };
        let date = s.updated_at.format("%b %d %H:%M");
        let label = format!("{} ({})", title, date);
        text_lines.push(format!("• `{}`{}", label, marker));
        items.push((s.id, label));
    }

    if sessions.is_empty() {
        text_lines.push("No sessions found.".to_string());
    }

    SessionsResponse {
        current_session_id,
        sessions: items,
        text: text_lines.join("\n"),
    }
}

// ── /models ─────────────────────────────────────────────────────────────────

fn format_providers(agent: &AgentService) -> ProvidersResponse {
    // Use the agent's ACTUAL current provider/model, not config.toml.
    // Channel model switches call agent.swap_provider() without touching config,
    // so reading from Config::load() shows stale data after a channel switch.
    let current_provider = agent.provider_name();
    let current_model = agent.provider_model();

    let providers = configured_providers();

    let mut text_lines = vec![
        "🤖 *Switch Provider*".to_string(),
        format!("Current: `{}` / `{}`", current_provider, current_model),
        String::new(),
    ];
    for (name, label) in &providers {
        let marker = if *name == current_provider {
            " ✓"
        } else {
            ""
        };
        text_lines.push(format!("• `{}`{}", label, marker));
    }

    ProvidersResponse {
        current_provider: current_provider.clone(),
        current_model: current_model.clone(),
        providers,
        text: text_lines.join("\n"),
    }
}

/// List configured providers (those with API keys set or enabled CLI providers).
fn configured_providers() -> Vec<(String, String)> {
    let config = match crate::config::Config::load() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    crate::utils::providers::configured_providers(&config.providers)
}

/// Fetch models for a specific provider (called from callback handler).
pub async fn models_for_provider(provider_name: &str) -> ModelsResponse {
    let config = match crate::config::Config::load() {
        Ok(c) => c,
        Err(_) => {
            return ModelsResponse {
                provider_name: provider_name.to_string(),
                current_model: String::new(),
                models: vec![],
                text: "Failed to load config.".to_string(),
                agent_handled: false,
            };
        }
    };

    let display_name = provider_display_name(provider_name);
    let config_models = provider_config_models(&config, provider_name);

    // CLI providers (Claude CLI, OpenCode CLI) don't need the binary to list models.
    // They have hardcoded supported_models() and don't require API keys.
    // If the binary isn't installed on the server, create_provider_by_name would fail,
    // but we can still show models from config or hardcoded defaults.
    let is_cli_provider = matches!(
        provider_name,
        "claude-cli" | "claude_cli" | "opencode-cli" | "opencode_cli"
    );

    if is_cli_provider {
        // Get default model from config or use hardcoded fallback
        let current_model = config_models.first().cloned().unwrap_or_else(|| {
            if provider_name.starts_with("claude") {
                "opus-4-7".to_string()
            } else {
                "sonnet-4.5".to_string()
            }
        });

        // Hardcoded supported models for CLI providers (no binary needed)
        let models = if !config_models.is_empty() {
            config_models
        } else if provider_name.starts_with("claude") {
            vec![
                "opus-4-7".to_string(),
                "sonnet-4-6".to_string(),
                "haiku-4-5".to_string(),
            ]
        } else {
            vec!["sonnet-4.5".to_string(), "opus-4.1".to_string()]
        };

        let mut text_lines = vec![
            format!("🤖 *{} Models*", display_name),
            format!("Current: `{}`", current_model),
            String::new(),
        ];
        for (i, m) in models.iter().enumerate() {
            let marker = if *m == current_model { " ✓" } else { "" };
            text_lines.push(format!("{}. `{}`{}", i + 1, m, marker));
        }

        return ModelsResponse {
            provider_name: provider_name.to_string(),
            current_model,
            models,
            text: text_lines.join("\n"),
            agent_handled: false,
        };
    }

    // OpenRouter (300+ models) and custom providers skip live fetch on channels.
    // Show config models if available, otherwise fall back to the provider's
    // actual default_model from config (never invent fake '-default' names).
    if provider_name == "openrouter" || provider_name.starts_with("custom:") {
        let config_default = crate::utils::providers::config_for(&config.providers, provider_name)
            .and_then(|c| c.default_model.clone());
        let current_model = config_models
            .first()
            .cloned()
            .or(config_default)
            .unwrap_or_else(|| {
                if provider_name.starts_with("custom:") {
                    "unknown (no models configured)".to_string()
                } else {
                    "openrouter-default".to_string()
                }
            });

        let models = if !config_models.is_empty() {
            config_models
        } else {
            vec![current_model.clone()]
        };

        let mut text_lines = vec![
            format!("🤖 *{} Models*", display_name),
            format!("Current: `{}`", current_model),
            String::new(),
        ];
        for (i, m) in models.iter().enumerate() {
            let marker = if *m == current_model { " ✓" } else { "" };
            text_lines.push(format!("{}. `{}`{}", i + 1, m, marker));
        }

        return ModelsResponse {
            provider_name: provider_name.to_string(),
            current_model,
            models,
            text: text_lines.join("\n"),
            agent_handled: false,
        };
    }

    // Standard API providers: create provider and fetch models
    let provider = match crate::brain::provider::factory::create_provider_by_name(
        &config,
        provider_name,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            return ModelsResponse {
                provider_name: provider_name.to_string(),
                current_model: String::new(),
                models: vec![],
                text: format!("Failed to create provider: {}", e),
                agent_handled: false,
            };
        }
    };

    let current_model = provider.default_model().to_string();

    // Standard providers: config models first (instant), then live fetch with timeout.
    let mut models = if !config_models.is_empty() {
        config_models
    } else {
        match tokio::time::timeout(std::time::Duration::from_secs(10), provider.fetch_models())
            .await
        {
            Ok(fetched) if !fetched.is_empty() => fetched,
            Ok(_) => vec![current_model.clone()],
            Err(_) => {
                tracing::warn!("fetch_models timed out for '{}'", provider_name);
                vec![current_model.clone()]
            }
        }
    };

    // Ensure current model is in the list
    if !models.contains(&current_model) {
        models.insert(0, current_model.clone());
    }

    let mut text_lines = vec![
        format!("🤖 *{} Models*", display_name),
        format!("Current: `{}`", current_model),
        String::new(),
    ];
    for (i, m) in models.iter().enumerate() {
        let marker = if *m == current_model { " ✓" } else { "" };
        text_lines.push(format!("{}. `{}`{}", i + 1, m, marker));
    }

    ModelsResponse {
        provider_name: provider_name.to_string(),
        current_model,
        models,
        text: text_lines.join("\n"),
        agent_handled: false,
    }
}

/// Get models from the provider's config section (for providers without /models endpoint).
fn provider_config_models(config: &crate::config::Config, name: &str) -> Vec<String> {
    crate::utils::providers::config_for(&config.providers, name)
        .map(|c| c.models.clone())
        .unwrap_or_default()
}

pub fn provider_display_name(name: &str) -> &str {
    crate::utils::providers::display_name(name)
}

// ── Model switching ─────────────────────────────────────────────────────────

/// Switch the active model for this session's provider.
///
/// Persists provider + model to the session DB record so the session keeps
/// its own provider independently. Does NOT toggle global config enabled flags
/// — that would leak into other sessions/channels.
/// Saves a `[Model changed to ...]` message to the session history so the agent
/// is aware of the switch.
/// Returns an error message on failure so channels can report it to the user.
pub async fn switch_model(
    agent: &AgentService,
    model_name: &str,
    session_id: Option<uuid::Uuid>,
    provider_name_override: Option<&str>,
) -> Result<String, String> {
    // Provider name MUST come from the caller (callback data) when available.
    // Falling back to agent state caused crossed pairs when the in-memory
    // slot was stale or another session had just nudged the global default.
    let provider_name = match provider_name_override {
        Some(p) => p.to_string(),
        None => match session_id {
            Some(sid) => agent.provider_name_for_session(sid),
            None => agent.provider_name(),
        },
    };

    let config =
        crate::config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    tracing::info!(
        "Channel: switched model to {} (provider: {}, session: {:?})",
        model_name,
        provider_name,
        session_id
    );

    // Create provider by name (doesn't modify global config enabled flags)
    let new_provider =
        crate::brain::provider::factory::create_provider_by_name(&config, &provider_name)
            .await
            .map_err(|e| {
                tracing::warn!("Failed to create provider after model switch: {}", e);
                format!("Model saved but failed to reload provider: {}", e)
            })?;
    let display_name = provider_display_name(&provider_name);
    // Pin per-session when possible; only touch the global slot for
    // callers without a session (kept for the bootstrap path).
    match session_id {
        Some(sid) => {
            agent.swap_provider_for_session(sid, new_provider);
            // The freshly-created provider reports the global config's
            // default model from `default_model()`, not the model the
            // user just picked. Pin the per-session override so every
            // "current model" display surface (TUI status bar,
            // /sessions, channel footers) matches what tool_loop will
            // actually send on the wire (which already reads
            // `session.model` from the DB row).
            agent.set_session_model(sid, model_name.to_string());
        }
        None => agent.swap_provider(new_provider),
    }

    let change_msg = format!("[Model changed to {}/{}]", display_name, model_name);

    // Persist provider + model to session DB record so it survives restarts
    if let Some(sid) = session_id {
        let session_svc = crate::services::SessionService::new(agent.context().clone());
        if let Ok(Some(mut session)) = session_svc.get_session(sid).await {
            session.provider_name = Some(provider_name.clone());
            session.model = Some(model_name.to_string());
            if let Err(e) = session_svc.update_session(&session).await {
                tracing::warn!("Failed to persist provider to session: {}", e);
            }
        }

        // Persist change message to session history so the agent knows
        let msg_svc = crate::services::MessageService::new(agent.context().clone());
        if let Err(e) = msg_svc
            .create_message(sid, "user".to_string(), change_msg.clone())
            .await
        {
            tracing::warn!("Failed to persist model-change message: {}", e);
        }
    }

    Ok(change_msg)
}
/// Run evolve directly (no LLM needed). Returns a user-facing status message.
/// Handles the RestartReady signal by triggering a process restart via exec().
pub async fn run_evolve() -> String {
    use crate::brain::agent::ProgressEvent;
    use crate::brain::tools::{Tool, ToolExecutionContext, evolve::EvolveTool};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    // Track whether we received a RestartReady signal
    let restart_ready = Arc::new(AtomicBool::new(false));
    let restart_flag = restart_ready.clone();

    // Create a progress callback that detects RestartReady
    let progress_callback: crate::brain::agent::ProgressCallback = Arc::new(move |_sid, event| {
        if matches!(event, ProgressEvent::RestartReady { .. }) {
            restart_flag.store(true, Ordering::SeqCst);
        }
    });

    let ctx = ToolExecutionContext::new(uuid::Uuid::nil());
    let tool = EvolveTool::new(Some(progress_callback));
    let result = match tool
        .execute(serde_json::json!({"check_only": false}), &ctx)
        .await
    {
        Ok(result) => result.output,
        Err(e) => format!("Evolve failed: {}", e),
    };

    // If we received a RestartReady signal, trigger the restart
    if restart_ready.load(Ordering::SeqCst) {
        trigger_restart();
    }

    result
}

/// Trigger a process restart by exec-ing the current binary.
/// This replaces the current process with a fresh instance.
#[cfg(unix)]
fn trigger_restart() {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("opencrabs"));
    let args: Vec<String> = std::env::args().skip(1).collect();

    tracing::info!("Restarting daemon via exec()");

    // exec() replaces the current process, so this never returns on success
    let err = std::process::Command::new(&exe).args(&args).exec();
    tracing::error!("exec() failed: {}", err);
}

#[cfg(not(unix))]
fn trigger_restart() {
    tracing::warn!("Restart via exec() not supported on this platform. Manual restart required.");
}

/// Run doctor health check directly (no LLM needed). Returns a user-facing status message.
pub fn run_doctor() -> String {
    use crate::brain::tools::slash_command::SlashCommandTool;

    // Reuse the slash command tool's doctor logic
    SlashCommandTool::doctor_text()
}

/// Try to execute a command that returns a simple text response (no platform-specific UI).
/// Returns `Some(text)` for commands handled here, `None` for commands that need
/// platform-specific rendering (Models, Sessions, NewSession) or agent passthrough.
/// Channels call this first — if it returns Some, send the text and return.
pub async fn try_execute_text_command(cmd: &ChannelCommand) -> Option<String> {
    match cmd {
        ChannelCommand::Help(body)
        | ChannelCommand::Usage(body)
        | ChannelCommand::UserSystem(body)
        | ChannelCommand::Rtk(body) => Some(body.clone()),
        ChannelCommand::Doctor => Some(run_doctor()),
        ChannelCommand::Evolve => Some(run_evolve().await),
        _ => None,
    }
}

/// Map a provider name to its config section key.
#[cfg(test)]
pub(crate) fn provider_section(provider_name: &str) -> Option<String> {
    crate::utils::providers::config_section(provider_name)
}
