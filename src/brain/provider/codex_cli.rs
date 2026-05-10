//! Codex CLI Provider — direct subprocess integration
//!
//! Spawns the `codex` CLI binary in non-interactive mode (`codex exec`)
//! and reads its JSONL stream output, converting it to standard
//! `StreamEvent`s. OpenCrabs handles all tools, memory, and context
//! locally; codex is used as the LLM backend so users can piggyback on
//! their existing ChatGPT/Codex auth (`~/.codex/auth.json`) without
//! needing a separate API key.

use super::error::{ProviderError, Result};
use super::r#trait::{Provider, ProviderStream};
use super::types::*;
use async_trait::async_trait;
use futures::stream::StreamExt;
use serde::Deserialize;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;

/// Codex CLI provider — talks directly to the `codex` binary.
#[derive(Clone)]
pub struct CodexCliProvider {
    codex_path: String,
    default_model: String,
}

impl CodexCliProvider {
    /// Create a new provider, auto-detecting the codex binary.
    pub fn new() -> Result<Self> {
        let path = resolve_codex_path()?;
        Ok(Self {
            codex_path: path,
            default_model: "gpt-5".to_string(),
        })
    }

    /// Override the default model (e.g. "gpt-5", "gpt-5-codex").
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
                        // codex exec supports `-i <FILE>` for images, but the
                        // prompt-builder path here can't add CLI args. Fall
                        // back to the analyze_image tool the same way Claude
                        // CLI does.
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
                                    "opencrabs_cli_img_{}.{}",
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

/// Resolve the codex CLI binary path.
fn resolve_codex_path() -> Result<String> {
    if let Ok(path) = std::env::var("CODEX_PATH") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
        return Err(ProviderError::Internal(format!(
            "CODEX_PATH set but not found: {}",
            path
        )));
    }

    let home = dirs::home_dir().unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("/opt/homebrew/bin/codex"),
        std::path::PathBuf::from("/usr/local/bin/codex"),
        home.join(".codex/bin/codex"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    if let Some(path) = super::which_binary("codex") {
        return Ok(path);
    }

    Err(ProviderError::Internal(
        "codex CLI not found — install `@openai/codex` or set CODEX_PATH".to_string(),
    ))
}

// ── CLI JSONL types ──

/// A parsed JSONL event from codex CLI stdout.
///
/// Codex emits these envelopes (observed against codex-cli 0.130.0):
///   `{"type":"thread.started","thread_id":"…"}`
///   `{"type":"turn.started"}`
///   `{"type":"item.started","item":{"id":"…","type":"command_execution","command":"…","status":"in_progress"}}`
///   `{"type":"item.completed","item":{"id":"…","type":"agent_message","text":"…"}}`
///   `{"type":"item.completed","item":{"id":"…","type":"command_execution","aggregated_output":"…","exit_code":0,"status":"completed"}}`
///   `{"type":"turn.completed","usage":{"input_tokens":…,"cached_input_tokens":…,"output_tokens":…,"reasoning_output_tokens":…}}`
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CliEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted {
        #[serde(default)]
        thread_id: Option<String>,
    },
    #[serde(rename = "turn.started")]
    TurnStarted {},
    #[serde(rename = "item.started")]
    ItemStarted { item: serde_json::Value },
    #[serde(rename = "item.completed")]
    ItemCompleted { item: serde_json::Value },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        #[serde(default)]
        usage: Option<CliUsage>,
    },
    #[serde(rename = "turn.failed")]
    TurnFailed {
        #[serde(default)]
        error: Option<serde_json::Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CliUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub cached_input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub reasoning_output_tokens: u32,
}

