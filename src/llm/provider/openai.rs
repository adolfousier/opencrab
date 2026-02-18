//! OpenAI Provider Implementation
//!
//! Implements the Provider trait for OpenAI's GPT models.
//!
//! ## Supported Models
//! - gpt-4-turbo-preview
//! - gpt-4
//! - gpt-4-32k
//! - gpt-3.5-turbo
//! - gpt-3.5-turbo-16k
//!
//! ## Compatibility
//! This implementation also works with OpenAI-compatible APIs:
//! - Local LLMs via LM Studio (http://localhost:1234/v1)
//! - Ollama with OpenAI compatibility (http://localhost:11434/v1)
//! - LocalAI and other compatible APIs

use super::error::{ProviderError, Result};
use super::r#trait::{Provider, ProviderStream};
use super::types::*;
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// OpenAI provider for GPT models
#[derive(Clone)]
pub struct OpenAIProvider {
    api_key: String,
    base_url: String,
    client: Client,
    custom_default_model: Option<String>,
}

impl OpenAIProvider {
    /// Create a new OpenAI provider with official API
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .pool_idle_timeout(DEFAULT_POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key,
            base_url: DEFAULT_OPENAI_API_URL.to_string(),
            client,
            custom_default_model: None,
        }
    }

    /// Create provider for local LLM (LM Studio, Ollama, etc.)
    pub fn local(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .pool_idle_timeout(DEFAULT_POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key: "not-needed".to_string(),
            base_url,
            client,
            custom_default_model: None,
        }
    }

    /// Create with custom base URL
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .pool_idle_timeout(DEFAULT_POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key,
            base_url,
            client,
            custom_default_model: None,
        }
    }

    /// Set custom default model (useful for local LLMs with specific model names)
    pub fn with_default_model(mut self, model: String) -> Self {
        self.custom_default_model = Some(model);
        self
    }

    /// Build request headers
    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();

        // Only add authorization if not using local
        if self.api_key != "not-needed" {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key)
                    .parse()
                    .expect("Invalid API key format"),
            );
        }

        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("valid content-type"),
        );

        headers
    }

    /// Convert our generic request to OpenAI-specific format
    fn to_openai_request(&self, request: LLMRequest) -> OpenAIRequest {
        let mut messages = Vec::new();

        // Add system message if present
        if let Some(system) = request.system {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(system),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add conversation messages
        for msg in request.messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
            };

            // Separate content blocks by type
            let mut text_parts = Vec::new();
            let mut tool_uses = Vec::new();
            let mut tool_results = Vec::new();

            for block in msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(text);
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_uses.push((id, name, input));
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        tool_results.push((tool_use_id, content));
                    }
                    ContentBlock::Image { .. } => {
                        // Skip images for now (OpenAI needs special handling)
                        tracing::warn!("Image content blocks not yet supported for OpenAI");
                    }
                }
            }

            // Handle assistant messages with tool calls
            if !tool_uses.is_empty() {
                let openai_tool_calls = tool_uses
                    .into_iter()
                    .map(|(id, name, input)| OpenAIToolCall {
                        id,
                        r#type: "function".to_string(),
                        function: OpenAIFunctionCall {
                            name,
                            arguments: serde_json::to_string(&input).unwrap_or_default(),
                        },
                    })
                    .collect();

                let content_str = if text_parts.is_empty() {
                    None
                } else {
                    Some(text_parts.join("\n"))
                };

                messages.push(OpenAIMessage {
                    role: role.to_string(),
                    content: content_str,
                    tool_calls: Some(openai_tool_calls),
                    tool_call_id: None,
                });
            }
            // Handle tool result messages
            else if !tool_results.is_empty() {
                for (tool_use_id, content) in tool_results {
                    messages.push(OpenAIMessage {
                        role: "tool".to_string(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id),
                    });
                }
            }
            // Handle regular text messages
            else {
                let content_str = if text_parts.is_empty() {
                    Some(String::new())
                } else {
                    Some(text_parts.join("\n"))
                };

                messages.push(OpenAIMessage {
                    role: role.to_string(),
                    content: content_str,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        // Convert tools to OpenAI format
        let tools = request.tools.map(|tools| {
            tools
                .iter()
                .map(|tool| OpenAITool {
                    r#type: "function".to_string(),
                    function: OpenAIFunction {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.input_schema.clone(),
                    },
                })
                .collect()
        });

        OpenAIRequest {
            model: request.model,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: Some(request.stream),
            tools,
        }
    }

    /// Convert OpenAI response to our generic format
    #[allow(clippy::wrong_self_convention)]
    fn from_openai_response(&self, response: OpenAIResponse) -> LLMResponse {
        let choice = response
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".to_string(),
                    content: Some(String::new()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("error".to_string()),
            });

        // Convert content to content blocks
        let mut content_blocks = Vec::new();

        // Add text content if present
        if let Some(content) = choice.message.content
            && !content.is_empty() {
                content_blocks.push(ContentBlock::Text { text: content });
            }

        // Convert tool_calls to ToolUse content blocks
        if let Some(tool_calls) = choice.message.tool_calls {
            tracing::debug!(
                "Converting {} tool calls from OpenAI response",
                tool_calls.len()
            );
            for tool_call in tool_calls {
                // Parse arguments JSON string
                let input =
                    serde_json::from_str(&tool_call.function.arguments).unwrap_or_else(|e| {
                        tracing::warn!(
                            "Failed to parse tool arguments for {}: {}",
                            tool_call.function.name,
                            e
                        );
                        serde_json::json!({})
                    });

                tracing::debug!(
                    "Converted tool call: {} with id {}",
                    tool_call.function.name,
                    tool_call.id
                );

                content_blocks.push(ContentBlock::ToolUse {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    input,
                });
            }
        }

        // Map finish_reason to StopReason
        let stop_reason = choice
            .finish_reason
            .and_then(|reason| match reason.as_str() {
                "stop" => Some(StopReason::EndTurn),
                "length" => Some(StopReason::MaxTokens),
                "tool_calls" | "function_call" => Some(StopReason::ToolUse),
                _ => None,
            });

        LLMResponse {
            id: response.id,
            model: response.model,
            content: content_blocks,
            stop_reason,
            usage: TokenUsage {
                input_tokens: response.usage.prompt_tokens,
                output_tokens: response.usage.completion_tokens,
            },
        }
    }

    /// Handle API error response
    async fn handle_error(&self, response: reqwest::Response) -> ProviderError {
        let status = response.status().as_u16();

        // Extract Retry-After header for rate limits
        let retry_after = response.headers().get("retry-after").and_then(|v| {
            v.to_str().ok().and_then(|s| {
                // Retry-After can be either seconds or HTTP date
                // Try parsing as seconds first
                s.parse::<u64>().ok()
            })
        });

        // Try to parse error body
        if let Ok(error_body) = response.json::<OpenAIErrorResponse>().await {
            let message = if status == 429 {
                // Enhance rate limit error message
                if let Some(secs) = retry_after {
                    format!(
                        "{} (retry after {} seconds)",
                        error_body.error.message, secs
                    )
                } else {
                    format!(
                        "{} (rate limited, please retry later)",
                        error_body.error.message
                    )
                }
            } else {
                error_body.error.message
            };

            return if status == 429 {
                ProviderError::RateLimitExceeded(message)
            } else {
                ProviderError::ApiError {
                    status,
                    message,
                    error_type: Some(error_body.error.error_type.unwrap_or_default()),
                }
            };
        }

        // Fallback error
        if status == 429 {
            let message = if let Some(secs) = retry_after {
                format!("Rate limit exceeded (retry after {} seconds)", secs)
            } else {
                "Rate limit exceeded, please retry later".to_string()
            };
            ProviderError::RateLimitExceeded(message)
        } else {
            ProviderError::ApiError {
                status,
                message: "Unknown error".to_string(),
                error_type: None,
            }
        }
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        use super::retry::{retry_with_backoff, RetryConfig};

        let model = request.model.clone();
        let message_count = request.messages.len();
        let openai_request = self.to_openai_request(request);
        let retry_config = RetryConfig::default();

        let tool_count = openai_request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        tracing::info!(
            "OpenAI API request: model={}, messages={}, max_tokens={}, tools={}",
            model,
            message_count,
            openai_request.max_tokens.unwrap_or(4096),
            tool_count
        );
        if tool_count == 0 {
            tracing::warn!(
                "OpenAI request has NO tools - LLM won't know about file/bash operations!"
            );
        }

        // Retry the entire API call with exponential backoff
        let result = retry_with_backoff(
            || async {
                tracing::debug!("Sending request to OpenAI API: {}", self.base_url);
                let response = self
                    .client
                    .post(&self.base_url)
                    .headers(self.headers())
                    .json(&openai_request)
                    .send()
                    .await?;

                let status = response.status();
                tracing::debug!("OpenAI API response status: {}", status);

                if !status.is_success() {
                    return Err(self.handle_error(response).await);
                }

                let openai_response: OpenAIResponse = response.json().await?;
                let llm_response = self.from_openai_response(openai_response);

                tracing::info!(
                    "OpenAI API response: input_tokens={}, output_tokens={}, stop_reason={:?}",
                    llm_response.usage.input_tokens,
                    llm_response.usage.output_tokens,
                    llm_response.stop_reason
                );

                Ok(llm_response)
            },
            &retry_config,
        )
        .await;

        if let Err(ref e) = result {
            tracing::error!("OpenAI API request failed: {}", e);
        }

        result
    }

    async fn stream(&self, request: LLMRequest) -> Result<ProviderStream> {
        use super::retry::{retry_with_backoff, RetryConfig};

        let model = request.model.clone();
        let message_count = request.messages.len();
        tracing::info!(
            "OpenAI streaming request: model={}, messages={}",
            model,
            message_count
        );

        let mut openai_request = self.to_openai_request(request);
        openai_request.stream = Some(true);
        let retry_config = RetryConfig::default();

        // Retry the stream connection establishment
        let response = retry_with_backoff(
            || async {
                let response = self
                    .client
                    .post(&self.base_url)
                    .headers(self.headers())
                    .json(&openai_request)
                    .send()
                    .await?;

                if !response.status().is_success() {
                    return Err(self.handle_error(response).await);
                }

                Ok(response)
            },
            &retry_config,
        )
        .await?;

        // Parse Server-Sent Events stream
        let byte_stream = response.bytes_stream();
        let event_stream = byte_stream.map(|chunk_result| {
            chunk_result
                .map_err(|e| ProviderError::StreamError(e.to_string()))
                .map(|chunk| {
                    let text = String::from_utf8_lossy(&chunk);

                    // Parse SSE format: "data: {...}\n\n"
                    for line in text.lines() {
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            // Check for stream end
                            if json_str == "[DONE]" {
                                tracing::trace!("OpenAI stream completed with [DONE] marker");
                                return StreamEvent::MessageStop;
                            }

                            // Parse JSON chunk
                            match serde_json::from_str::<OpenAIStreamChunk>(json_str) {
                                Ok(chunk) => {
                                    if let Some(choice) = chunk.choices.first()
                                        && let Some(ref delta) = choice.delta
                                            && let Some(ref content) = delta.content
                                                && !content.is_empty() {
                                                    return StreamEvent::ContentBlockDelta {
                                                        index: 0,
                                                        delta: ContentDelta::TextDelta {
                                                            text: content.clone(),
                                                        },
                                                    };
                                                }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to parse OpenAI stream chunk: {}. Data: {}",
                                        e,
                                        json_str.chars().take(200).collect::<String>()
                                    );
                                }
                            }
                        } else if !line.trim().is_empty()
                            && !line.starts_with("event:")
                            && !line.starts_with("id:")
                            && !line.starts_with("retry:")
                        {
                            // Log unexpected SSE line formats for debugging
                            tracing::debug!("OpenAI: Unexpected SSE line format: {}", line);
                        }
                    }

                    // Skip non-data lines
                    StreamEvent::Ping
                })
        });

        Ok(Box::pin(event_stream))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        // Only GPT-4 Vision models support vision
        false
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> &str {
        self.custom_default_model
            .as_deref()
            .unwrap_or("gpt-4-turbo-preview")
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "gpt-4-turbo-preview".to_string(),
            "gpt-4".to_string(),
            "gpt-4-32k".to_string(),
            "gpt-3.5-turbo".to_string(),
            "gpt-3.5-turbo-16k".to_string(),
        ]
    }

    async fn fetch_models(&self) -> Vec<String> {
        // Derive models URL from base_url (replace /chat/completions with /models)
        let models_url = self
            .base_url
            .replace("/chat/completions", "/models");

        #[derive(Deserialize)]
        struct ModelEntry { id: String }
        #[derive(Deserialize)]
        struct ModelsResponse { data: Vec<ModelEntry> }

        match self.client
            .get(&models_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ModelsResponse>().await {
                    Ok(body) => {
                        let mut models: Vec<String> = body.data
                            .into_iter()
                            .map(|m| m.id)
                            .collect();
                        models.sort();
                        if models.is_empty() {
                            return self.supported_models();
                        }
                        models
                    }
                    Err(_) => self.supported_models(),
                }
            }
            _ => self.supported_models(),
        }
    }

    fn context_window(&self, model: &str) -> Option<u32> {
        match model {
            "gpt-4-turbo-preview" => Some(128_000),
            "gpt-4" => Some(8_192),
            "gpt-4-32k" => Some(32_768),
            "gpt-3.5-turbo" => Some(4_096),
            "gpt-3.5-turbo-16k" => Some(16_384),
            _ => None,
        }
    }

    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        // Costs per million tokens (as of 2024)
        let (input_cost, output_cost) = match model {
            "gpt-4-turbo-preview" => (10.0, 30.0),
            "gpt-4" => (30.0, 60.0),
            "gpt-4-32k" => (60.0, 120.0),
            "gpt-3.5-turbo" => (0.5, 1.5),
            "gpt-3.5-turbo-16k" => (3.0, 4.0),
            _ => return 0.0,
        };

        let input_cost_total = (input_tokens as f64 / 1_000_000.0) * input_cost;
        let output_cost_total = (output_tokens as f64 / 1_000_000.0) * output_cost;

        input_cost_total + output_cost_total
    }
}

