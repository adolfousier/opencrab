//! Command Code CLI Provider — direct subprocess integration
//!
//! Spawns the `cmd` CLI binary (Command Code) in non-interactive mode
//! (`cmd -p`) and reads its plain-text output, converting it to standard
//! `StreamEvent`s. OpenCrabs handles all tools, memory, and context
//! locally; Command Code is used as the LLM backend so users can piggyback
//! on their existing Command Code account/auth (`~/.commandcode/auth.json`)
//! without needing a separate API key.
//!
//! Unlike the Claude/Codex CLIs, `cmd -p` emits plain text (not NDJSON), so
//! this provider spawns the subprocess, collects stdout, and emits a single
//! `MessageStart` -> `ContentBlockDelta` -> `MessageDelta`/`MessageStop`
//! translation. The model list mirrors `cmd --list-models` (abridged to the
//! recommended set); the CLI itself validates the model name.

use super::error::{ProviderError, Result};
use super::r#trait::{Provider, ProviderStream};
use super::types::*;
use async_trait::async_trait;
use futures::stream::StreamExt;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;

/// Command Code CLI provider — talks directly to the `cmd` binary.
#[derive(Clone)]
pub struct CommandCodeCliProvider {
    cmd_path: String,
    default_model: String,
    /// User override from `providers.command_code_cli.context_window` in config.toml.
    configured_context_window: Option<u32>,
}

impl CommandCodeCliProvider {
    /// Create a new provider, auto-detecting the `cmd` binary.
    pub fn new() -> Result<Self> {
        let path = resolve_cmd_path()?;
        Ok(Self {
            cmd_path: path,
            default_model: DEFAULT_MODEL.to_string(),
            configured_context_window: None,
        })
    }

    /// Override the context-window budget from `providers.command_code_cli.context_window`.
    pub fn with_context_window(mut self, context_window: u32) -> Self {
        self.configured_context_window = Some(context_window);
        self
    }

    /// Override the default model (e.g. a `cmd --list-models` id).
    pub fn with_default_model(mut self, model: String) -> Self {
        self.default_model = model;
        self
    }

    /// Build a plain-text prompt from LLMRequest messages.
    fn build_prompt(request: &LLMRequest) -> String {
        let mut parts = Vec::new();

        if let Some(ref system) = request.system
            && !system.is_empty()
        {
            parts.push(system.clone());
        }

        for msg in &request.messages {
            let role = match msg.role {
                Role::User => "Human",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            let content: String = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => Some(format!("[tool_result for {}]: {}", tool_use_id, content)),
                    ContentBlock::ToolUse { id, name, input } => {
                        Some(format!("[tool_use {} ({}): {}]", name, id, input))
                    }
                    ContentBlock::Thinking { thinking, .. } => {
                        if thinking.is_empty() {
                            None
                        } else {
                            Some(format!("<thinking>{}</thinking>", thinking))
                        }
                    }
                    ContentBlock::Image { source } => {
                        Some(match source {
                            ImageSource::Base64 { media_type, data } => {
                                let ext = match media_type.as_str() {
                                    "image/png" => "png",
                                    "image/jpeg" => "jpeg",
                                    "image/gif" => "gif",
                                    "image/webp" => "webp",
                                    _ => "png",
                                };
                                let tmp = std::env::temp_dir().join(format!(
                                    "opencrabs_cmd_img_{}.{}",
                                    uuid::Uuid::new_v4(),
                                    ext
                                ));
                                use base64::Engine;
                                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
                                    && std::fs::write(&tmp, &bytes).is_ok()
                                {
                                    format!(
                                        "[User attached an image at {}. Use the analyze_image tool to view it.]",
                                        tmp.display()
                                    )
                                } else {
                                    "[User attached an image but it could not be decoded.]".to_string()
                                }
                            }
                            ImageSource::Url { url } => {
                                format!(
                                    "[User attached an image: {}. Use the analyze_image tool to view it.]",
                                    url
                                )
                            }
                        })
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            if content.trim().is_empty() {
                continue;
            }
            parts.push(format!("{}: {}", role, content));
        }

        parts.join("\n\n")
    }
}

/// Resolve the `cmd` CLI binary path.
fn resolve_cmd_path() -> Result<String> {
    if let Ok(path) = std::env::var("CMD_PATH") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
        return Err(ProviderError::Internal(format!(
            "CMD_PATH set but not found: {}",
            path
        )));
    }

    for candidate in &[
        std::path::PathBuf::from("/usr/local/bin/cmd"),
        std::path::PathBuf::from("/usr/bin/cmd"),
        std::path::PathBuf::from("/opt/homebrew/bin/cmd"),
    ] {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    if let Some(path) = super::which_binary("cmd") {
        return Ok(path);
    }

    Err(ProviderError::Internal(
        "Command Code CLI (`cmd`) not found — install `command-code` or set CMD_PATH".to_string(),
    ))
}

/// Canonical model list for Command Code CLI. Abridged from `cmd --list-models`;
/// the CLI validates whatever the account can access. Adding or removing a
/// variant only requires editing this const.
pub(crate) const SUPPORTED_MODELS: &[&str] = &[
    "taste-1",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-flash",
    "moonshotai/Kimi-K2.7-Code",
    "moonshotai/Kimi-K2.6",
    "zai-org/GLM-5.2",
    "zai-org/GLM-5.1",
    "xiaomi/mimo-v2.5-pro",
    "xiaomi/mimo-v2.5",
    "MiniMaxAI/MiniMax-M3",
    "MiniMaxAI/MiniMax-M2.7",
    "Qwen/Qwen3.6-Max-Preview",
];

/// Default model when no per-session override is set.
pub(crate) const DEFAULT_MODEL: &str = "taste-1";

#[async_trait]
impl Provider for CommandCodeCliProvider {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let mut stream = self.stream(request).await?;

        let mut id = String::new();
        let mut model = String::new();
        let mut content = Vec::new();
        let mut stop_reason = None;
        let mut usage = TokenUsage::default();
        let mut text_buf = String::new();

        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::MessageStart { message } => {
                    id = message.id;
                    model = message.model;
                    usage = message.usage;
                }
                StreamEvent::ContentBlockDelta {
                    delta: ContentDelta::TextDelta { text },
                    ..
                } => {
                    text_buf.push_str(&text);
                }
                StreamEvent::MessageDelta { delta: d, usage: u } => {
                    stop_reason = d.stop_reason;
                    usage.input_tokens = u.input_tokens;
                    usage.output_tokens = u.output_tokens;
                    usage.cache_read_tokens = u.cache_read_tokens;
                }
                StreamEvent::MessageStop => break,
                _ => {}
            }
        }