#[async_trait]
impl Provider for CodexCliProvider {
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
            "Spawning codex CLI: model={}, prompt_len={}, cwd={}",
            model,
            prompt.len(),
            cwd.display()
        );

        let mut child = tokio::process::Command::new(&self.codex_path)
            .arg("exec")
            .arg("--json")
            // Skip approvals + sandbox: we run codex non-interactively, so
            // any approval prompt would block forever. The workspace OpenCrabs
            // owns the trust boundary at the channel level (TUI / Telegram /
            // Slack), not at the codex level.
            .arg("--dangerously-bypass-approvals-and-sandbox")
            // Don't persist session files under ~/.codex/sessions/. OpenCrabs
            // owns conversation state — codex's stored sessions would just
            // accumulate stale forks.
            .arg("--ephemeral")
            // Allow running outside a git repo (TUI may launch from $HOME).
            .arg("--skip-git-repo-check")
            .arg("--cd")
            .arg(cwd.to_string_lossy().as_ref())
            .arg("--model")
            .arg(&model)
            // Read prompt from stdin; `-` tells codex to use stdin as the
            // initial instructions instead of as an appended `<stdin>` block.
            // Avoids leaking the prompt via `ps aux`.
            .arg("-")
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Internal(format!("failed to spawn codex CLI: {}", e)))?;

        // Pipe the prompt over stdin
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::Internal("failed to capture stdin".to_string()))?;
        let prompt_bytes = prompt.into_bytes();
        tokio::spawn(async move {
            if let Err(e) = stdin.write_all(&prompt_bytes).await {
                tracing::warn!("codex CLI stdin write failed: {}", e);
            }
            if let Err(e) = stdin.shutdown().await {
                tracing::debug!("codex CLI stdin shutdown: {}", e);
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

        // Surface stderr — codex prints onboarding hints, auth errors, and
        // version banners there.
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    tracing::warn!("codex CLI stderr: {}", line);
                }
            }
        });

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(64);
        let model_for_task = model.clone();

        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();

            let mut started = false;
            let mut block_index: usize = 0;
            // Track the codex item.id → our block index for command_execution
            // tools so item.completed can close the right block.
            let mut active_tool_blocks: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut thread_id: Option<String> = None;
            let mut final_usage = CliUsage::default();
            let mut turn_failed: Option<String> = None;

            loop {
                let line_result = tokio::select! {
                    biased;
                    _ = tx.closed() => {
                        tracing::info!("codex CLI stream cancelled — killing subprocess");
                        let _ = child.kill().await;
                        break;
                    }
                    result = lines.next_line() => result,
                };
                let line = match line_result {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("codex CLI stdout read error: {}", e);
                        break;
                    }
                };
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                tracing::debug!(
                    "codex CLI raw: {}",
                    &line[..line.floor_char_boundary(300)]
                );

                let event: CliEvent = match serde_json::from_str(&line) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping unparseable codex line: {} — {}",
                            e,
                            &line[..line.floor_char_boundary(200)]
                        );
                        continue;
                    }
                };

                match event {
                    CliEvent::ThreadStarted { thread_id: tid } => {
                        thread_id = tid;
                    }

                    CliEvent::TurnStarted {} => {
                        if !started {
                            started = true;
                            let msg_id = thread_id
                                .clone()
                                .map(|t| format!("msg_{}", t))
                                .unwrap_or_else(|| {
                                    format!("msg_{}", uuid::Uuid::new_v4().simple())
                                });
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
                        }
                    }

                    CliEvent::ItemStarted { item } => {
                        let item_type = item
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let item_id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Show shell tool calls so the TUI can render an
                        // expandable "Bash" group, just like for opencode.
                        if item_type == "command_execution" {
                            let command = item
                                .get("command")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let _ = tx
                                .send(Ok(StreamEvent::ContentBlockStart {
                                    index: block_index,
                                    content_block: ContentBlock::ToolUse {
                                        id: item_id.clone(),
                                        name: "bash".to_string(),
                                        input: serde_json::json!({ "command": command }),
                                    },
                                }))
                                .await;
                            active_tool_blocks.insert(item_id, block_index);
                            block_index += 1;
                            // Keep stream alive while the command runs.
                            if tx.send(Ok(StreamEvent::Ping)).await.is_err() {
                                break;
                            }
                        }
                    }

                    CliEvent::ItemCompleted { item } => {
                        let item_type = item
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let item_id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        match item_type.as_str() {
                            "agent_message" => {
                                let text = item
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if text.is_empty() {
                                    continue;
                                }
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockStart {
                                        index: block_index,
                                        content_block: ContentBlock::Text {
                                            text: String::new(),
                                        },
                                    }))
                                    .await;
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockDelta {
                                        index: block_index,
                                        delta: ContentDelta::TextDelta { text },
                                    }))
                                    .await;
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockStop {
                                        index: block_index,
                                    }))
                                    .await;
                                block_index += 1;
                            }
                            "reasoning" => {
                                let text = item
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                                    .unwrap_or("")
                                    .to_string();
                                if text.is_empty() {
                                    continue;
                                }
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockStart {
                                        index: block_index,
                                        content_block: ContentBlock::Thinking {
                                            thinking: String::new(),
                                            signature: None,
                                        },
                                    }))
                                    .await;
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockDelta {
                                        index: block_index,
                                        delta: ContentDelta::ThinkingDelta { thinking: text },
                                    }))
                                    .await;
                                let _ = tx
                                    .send(Ok(StreamEvent::ContentBlockStop {
                                        index: block_index,
                                    }))
                                    .await;
                                block_index += 1;
                            }
                            "command_execution" => {
                                // Close the tool block we opened in item.started.
                                if let Some(idx) = active_tool_blocks.remove(&item_id) {
                                    let _ = tx
                                        .send(Ok(StreamEvent::ContentBlockStop { index: idx }))
                                        .await;
                                }
                                // Keep the stream alive — codex may run more
                                // commands before producing final text.
                                if tx.send(Ok(StreamEvent::Ping)).await.is_err() {
                                    break;
                                }
                            }
                            other => {
                                tracing::debug!(
                                    "codex CLI item.completed of unknown type '{}' — skipping",
                                    other
                                );
                            }
                        }
                    }

                    CliEvent::TurnCompleted { usage } => {
                        if let Some(u) = usage {
                            final_usage = u;
                        }

                        // Close any tool blocks we never saw a completion for.
                        for (_, idx) in active_tool_blocks.drain() {
                            let _ = tx
                                .send(Ok(StreamEvent::ContentBlockStop { index: idx }))
                                .await;
                        }

                        if !started {
                            // Defensive: ensure helpers see at least a
                            // MessageStart even when codex emits no turn.started.
                            let _ = tx
                                .send(Ok(StreamEvent::MessageStart {
                                    message: StreamMessage {
                                        id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
                                        model: model_for_task.clone(),
                                        role: Role::Assistant,
                                        usage: TokenUsage::default(),
                                    },
                                }))
                                .await;
                        }

                        let _ = tx
                            .send(Ok(StreamEvent::MessageDelta {
                                delta: MessageDelta {
                                    stop_reason: Some(StopReason::EndTurn),
                                    stop_sequence: None,
                                },
                                usage: TokenUsage {
                                    input_tokens: final_usage.input_tokens,
                                    output_tokens: final_usage.output_tokens
                                        + final_usage.reasoning_output_tokens,
                                    // Codex reports `cached_input_tokens` —
                                    // map onto cache_read for billing parity
                                    // with other providers.
                                    cache_read_tokens: final_usage.cached_input_tokens,
                                    ..Default::default()
                                },
                            }))
                            .await;
                        let _ = tx.send(Ok(StreamEvent::MessageStop)).await;
                        break;
                    }

                    CliEvent::TurnFailed { error } => {
                        let msg = error
                            .as_ref()
                            .and_then(|e| e.get("message").and_then(|m| m.as_str()))
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                error
                                    .as_ref()
                                    .map(|e| e.to_string())
                                    .unwrap_or_else(|| "codex turn failed".to_string())
                            });
                        tracing::error!("codex CLI turn.failed: {}", msg);
                        turn_failed = Some(msg);
                    }

                    CliEvent::Unknown => {}
                }
            }

            // If turn.failed fired without a turn.completed, surface the
            // error so the fallback layer can swap providers.
            if let Some(msg) = turn_failed.as_ref() {
                let lower = msg.to_lowercase();
                if lower.contains("rate limit")
                    || lower.contains("quota")
                    || lower.contains("429")
                    || lower.contains("overloaded")
                    || lower.contains("capacity")
                {
                    let _ = tx
                        .send(Err(ProviderError::RateLimitExceeded(msg.clone())))
                        .await;
                } else if lower.contains("context length")
                    || lower.contains("too many tokens")
                    || lower.contains("prompt is too long")
                {
                    let _ = tx.send(Err(ProviderError::ContextLengthExceeded(0))).await;
                } else {
                    let _ = tx
                        .send(Err(ProviderError::ApiError {
                            status: 500,
                            message: msg.clone(),
                            error_type: Some("codex_turn_failed".to_string()),
                        }))
                        .await;
                }
            }

            let exit_status = child.wait().await;
            match &exit_status {
                Ok(status) if !status.success() => {
                    tracing::warn!("codex CLI exited with status: {}", status);
                    if !started && turn_failed.is_none() {
                        let _ = tx
                            .send(Err(ProviderError::Internal(format!(
                                "codex CLI exited with {} before producing any output",
                                status
                            ))))
                            .await;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to wait on codex CLI: {}", e);
                }
                Ok(_) => {
                    if !started && turn_failed.is_none() {
                        tracing::warn!(
                            "codex CLI exited successfully but produced no stream events"
                        );
                    }
                }
            }
        });

        let stream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "codex-cli"
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn supported_models(&self) -> Vec<String> {
        // Codex resolves whatever the user has access to via their
        // ChatGPT/Codex auth — we list a few common ids so the picker
        // shows something reasonable. The CLI itself validates the model.
        vec![
            "gpt-5".to_string(),
            "gpt-5-codex".to_string(),
            "gpt-5-mini".to_string(),
            "gpt-5-nano".to_string(),
            "o3".to_string(),
            "o3-mini".to_string(),
        ]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        // GPT-5 family ships with a 400k context window per OpenAI's docs;
        // older o-series models cap at 200k. Use the smaller value as a
        // safe default — pricing/compaction logic will respect this.
        Some(200_000)
    }

    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        crate::usage::pricing::PricingConfig::load()
            .map(|cfg| cfg.calculate_cost(model, input_tokens, output_tokens))
            .unwrap_or(0.0)
    }

    fn supports_tools(&self) -> bool {
        // Codex runs its own tool loop (shell, file edits, web). OpenCrabs
        // tool_loop sees the calls via cli_handles_tools() and renders them
        // for display without re-executing.
        true
    }

    fn supports_vision(&self) -> bool {
        // codex exec accepts `-i <FILE>` but the CLI is invoked here without
        // image attachments — we route vision through analyze_image instead.
        false
    }

    fn cli_handles_tools(&self) -> bool {
        true
    }

    fn cli_manages_context(&self) -> bool {
        // We feed codex the full conversation each invocation via stdin
        // (--ephemeral, no --resume). OpenCrabs owns context + compaction.
        false
    }
}