// ============================================================================
// OpenAI API Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIToolCall {
    id: String,
    r#type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAITool {
    r#type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIResponse {
    id: String,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: OpenAIUsage,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAIChoice {
    index: u32,
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAIStreamChunk {
    id: String,
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAIStreamChoice {
    index: u32,
    delta: Option<OpenAIMessageDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAIMessageDelta {
    role: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIError {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_creation() {
        let provider = OpenAIProvider::new("test-key".to_string());
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.base_url, DEFAULT_OPENAI_API_URL);
    }

    #[test]
    fn test_local_provider_creation() {
        let provider =
            OpenAIProvider::local("http://localhost:1234/v1/chat/completions".to_string());
        assert_eq!(provider.api_key, "not-needed");
    }

    #[test]
    fn test_supported_models() {
        let provider = OpenAIProvider::new("test-key".to_string());
        let models = provider.supported_models();
        assert!(models.contains(&"gpt-4".to_string()));
        assert!(models.contains(&"gpt-3.5-turbo".to_string()));
    }

    #[test]
    fn test_context_window() {
        let provider = OpenAIProvider::new("test-key".to_string());
        assert_eq!(provider.context_window("gpt-4"), Some(8_192));
        assert_eq!(
            provider.context_window("gpt-4-turbo-preview"),
            Some(128_000)
        );
        assert_eq!(provider.context_window("unknown"), None);
    }

    #[test]
    fn test_calculate_cost() {
        let provider = OpenAIProvider::new("test-key".to_string());
        // 1000 input + 1000 output tokens on gpt-3.5-turbo
        // Cost: (1000/1M * 0.5) + (1000/1M * 1.5) = 0.0005 + 0.0015 = 0.002
        let cost = provider.calculate_cost("gpt-3.5-turbo", 1000, 1000);
        assert!((cost - 0.002).abs() < 0.0001);
    }
}