        if !text_buf.is_empty() {
            content.push(ContentBlock::Text { text: text_buf });
        }

        Ok(LLMResponse {
            id,
            model,
            content,
            stop_reason,
            usage,
            streaming_active_secs: None,
        })
    }

    async fn stream(&self, request: LLMRequest) -> Result<ProviderStream> {
        let prompt = Self::build_prompt(&request);
        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        let cwd = request
            .working_directory
            .as_deref()
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")));

        tracing::info!(
            "Spawning Command Code CLI: model={}, prompt_len={}, cwd={}",
            model,
            prompt.len(),
            cwd.display()
        );

        let mut child = tokio::process::Command::new(&self.cmd_path)
            // Non-interactive pipe mode: read prompt from stdin, print response, exit.
            .arg("-p")
            // Run with the configured model for this session.
            .arg("-m")
            .arg(&model)
            // Bypass permission prompts — OpenCrabs owns the trust boundary at
            // the channel level (TUI / Telegram / Slack), not at the cmd level.
            .arg("--yolo")
            // Skip onboarding/telemetry nags in automation.
            .arg("--skip-onboarding")
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Internal(format!("failed to spawn Command Code CLI: {}", e)))?;

        // Write prompt via stdin to avoid leaking in `ps aux`.
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::Internal("failed to capture stdin".to_string()))?;
        let prompt_bytes = prompt.into_bytes();
        tokio::spawn(async move {
            if let Err(e) = stdin.write_all(&prompt_bytes).await {
                tracing::warn!("Command Code CLI stdin write failed: {}", e);
            }
            if let Err(e) = stdin.shutdown().await {
                tracing::debug!("Command Code CLI stdin shutdown: {}", e);
            }
        });

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::Internal("failed to capture stdout".to_string()))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::Internal("failed to capture stderr".to_string()))?;

        // Surface stderr — cmd prints auth errors, version banners, and tips there.
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    tracing::warn!("Command Code CLI stderr: {}", line);
                }
            }
        });

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(64);
        let model_for_task = model.clone();

        tokio::spawn(async move {
            let mut stdout_reader = tokio::io::BufReader::new(stdout);
            let mut stdout_text = String::new();
            let mut saw_error = false;
            let mut error_text = String::new();

            // `cmd -p` emits plain text on stdout. Read it all, then translate
            // to the StreamEvent contract once (complete()/proxy helpers expect
            // a MessageStart before any deltas).
            let read_result = stdout_reader.read_to_string(&mut stdout_text).await;

            if let Err(e) = read_result {
                tracing::error!("Command Code CLI stdout read error: {}", e);
                let _ = tx
                    .send(Err(ProviderError::Internal(format!(
                        "Command Code CLI stdout read error: {}",
                        e
                    ))))
                    .await;
                let _ = child.kill().await;
                return;
            }

            let trimmed = stdout_text.trim();

            // Heuristic error detection — `cmd -p` may emit error text on stdout
            // for auth/rate-limit failures. Surface rate limits so the FallbackProvider
            // can swap to the next provider.
            let lower = trimmed.to_lowercase();
            if lower.contains("rate limit")
                || lower.contains("429")
                || lower.contains("overloaded")
                || lower.contains("capacity")
                || lower.contains("hit your limit")
            {
                saw_error = true;
                error_text = trimmed.to_string();
            } else if lower.contains("context length")
                || lower.contains("too many tokens")
                || lower.contains("prompt is too long")
            {
                let _ = tx.send(Err(ProviderError::ContextLengthExceeded(0))).await;
                let _ = child.wait().await;
                return;
            }

            let msg_id = format!("msg_{}", uuid::Uuid::new_v4().simple());

            if !saw_error && !trimmed.is_empty() {
                let _ = tx
                    .send(Ok(StreamEvent::MessageStart {
                        message: StreamMessage {
                            id: msg_id,
                            model: model_for_task.clone(),
                            role: Role::Assistant,
                            usage: TokenUsage::default(),
                        },
                    }))
                    .await;

                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockStart {
                        index: 0,
                        content_block: ContentBlock::Text { text: String::new() },
                    }))
                    .await;

                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::TextDelta {
                            text: trimmed.to_string(),
                        },
                    }))
                    .await;

                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockStop { index: 0 }))
                    .await;

                let _ = tx
                    .send(Ok(StreamEvent::MessageDelta {
                        delta: MessageDelta {
                            stop_reason: Some(StopReason::EndTurn),
                            stop_sequence: None,
                        },
                        usage: TokenUsage::default(),
                    }))
                    .await;
                let _ = tx.send(Ok(StreamEvent::MessageStop)).await;
            } else if saw_error {
                let _ = tx
                    .send(Ok(StreamEvent::MessageStart {
                        message: StreamMessage {
                            id: msg_id,
                            model: model_for_task.clone(),
                            role: Role::Assistant,
                            usage: TokenUsage::default(),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockStart {
                        index: 0,
                        content_block: ContentBlock::Text { text: String::new() },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::TextDelta {
                            text: format!("\n\n⚠️ Command Code CLI error: {}", error_text),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(StreamEvent::ContentBlockStop { index: 0 }))
                    .await;
                let _ = tx
                    .send(Err(ProviderError::RateLimitExceeded(error_text)))
                    .await;
            }

            let exit_status = child.wait().await;
            if let Ok(status) = exit_status
                && !status.success()
                && !saw_error
            {
                tracing::warn!("Command Code CLI exited with status: {}", status);
            }
        });

        let stream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "command-code-cli"
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn supported_models(&self) -> Vec<String> {
        SUPPORTED_MODELS.iter().map(|s| s.to_string()).collect()
    }

    fn configured_context_window(&self) -> Option<u32> {
        self.configured_context_window
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        // Command Code routes to upstream models (DeepSeek/GLM/MiMo/etc.) that
        // typically expose a 200k context window. Safe default for compaction.
        Some(200_000)
    }

    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        crate::usage::pricing::PricingConfig::load()
            .map(|cfg| cfg.calculate_cost(model, input_tokens, output_tokens))
            .unwrap_or(0.0)
    }

    fn supports_tools(&self) -> bool {
        // Command Code runs its own tool loop internally. OpenCrabs sees the
        // final text and must not re-execute any tool_use blocks.
        true
    }

    fn supports_vision(&self) -> bool {
        // `cmd -p` pipe mode has no inline image support — route through analyze_image.
        false
    }

    fn cli_handles_tools(&self) -> bool {
        true
    }

    fn cli_manages_context(&self) -> bool {
        // We feed Command Code the full conversation each invocation via stdin.
        // OpenCrabs owns context + compaction.
        false
    }
}
