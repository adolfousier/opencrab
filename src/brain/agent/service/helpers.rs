use super::builder::AgentService;
use super::types::{MessageQueueCallback, ProgressCallback, ProgressEvent};
use crate::brain::provider::{
    ContentBlock, ImageSource, LLMRequest, LLMResponse, Message, Role, StopReason,
};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Pick the stream-handshake timeout based on what kind of provider
/// we're calling. Pulled out as a free `pub(crate)` function so the
/// matrix is unit-testable without spinning up a real async stream.
///
/// Branches:
///   - CLI providers (claude-cli, opencode-cli, etc.): **10 min**.
///     Subprocess startup + auth refresh can be slow.
///   - Local HTTP (llama.cpp / MLX / Ollama / LM Studio): **90s**.
///     30s killed real Unsloth cold starts (2026-04-17 20:18: 35B
///     gguf loading KV cache succeeded at +38s); 90s still fails an
///     unrecoverable wedge in under 2 min via the retry chain.
///   - Cloud HTTP (OpenAI, Anthropic, Zhipu, opencode.ai, NVIDIA NIM,
///     etc.): **60s**. Healthy cloud gateways return headers in <5s,
///     but routing proxies (dialagram, openrouter) can take 20-45s
///     when upstream is slow. Wedged servers are >120s, so 60s still
///     catches real hangs in under 3 min via the retry chain. Previous
///     30s killed legitimate requests to slower-but-healthy providers.
pub(crate) fn handshake_timeout_for(cli_handles_tools: bool, base_url: Option<&str>) -> Duration {
    if cli_handles_tools {
        Duration::from_secs(600)
    } else if base_url.is_some_and(crate::brain::provider::factory::is_local_base_url) {
        Duration::from_secs(90)
    } else {
        Duration::from_secs(60)
    }
}

impl AgentService {
    /// Actual token count for the serialized tool schemas (cached per call).
    pub(super) fn actual_tool_schema_tokens(&self) -> usize {
        crate::brain::tokenizer::count_tokens(
            &serde_json::to_string(&self.tool_registry.get_tool_definitions()).unwrap_or_default(),
        )
    }

    /// Stream a request and accumulate into an LLMResponse.
    ///
    /// Sends text deltas to the progress callback as `StreamingChunk` events
    /// so the TUI can display them in real-time. Returns the full response
    /// once the stream completes, ready for tool extraction.
    ///
    /// `override_cb` takes precedence over the service-level `self.progress_callback`
    /// so per-call callbacks (e.g. Telegram) receive real-time streaming chunks.
    ///
    /// `queue_cb` + `queued_out`: CLI providers only. When a queued user message
    /// is consumed mid-stream at a tool boundary, it is written to `queued_out`
    /// so the caller can inject it into context after the stream ends.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn stream_complete(
        &self,
        session_id: Uuid,
        request: LLMRequest,
        cancel_token: Option<&CancellationToken>,
        override_cb: Option<&ProgressCallback>,
        queue_cb: Option<&MessageQueueCallback>,
        queued_out: Option<&tokio::sync::Mutex<Option<String>>>,
        suppress_callback: bool,
    ) -> std::result::Result<(LLMResponse, Option<String>), crate::brain::provider::ProviderError>
    {
        use crate::brain::provider::{ContentDelta, StreamEvent, TokenUsage};
        use futures::StreamExt;

        // suppress_callback=true skips all progress events (used during compaction
        // to prevent the compaction LLM response from leaking as visible TUI text).
        let effective_cb: Option<&ProgressCallback> = if suppress_callback {
            None
        } else {
            override_cb.or(self.progress_callback.as_ref())
        };

        let provider = self.provider_for_session(session_id);

        // Invariant: never send a {provider, model} pair the user didn't
        // configure. If the request's model isn't in this provider's
        // supported list, remap to the provider's own default. This
        // catches every path that might end up with a mismatched pair —
        // cancelled fallback that bypassed its Drop guard, stale
        // session_providers entry, upstream bug — so the HTTP call
        // always goes out with a valid pair. 2026-04-18 18:14:
        // zhipu was sent `model=qwen3.6-plus` (opencodeiolo's model)
        // because the session's provider got stuck on a fallback
        // after a cancelled turn. That exact case is now prevented
        // at the stream site regardless of the cached state.
        let mut request = request;
        let supported = provider.supported_models();
        if !supported.is_empty() && !supported.iter().any(|m| m == &request.model) {
            let remapped = provider.default_model().to_string();
            tracing::warn!(
                "stream_complete: provider '{}' does not support model '{}' — remapping to '{}' (never send a pair the user never configured)",
                provider.name(),
                request.model,
                remapped,
            );
            request.model = remapped;
        }
        let request_model = request.model.clone();

        // Bound the initial stream handshake (HTTP POST + response headers)
        // so a wedged server — accepts TCP but never replies — can't eat
        // the full 300s reqwest timeout before the retry chain fires.
        let handshake_timeout =
            handshake_timeout_for(provider.cli_handles_tools(), provider.base_url());
        let mut stream =
            match tokio::time::timeout(handshake_timeout, provider.stream(request)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    crate::config::health::record_failure(provider.name(), &e.to_string());
                    return Err(e);
                }
                Err(_elapsed) => {
                    let secs = handshake_timeout.as_secs();
                    tracing::warn!(
                        "⏱️ stream handshake timeout after {}s ({}); retry chain will fire",
                        secs,
                        provider.base_url().unwrap_or("<no-base-url>"),
                    );
                    crate::config::health::record_failure(
                        provider.name(),
                        &format!("handshake timeout after {}s", secs),
                    );
                    return Err(crate::brain::provider::ProviderError::Timeout(secs));
                }
            };

        // Accumulate state from stream events
        let mut id = String::new();
        let mut model = String::new();
        let mut stop_reason: Option<StopReason> = None;
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut cache_creation_tokens = 0u32;
        let mut cache_read_tokens = 0u32;
        let mut billing_cache_creation = 0u32;
        let mut billing_cache_read = 0u32;

        // --- Text repetition detection ---
        // Some providers (e.g. MiniMax) loop the same content indefinitely without
        // sending a stop signal. We keep a sliding window of recent text chunks and
        // detect when a long enough substring repeats, indicating a stuck loop.
        let mut total_text_len: usize = 0;
        let mut text_window = String::new(); // rolling window of recent text
        const REPEAT_WINDOW: usize = 2048; // bytes to keep in window
        const REPEAT_MIN_MATCH: usize = 200; // minimum repeated substring to trigger

        // Track partial content blocks by index
        // Text blocks: accumulate text deltas
        // ToolUse blocks: accumulate JSON deltas
        struct BlockState {
            block: ContentBlock,
            json_buf: String, // for tool use JSON accumulation
        }
        let mut block_states: Vec<BlockState> = Vec::new();
        let mut reasoning_buf = String::new();
        let is_cli = provider.cli_handles_tools();
        // CLI: track unflushed text so we can emit IntermediateText at tool
        // boundaries, giving the TUI real-time text→tools→text interleaving
        // during streaming instead of one massive wall after stream ends.
        let mut cli_unflushed_text = String::new();

        // Maximum idle time between SSE events before treating as a dropped
        // connection. NVIDIA/Kimi and some other providers occasionally hang
        // silently without sending [DONE] — this timeout lets the retry logic
        // in tool_loop.rs recover instead of blocking the TUI forever.
        //
        // Three-way split:
        //
        //   CLI: 1h. Subprocess runs tools internally (cargo build, gh, etc.).
        //
        //   Local HTTP server (llama.cpp / Unsloth / LM Studio / Ollama / MLX):
        //   1h. Hardware + model size varies too much to pick a smaller
        //   number confidently — 4B on an M3 Ultra emits in seconds, but a
        //   70B gguf on an older Mac or a LAN box running MoE 100B+ can
        //   spend tens of minutes on prefill for a full-context prompt. The
        //   handshake timeout (90s) above still catches genuinely dead
        //   servers fast; once headers arrive the server is clearly alive
        //   and just working — don't cut it off on a guess. The user can
        //   always Esc if a turn truly runs away.
        //
        //   Remote HTTP: 90s. Cross-continent latency plus LLM warmup.
        //   OpenAI/OpenRouter/etc. emit stream events faster than a local
        //   prefill, so 90s of silence really does mean the connection
        //   dropped.
        let is_local = provider
            .base_url()
            .map(crate::brain::provider::factory::is_local_base_url)
            .unwrap_or(false);
        let stream_idle_timeout = if is_cli || is_local {
            std::time::Duration::from_secs(3600)
        } else {
            std::time::Duration::from_secs(90)
        };

        loop {
            // Race stream.next() against cancellation token and idle timeout.
            // This ensures /stop takes effect immediately even mid-chunk.
            let next = tokio::select! {
                biased;
                _ = async {
                    if let Some(token) = cancel_token {
                        token.cancelled().await;
                    } else {
                        // No cancel token — never resolves
                        std::future::pending::<()>().await;
                    }
                } => {
                    tracing::info!("Stream cancelled by user");
                    break;
                }
                result = tokio::time::timeout(stream_idle_timeout, stream.next()) => {
                    match result {
                        Ok(Some(item)) => item,
                        Ok(None) => break, // Stream ended normally
                        Err(_elapsed) => {
                            tracing::warn!(
                                "⏱️ Stream idle timeout after {}s — no event received from provider. \
                                 Treating as dropped stream (stop_reason=None → will retry).",
                                stream_idle_timeout.as_secs()
                            );
                            break; // stop_reason stays None → triggers retry in tool_loop
                        }
                    }
                }
            };

            let event = match next {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Stream error: {}", e);
                    return Err(e);
                }
            };

            match event {
                StreamEvent::MessageStart { message } => {
                    id = message.id;
                    model = message.model;
                    input_tokens = message.usage.input_tokens;
                }
                StreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    // Ensure block_states has enough capacity
                    while block_states.len() <= index {
                        block_states.push(BlockState {
                            block: ContentBlock::Text {
                                text: String::new(),
                            },
                            json_buf: String::new(),
                        });
                    }
                    // Separate thinking blocks from different rounds with a blank line
                    if matches!(content_block, ContentBlock::Thinking { .. })
                        && !reasoning_buf.is_empty()
                    {
                        reasoning_buf.push_str("\n\n");
                        // Also emit separator to TUI so streaming display stays in sync
                        if let Some(cb) = effective_cb {
                            cb(
                                session_id,
                                ProgressEvent::ReasoningChunk {
                                    text: "\n\n".to_string(),
                                },
                            );
                        }
                    }
                    block_states[index] = BlockState {
                        block: content_block,
                        json_buf: String::new(),
                    };
                }
                StreamEvent::ContentBlockDelta { index, delta } => {
                    if index < block_states.len() {
                        match delta {
                            ContentDelta::TextDelta { text } => {
                                // Forward to TUI / per-call callback for real-time display
                                if let Some(cb) = effective_cb {
                                    cb(
                                        session_id,
                                        ProgressEvent::StreamingChunk { text: text.clone() },
                                    );
                                }
                                // CLI: track unflushed text for tool-boundary flushing
                                if is_cli {
                                    cli_unflushed_text.push_str(&text);
                                }
                                // Accumulate into block
                                if let ContentBlock::Text { text: ref mut t } =
                                    block_states[index].block
                                {
                                    t.push_str(&text);
                                }

                                // --- Repetition & size detection ---
                                total_text_len += text.len();
                                text_window.push_str(&text);
                                if text_window.len() > REPEAT_WINDOW {
                                    let mut drain = text_window.len() - REPEAT_WINDOW;
                                    // Advance to a valid char boundary
                                    while !text_window.is_char_boundary(drain)
                                        && drain < text_window.len()
                                    {
                                        drain += 1;
                                    }
                                    text_window.drain(..drain);
                                }

                                // Check for repeated substring in window
                                if detect_text_repetition(&text_window, REPEAT_MIN_MATCH) {
                                    tracing::warn!(
                                        "🔁 Repetition detected in streaming response after {} bytes. \
                                         Provider appears to be looping. Terminating stream.",
                                        total_text_len,
                                    );
                                    stop_reason = Some(StopReason::EndTurn);
                                    break;
                                }
                            }
                            ContentDelta::InputJsonDelta { partial_json } => {
                                block_states[index].json_buf.push_str(&partial_json);
                            }
                            ContentDelta::ReasoningDelta { text } => {
                                if let Some(cb) = effective_cb {
                                    cb(
                                        session_id,
                                        ProgressEvent::ReasoningChunk { text: text.clone() },
                                    );
                                }
                                // Always accumulate for DB persistence
                                reasoning_buf.push_str(&text);
                            }
                            ContentDelta::ThinkingDelta { thinking } => {
                                // Anthropic native thinking_delta — same as reasoning
                                if let Some(cb) = effective_cb {
                                    cb(
                                        session_id,
                                        ProgressEvent::ReasoningChunk {
                                            text: thinking.clone(),
                                        },
                                    );
                                }
                                reasoning_buf.push_str(&thinking);
                            }
                        }
                    }
                }
                StreamEvent::ContentBlockStop { index } => {
                    if index < block_states.len() {
                        // Finalize tool use blocks: parse accumulated JSON with
                        // partial-repair fallback so a truncated stream surfaces
                        // partial intent instead of {}.
                        {
                            let state = &mut block_states[index];
                            if let ContentBlock::ToolUse { ref mut input, .. } = state.block
                                && !state.json_buf.is_empty()
                            {
                                *input = crate::brain::provider::json_repair::parse_or_repair(
                                    &state.json_buf,
                                );
                            }
                        }
                        // CLI: flush accumulated text as IntermediateText before
                        // emitting tool events, so TUI shows text→tools sequentially
                        // during streaming instead of one wall after stream ends.
                        // Also clear the text from prior text blocks so the final
                        // response.content only contains text emitted AFTER the
                        // last flush (preventing complete_response from
                        // overwriting the last intermediate msg with duplicate text).
                        let is_tool =
                            matches!(block_states[index].block, ContentBlock::ToolUse { .. });
                        if is_cli
                            && is_tool
                            && !cli_unflushed_text.is_empty()
                            && let Some(cb) = effective_cb
                        {
                            cb(
                                session_id,
                                ProgressEvent::IntermediateText {
                                    text: cli_unflushed_text.clone(),
                                    // None lets the TUI pull from its
                                    // accumulated streaming_reasoning
                                    reasoning: None,
                                },
                            );
                            cli_unflushed_text.clear();
                            for bs in block_states.iter_mut() {
                                if let ContentBlock::Text { text: ref mut t } = bs.block {
                                    t.clear();
                                }
                            }
                        }
                        // Emit ToolStarted + ToolCompleted with fully parsed input
                        // so the TUI shows real tool context (command, file path, etc.)
                        //
                        // CLI-ONLY: for CLI providers (claude-cli, qwen-cli, opencode),
                        // the CLI runs tools itself and the stream-close is the ONLY
                        // signal we get — so we synthesize the lifecycle here.
                        //
                        // For non-CLI providers (OpenAI-compatible, Anthropic, etc.)
                        // tool_loop owns the full lifecycle: it fires ToolStarted
                        // before invoking the tool and ToolCompleted with the real
                        // output. Firing here too would DOUBLE every event, bloat
                        // the tool-call count in the TUI, and leave a phantom
                        // "Processing: <tool>" indicator because the premature
                        // fake completion races the real one. See the 6-in-85µs
                        // duplication observed in logs.
                        if is_cli {
                            let state = &mut block_states[index];
                            if let ContentBlock::ToolUse {
                                ref name,
                                ref input,
                                ..
                            } = state.block
                                && let Some(cb) = effective_cb
                            {
                                let emit_name = name.to_lowercase();
                                cb(
                                    session_id,
                                    ProgressEvent::ToolStarted {
                                        tool_name: emit_name.clone(),
                                        tool_input: input.clone(),
                                    },
                                );
                                cb(
                                    session_id,
                                    ProgressEvent::ToolCompleted {
                                        tool_name: emit_name,
                                        tool_input: input.clone(),
                                        success: true,
                                        summary: String::new(),
                                    },
                                );

                                // CLI only: check if user queued a message during
                                // tool execution. Consume it and break the stream
                                // so tool_loop can inject it into context.
                                if let Some(qcb) = queue_cb
                                    && let Some(queued) = qcb(session_id).await
                                {
                                    tracing::info!(
                                        "Queued user message at CLI tool boundary — storing for tool_loop"
                                    );
                                    // Only store — don't emit QueuedUserMessage here.
                                    // tool_loop emits it AFTER CLI interleaving so it
                                    // appears in the correct position (after all tools).
                                    if let Some(buf) = queued_out {
                                        *buf.lock().await = Some(queued);
                                    }
                                    stop_reason = Some(StopReason::EndTurn);
                                    break;
                                }
                            }
                        }
                    }
                }
                StreamEvent::MessageDelta { delta, usage } => {
                    // Only update stop_reason if the delta carries one — deferred
                    // usage chunks send a second MessageDelta with stop_reason=None
                    // that must not overwrite the real stop_reason.
                    if delta.stop_reason.is_some() {
                        stop_reason = delta.stop_reason;
                    }
                    // Take the largest values — MiniMax sends two deltas:
                    // first (0,0), then the real usage. Other providers
                    // may only send one. Using max() handles both cases.
                    if usage.input_tokens > input_tokens {
                        input_tokens = usage.input_tokens;
                    }
                    if usage.output_tokens > output_tokens {
                        output_tokens = usage.output_tokens;
                    }
                    // Per-call cache tokens (context window proxy)
                    if usage.cache_creation_tokens > cache_creation_tokens {
                        cache_creation_tokens = usage.cache_creation_tokens;
                    }
                    if usage.cache_read_tokens > cache_read_tokens {
                        cache_read_tokens = usage.cache_read_tokens;
                    }
                    // Billing cache tokens (cumulative across CLI rounds)
                    if usage.billing_cache_creation > billing_cache_creation {
                        billing_cache_creation = usage.billing_cache_creation;
                    }
                    if usage.billing_cache_read > billing_cache_read {
                        billing_cache_read = usage.billing_cache_read;
                    }
                }
                StreamEvent::MessageStop => break,
                StreamEvent::Ping => {
                    // CLI providers (opencode, claude-cli, qwen-cli) emit Ping
                    // at step_finish / tool_result boundaries. Use them as flush
                    // points so the TUI shows each step's tool calls + thinking
                    // live, instead of batching everything into one giant group
                    // at end-of-stream. Skip if there's nothing pending — empty
                    // flushes inflate the message count and waste vertical space.
                    if is_cli
                        && !cli_unflushed_text.is_empty()
                        && let Some(cb) = effective_cb
                    {
                        cb(
                            session_id,
                            ProgressEvent::IntermediateText {
                                text: std::mem::take(&mut cli_unflushed_text),
                                reasoning: None,
                            },
                        );
                        // Clear text from prior text blocks so the final
                        // response.content only contains text emitted AFTER
                        // this flush — prevents complete_response from
                        // overwriting the last intermediate msg with duplicate text.
                        for bs in block_states.iter_mut() {
                            if let ContentBlock::Text { text: ref mut t } = bs.block {
                                t.clear();
                            }
                        }
                    }
                }
                StreamEvent::Error { error } => {
                    crate::config::health::record_failure(provider.name(), &error);
                    return Err(crate::brain::provider::ProviderError::StreamError(error));
                }
            }
        }

        // CLI: flush any trailing text after the last tool
        if is_cli
            && !cli_unflushed_text.is_empty()
            && let Some(cb) = effective_cb
        {
            cb(
                session_id,
                ProgressEvent::IntermediateText {
                    text: cli_unflushed_text,
                    reasoning: None,
                },
            );
        }

        // Detect premature stream termination — if we accumulated blocks but never
        // got a stop_reason, the connection likely dropped before [DONE]/MessageStop.
        // Return a StreamError so the tool_loop's retry logic can re-issue the request
        // instead of silently returning partial/empty content to the user.
        if stop_reason.is_none() && !block_states.is_empty() {
            let msg = format!(
                "Stream ended without [DONE]: {} content blocks, {} output tokens — connection likely dropped",
                block_states.len(),
                output_tokens,
            );
            tracing::warn!("⚠️ {}", msg);
            return Err(crate::brain::provider::ProviderError::StreamError(msg));
        }

        // Self-heal: detect truncated responses disguised as complete.
        // Some providers (notably Qwen) occasionally send finish_reason="stop"
        // with a usage chunk after only a handful of tokens, producing a response
        // like "Let me check the current state:" — clearly mid-thought.  The
        // premature-termination guard above can't catch this because stop_reason
        // IS set (from the finish_reason chunk).  Detect it by checking: very low
        // output tokens + text that looks like an incomplete preamble (ends with
        // `:` or `...`).  Returning StreamError lets retry/rotation/fallback
        // re-issue the request instead of accepting garbage.
        if stop_reason == Some(StopReason::EndTurn) && output_tokens > 0 && output_tokens < 100 {
            let has_tool_use = block_states
                .iter()
                .any(|bs| matches!(&bs.block, ContentBlock::ToolUse { .. }));
            if !has_tool_use {
                let text: String = block_states
                    .iter()
                    .filter_map(|bs| match &bs.block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let trimmed = text.trim();
                if trimmed.ends_with(':') || trimmed.ends_with("...") {
                    // Heuristic to reduce false positives: if the text contains
                    // multiple sentences (period, exclamation) it's more likely
                    // a legitimate short instruction than a truncated preamble.
                    // Real truncations are single preamble sentences like
                    // "Let me check the current state:" — no prior punctuation.
                    let has_prior_sentence = trimmed[..trimmed.len().saturating_sub(1)]
                        .contains('.')
                        || trimmed[..trimmed.len().saturating_sub(1)].contains('!');
                    if has_prior_sentence {
                        tracing::debug!(
                            "Self-heal: skipping truncation check — text contains \
                             prior sentences (likely deliberate short response)"
                        );
                    } else {
                        let preview = if trimmed.len() > 80 {
                            &trimmed[trimmed.len() - 80..]
                        } else {
                            trimmed
                        };
                        let msg = format!(
                            "Self-heal: provider sent stop after only {} output tokens — \
                             response appears truncated: \"{}\"",
                            output_tokens, preview,
                        );
                        tracing::warn!("⚠️ {}", msg);
                        if let Some(cb) = effective_cb {
                            cb(
                                session_id,
                                ProgressEvent::SelfHealingAlert {
                                    message: msg.clone(),
                                },
                            );
                        }
                        return Err(crate::brain::provider::ProviderError::StreamError(msg));
                    }
                }
            }
        }

        // Build final content blocks from accumulated state
        // Filter out empty text blocks — Anthropic rejects "text content blocks must be non-empty"
        let content_blocks: Vec<ContentBlock> = block_states
            .into_iter()
            .map(|s| s.block)
            .filter(|b| !matches!(b, ContentBlock::Text { text } if text.is_empty()))
            .collect();

        // Track provider health + snapshot config on first success.
        crate::config::health::record_success(provider.name());
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static SAVED: AtomicBool = AtomicBool::new(false);
            if !SAVED.swap(true, Ordering::Relaxed) {
                crate::config::save_last_good_config();
            }
        }

        let reasoning = if reasoning_buf.is_empty() {
            None
        } else {
            Some(reasoning_buf)
        };
        Ok((
            LLMResponse {
                id,
                // Some providers (e.g. MiniMax) don't include the model name in stream chunks.
                // Fall back to the request model so pricing lookup never gets an empty string.
                model: if model.is_empty() {
                    request_model
                } else {
                    model
                },
                content: content_blocks,
                stop_reason,
                usage: TokenUsage {
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                    billing_cache_creation,
                    billing_cache_read,
                },
            },
            reasoning,
        ))
    }

    /// Build a user Message, auto-attaching images from `<<IMG:path>>` markers.
    /// The TUI inserts these markers for detected image paths/URLs (handles spaces).
    pub(super) async fn build_user_message(text: &str) -> Message {
        let mut image_blocks: Vec<ContentBlock> = Vec::new();

        // Extract <<IMG:path>> markers
        let mut clean_text = text.to_string();
        while let Some(start) = clean_text.find("<<IMG:") {
            if let Some(end) = clean_text[start..].find(">>") {
                let marker_end = start + end + 2;
                let img_path = &clean_text[start + 6..start + end];

                // URL image
                if img_path.starts_with("http://") || img_path.starts_with("https://") {
                    image_blocks.push(ContentBlock::Image {
                        source: ImageSource::Url {
                            url: img_path.to_string(),
                        },
                    });
                    tracing::info!("Auto-attached image URL: {}", img_path);
                }
                // Local file
                else {
                    let path = std::path::Path::new(img_path);
                    if let Ok(data) = tokio::fs::read(path).await {
                        let lower = img_path.to_lowercase();
                        let media_type = match lower.rsplit('.').next().unwrap_or("") {
                            "png" => "image/png",
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            "webp" => "image/webp",
                            "bmp" => "image/bmp",
                            "svg" => "image/svg+xml",
                            _ => "application/octet-stream",
                        };
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        image_blocks.push(ContentBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: media_type.to_string(),
                                data: b64,
                            },
                        });
                        tracing::info!(
                            "Auto-attached image: {} ({}, {} bytes)",
                            img_path,
                            media_type,
                            data.len()
                        );
                    } else {
                        tracing::warn!("Could not read image file: {}", img_path);
                    }
                }

                // Replace marker with a path hint so any model (vision or text-only) can
                // route the image through analyze_image / other vision tools. Vision-capable
                // providers also get the base64 image block below; text-only models rely on
                // this path to call analyze_image directly.
                let hint = format!("[image attached: {}]", img_path);
                clean_text = format!(
                    "{}{}{}",
                    &clean_text[..start],
                    hint,
                    &clean_text[marker_end..]
                );
            } else {
                break; // Malformed marker
            }
        }

        let clean_text = clean_text.trim().to_string();

        if image_blocks.is_empty() {
            Message::user(clean_text)
        } else {
            // Text first, then images
            let mut blocks = vec![ContentBlock::Text { text: clean_text }];
            blocks.extend(image_blocks);
            Message {
                role: Role::User,
                content: blocks,
            }
        }
    }

    /// Compact tool description for DB persistence (mirrors TUI's format_tool_description).
    /// Paths / commands collapse `$HOME` → `~` so channel displays don't expose
    /// the user's full home path and don't waste the truncation budget on
    /// a constant prefix.
    pub(super) fn format_tool_summary(tool_name: &str, tool_input: &Value) -> String {
        use crate::utils::string::tilde_home;
        match tool_name {
            "bash" => {
                let cmd = tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("bash: {}", tilde_home(cmd))
            }
            "read_file" | "read" => {
                let path = tool_input
                    .get("path")
                    .or_else(|| tool_input.get("file_path"))
                    .or_else(|| tool_input.get("filePath"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Read {}", tilde_home(path))
            }
            "write_file" | "write" => {
                let path = tool_input
                    .get("path")
                    .or_else(|| tool_input.get("file_path"))
                    .or_else(|| tool_input.get("filePath"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Write {}", tilde_home(path))
            }
            "edit_file" | "edit" => {
                let path = tool_input
                    .get("path")
                    .or_else(|| tool_input.get("file_path"))
                    .or_else(|| tool_input.get("filePath"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Edit {}", tilde_home(path))
            }
            "ls" => {
                let path = tool_input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                format!("ls {}", tilde_home(path))
            }
            "glob" => {
                let p = tool_input
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Glob {}", p)
            }
            "grep" => {
                let p = tool_input
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let path = tool_input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if path.is_empty() {
                    format!("Grep '{}'", p)
                } else {
                    format!("Grep '{}' in {}", p, tilde_home(path))
                }
            }
            "web_search" | "exa_search" | "brave_search" => {
                let q = tool_input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Search: {}", q)
            }
            "plan" => {
                let op = tool_input
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Plan: {}", op)
            }
            "task_manager" => {
                let op = tool_input
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Task: {}", op)
            }
            "memory_search" => {
                let q = tool_input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Memory: {}", q)
            }
            other => other.to_string(),
        }
    }

    /// Normalize hallucinated tool names from providers.
    ///
    /// Some models (e.g. MiniMax) send tool names like `"Plan: complete_task"`
    /// instead of `tool="plan"` with `operation="complete_task"` in the input.
    /// This recovers the intended call so it doesn't fail with "Tool not found".
    pub(crate) fn normalize_tool_call(
        name: String,
        mut input: serde_json::Value,
    ) -> (String, serde_json::Value) {
        // "Plan: <op>" or "plan: <op>" → tool="plan", inject operation into input
        if let Some(op) = name
            .strip_prefix("Plan: ")
            .or_else(|| name.strip_prefix("plan: "))
            .or_else(|| name.strip_prefix("Plan:"))
            .or_else(|| name.strip_prefix("plan:"))
        {
            let op = op.trim().replace(' ', "_");
            if !op.is_empty() {
                if let Some(obj) = input.as_object_mut() {
                    obj.entry("operation")
                        .or_insert_with(|| serde_json::Value::String(op));
                }
                tracing::info!(
                    "[TOOL_NORM] Normalized '{}' → tool='plan', input={:?}",
                    name,
                    input
                );
                return ("plan".to_string(), input);
            }
        }

        // Generic fallback: if name contains ": " and isn't a registered tool,
        // try the part before ": " as the tool name (lowercased)
        if name.contains(": ") {
            let parts: Vec<&str> = name.splitn(2, ": ").collect();
            if parts.len() == 2 {
                let candidate = parts[0].to_lowercase().replace(' ', "_");
                let suffix = parts[1].trim().replace(' ', "_");
                if !suffix.is_empty() {
                    if let Some(obj) = input.as_object_mut() {
                        obj.entry("operation")
                            .or_insert_with(|| serde_json::Value::String(suffix));
                    }
                    tracing::info!(
                        "[TOOL_NORM] Normalized '{}' → tool='{}', input={:?}",
                        name,
                        candidate,
                        input
                    );
                    return (candidate, input);
                }
            }
        }

        // Claude Code tool name mapping (capitalized → OpenCrabs lowercase)
        // The cc-max-proxy returns Claude Code tool names which differ from ours.
        let mapped = match name.as_str() {
            "Bash" => Some("bash"),
            "Read" => Some("read_file"),
            "Write" => Some("write_file"),
            "Edit" => Some("edit_file"),
            "Glob" => Some("glob"),
            "Grep" => Some("grep"),
            "WebSearch" => Some("web_search"),
            "WebFetch" => Some("http_request"),
            "NotebookEdit" => Some("notebook_edit"),
            _ => None,
        };
        if let Some(canonical) = mapped {
            tracing::info!(
                "[TOOL_NORM] Mapped Claude Code tool '{}' → '{}'",
                name,
                canonical
            );
            return (canonical.to_string(), input);
        }

        // Final fallback: lowercase the name (catches simple case mismatches)
        let lowered = name.to_lowercase();
        if lowered != name {
            tracing::info!("[TOOL_NORM] Lowercased tool '{}' → '{}'", name, lowered);
            return (lowered, input);
        }

        (name, input)
    }

    /// Strip XML tool-call blocks from text so raw XML
    /// doesn't get persisted to DB or shown to the user.
    /// Catches `<tool_call>`, `<tool_code>`, `<StartToolCall>`, `<minimax:tool_call>`,
    /// `<tool_use>`, `<result>`, and any `<parameter>` blocks providers hallucinate.
    /// Check if text contains actual XML tool-call blocks (not just mentions).
    /// Requires BOTH opening AND closing tags to exist so that prose mentions
    /// like `` `<tool_use>` `` don't trigger false positives.
    pub(crate) fn has_xml_tool_block(text: &str) -> bool {
        (text.contains("<tool_call>") && text.contains("</tool_call>"))
            || (text.contains("<tool_code>") && text.contains("</tool_code>"))
            || (text.contains("<StartToolCall>") && text.contains("</StartToolCall>"))
            || (text.contains("<minimax:tool_call>") && text.contains("</minimax:tool_call>"))
            || (text.contains("<invoke") && text.contains("</invoke>"))
            || (text.contains("<tool_use>") && text.contains("</tool_use>"))
    }

    /// Parse XML tool-call blocks into (name, input) pairs.
    /// Handles multiple formats MiniMax uses:
    ///   <tool_call>{"tool_name":"bash","args":{"command":"..."}}</tool_call>
    ///   <tool_call>{"name":"bash","arguments":{"command":"..."}}</tool_call>
    ///   <tool_use>{"name":"bash","input":{"command":"..."}}</tool_use>
    pub(crate) fn parse_xml_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
        use regex::Regex;
        use std::sync::LazyLock;

        static XML_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?s)<(?:tool_call|tool_code|tool_use|minimax:tool_call|StartToolCall)>(.*?)</(?:tool_call|tool_code|tool_use|minimax:tool_call|StartToolCall)>"#).unwrap()
        });

        let mut results = Vec::new();
        for cap in XML_BLOCK_RE.captures_iter(text) {
            let inner = cap[1].trim();
            // Try parsing as JSON
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(inner) {
                // Extract tool name from various field names
                let name = obj
                    .get("tool_name")
                    .or_else(|| obj.get("name"))
                    .or_else(|| obj.get("function"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if name.is_empty() {
                    continue;
                }

                // Extract input/arguments from various field names
                let input = obj
                    .get("args")
                    .or_else(|| obj.get("arguments"))
                    .or_else(|| obj.get("input"))
                    .or_else(|| obj.get("parameters"))
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                tracing::info!(
                    "[XML_TOOL_PARSE] Recovered tool call: name={}, input_keys={:?}",
                    name,
                    input.as_object().map(|o| o.keys().collect::<Vec<_>>())
                );
                results.push((name, input));
            }
        }
        results
    }

    pub(crate) fn strip_xml_tool_calls(text: &str) -> String {
        use regex::Regex;
        use std::sync::LazyLock;

        // Match only properly closed XML tool-call blocks.
        // NO |$ fallback — unclosed tags (prose mentions) must NOT match.
        static TOOL_CALL_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?s)(<tool_call>.*?</tool_call>|<tool_code>.*?</tool_code>|<StartToolCall>.*?</StartToolCall>|<minimax:tool_call>.*?</minimax:tool_call>|<invoke\b.*?</invoke>|<param(?:eter)?\b[^>]*>.*?</param(?:eter)?>|<tool_use>.*?</tool_use>|<result>.*?</result>)"#).unwrap()
        });

        let result = TOOL_CALL_BLOCK_RE.replace_all(text, "");
        result.trim().to_string()
    }

    /// Strip ALL HTML comments from text.
    ///
    /// LLMs echo or hallucinate various HTML comment markers from context:
    /// `<!-- tools-v2: ... -->`, `<!-- lens -->`, `<!-- /tools-v2>`, etc.
    ///
    /// `<!-- tools-v2: [JSON] -->` is special-cased because its JSON payload
    /// embeds arbitrary tool output — including rustc's ` --> src/foo.rs:10`
    /// span arrows, React/JSX `{/* --> */}` fragments, markdown arrow
    /// glyphs, etc. A naive non-greedy `<!--.*?-->` terminates at the FIRST
    /// inner `-->` and leaks the rest of the JSON array as visible text in
    /// the TUI (screenshot 2026-04-17 16:58 — `cargo check` output bled
    /// into chat). For the v2 marker we parse the balanced JSON array and
    /// then consume the trailing ` -->`. Generic comments keep the regex.
    pub(crate) fn strip_html_comments(text: &str) -> String {
        use regex::Regex;
        use std::sync::LazyLock;

        let mut stripped = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("<!-- tools-v2:") {
            stripped.push_str(&rest[..start]);
            let after_prefix = &rest[start + "<!-- tools-v2:".len()..];
            let array_start = match after_prefix.find('[') {
                Some(i) => i,
                None => {
                    // Malformed — drop the opener and stop scanning v2.
                    rest = after_prefix;
                    break;
                }
            };
            let scan = &after_prefix[array_start..];
            let Some(array_end_rel) = find_balanced_json_end(scan) else {
                // Array never closes: drop the whole tail (well-formed
                // writers always close; an unclosed marker would dump raw
                // JSON otherwise).
                rest = "";
                break;
            };
            let tail = &scan[array_end_rel..];
            let tail_trim_len = tail.len() - tail.trim_start().len();
            let post = &tail[tail_trim_len..];
            if let Some(stripped_end) = post.strip_prefix("-->") {
                rest = stripped_end;
            } else {
                // No closing ` -->` — conservatively skip just the array so
                // downstream regex can still catch a stray `-->` later.
                rest = tail;
            }
        }
        stripped.push_str(rest);

        // Generic HTML comments (reasoning, lens, etc.) — safe with non-greedy
        // since none of those embed tool output.
        static HTML_COMMENT_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"(?s)<!--.*?-->"#).unwrap());
        let result = HTML_COMMENT_RE.replace_all(&stripped, "");

        // Collapse any runs of 3+ newlines left by stripping
        let collapsed = result.lines().collect::<Vec<_>>().join("\n");
        let trimmed = collapsed.trim().to_string();
        static MULTI_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
        MULTI_BLANK.replace_all(&trimmed, "\n\n").to_string()
    }
}

/// Walk a JSON array starting at `s[0] == '['` and return the byte offset
/// one-past the matching `]`. Tracks string + escape state so braces,
/// brackets, quotes or `-->` arrows embedded in string values don't fool
/// the depth counter. Returns `None` if the array never closes.
fn find_balanced_json_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (idx, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match b {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Detect repetition in a streaming text window.
///
/// Returns `true` if a substring of `min_match` bytes from the second half
/// of `window` also appears in the first half, indicating the provider is
/// looping the same content.
pub fn detect_text_repetition(window: &str, min_match: usize) -> bool {
    if min_match == 0 || window.len() < min_match * 2 {
        return false;
    }
    // Find a valid char boundary at or after the midpoint
    let mut half = window.len() / 2;
    while !window.is_char_boundary(half) && half < window.len() {
        half += 1;
    }
    let second_half = &window[half..];
    let mut check_len = min_match.min(second_half.len());
    // Ensure check_len lands on a char boundary within second_half
    while !second_half.is_char_boundary(check_len) && check_len < second_half.len() {
        check_len += 1;
    }
    if let Some(needle) = second_half.get(..check_len) {
        window[..half].contains(needle)
    } else {
        false
    }
}
