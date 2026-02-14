//! Qwen Provider Implementation
//!
//! Implements the Provider trait for Alibaba's Qwen models with:
//! - Hermes-style tool calling for optimal function calling performance
//! - Qwen3 thinking mode support
//! - Local deployment (vLLM, LM Studio) and DashScope cloud API
//!
//! ## Supported Models
//! - qwen3-235b-a22b (Qwen3 MoE flagship)
//! - qwen3-32b (Qwen3 32B)
//! - qwen3-14b (Qwen3 14B)
//! - qwen3-8b (Qwen3 8B)
//! - qwen2.5-coder-32b-instruct
//! - qwen2.5-coder-14b-instruct
//! - qwen2.5-coder-7b-instruct
//! - qwen2.5-72b-instruct
//! - qwen2.5-32b-instruct

use super::error::{ProviderError, Result};
use super::r#trait::{Provider, ProviderStream};
use super::types::*;
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// DashScope API endpoints
const DASHSCOPE_INTL_URL: &str =
    "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions";
const DASHSCOPE_CN_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180); // Longer for reasoning models
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Tool call parsing mode for Qwen models
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallParser {
    /// Standard OpenAI format (works with LM Studio auto-parsing)
    OpenAI,
    /// Hermes-style parsing with XML tags (recommended for Qwen3 via vLLM)
    Hermes,
    /// Native Qwen format with Unicode markers (✿FUNCTION✿, ✿ARGS✿, etc.)
    NativeQwen,
}

// Native Qwen function calling markers (from qwen_fncall_prompt.py)
const FN_NAME: &str = "✿FUNCTION✿";
const FN_ARGS: &str = "✿ARGS✿";
const FN_RESULT: &str = "✿RESULT✿";
const FN_EXIT: &str = "✿RETURN✿";

// Stop words for native Qwen format - prevent model from generating these
#[allow(dead_code)] // Used in tests, will be used for stop word configuration
const QWEN_FN_STOP_WORDS: &[&str] = &["✿RESULT✿", "✿RETURN✿"];

/// Qwen thinking mode configuration
#[derive(Debug, Clone, Default)]
pub struct ThinkingConfig {
    /// Enable thinking mode (Qwen3 feature)
    pub enabled: bool,
    /// Budget tokens for thinking (optional)
    pub budget_tokens: Option<u32>,
}

/// Qwen provider for Alibaba's Qwen models
#[derive(Clone)]
pub struct QwenProvider {
    api_key: String,
    base_url: String,
    client: Client,
    custom_default_model: Option<String>,
    tool_parser: ToolCallParser,
    thinking_config: ThinkingConfig,
}

impl QwenProvider {
    /// Create provider for DashScope International (Singapore)
    pub fn dashscope_intl(api_key: String) -> Self {
        Self::with_base_url(api_key, DASHSCOPE_INTL_URL.to_string())
    }

    /// Create provider for DashScope China (Beijing)
    pub fn dashscope_cn(api_key: String) -> Self {
        Self::with_base_url(api_key, DASHSCOPE_CN_URL.to_string())
    }

    /// Create provider for local Qwen deployment (vLLM, LM Studio, Ollama)
    pub fn local(base_url: String) -> Self {
        let client = Self::build_client();

        Self {
            api_key: "not-needed".to_string(),
            base_url,
            client,
            custom_default_model: None,
            tool_parser: ToolCallParser::Hermes, // Default to Hermes for local
            thinking_config: ThinkingConfig::default(),
        }
    }

    /// Create with custom base URL and API key
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        let client = Self::build_client();

        Self {
            api_key,
            base_url,
            client,
            custom_default_model: None,
            tool_parser: ToolCallParser::OpenAI, // Default to OpenAI for cloud
            thinking_config: ThinkingConfig::default(),
        }
    }

    /// Set custom default model
    pub fn with_default_model(mut self, model: String) -> Self {
        self.custom_default_model = Some(model);
        self
    }

    /// Set tool call parsing mode
    pub fn with_tool_parser(mut self, parser: ToolCallParser) -> Self {
        self.tool_parser = parser;
        self
    }

    /// Enable Qwen3 thinking mode
    pub fn with_thinking(mut self, enabled: bool) -> Self {
        self.thinking_config.enabled = enabled;
        self
    }

    /// Set thinking budget tokens (optional)
    pub fn with_thinking_budget(mut self, budget_tokens: u32) -> Self {
        self.thinking_config.budget_tokens = Some(budget_tokens);
        self
    }

    fn build_client() -> Client {
        Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .pool_idle_timeout(DEFAULT_POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .expect("Failed to create HTTP client")
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
            "application/json".parse().unwrap(),
        );

        headers
    }

    /// Format tools in Hermes style for Qwen3
    fn format_hermes_tools(&self, tools: &[Tool]) -> String {
        let mut result = String::from("You are a function calling AI model. You are provided with function signatures within <tools></tools> XML tags. You may call one or more functions to assist with the user query. Don't make assumptions about what values to plug into functions. Here are the available tools:\n<tools>\n");

        for tool in tools {
            result.push_str(&format!(
                r#"{{"type": "function", "function": {{"name": "{}", "description": "{}", "parameters": {}}}}}"#,
                tool.name,
                tool.description.replace('"', r#"\""#),
                serde_json::to_string(&tool.input_schema).unwrap_or_default()
            ));
            result.push('\n');
        }

        result.push_str("</tools>\n\n");
        result.push_str("Use the following pydantic model json schema for each tool call you will make: {\"properties\": {\"arguments\": {\"title\": \"Arguments\", \"type\": \"object\"}, \"name\": {\"title\": \"Name\", \"type\": \"string\"}}, \"required\": [\"arguments\", \"name\"], \"title\": \"FunctionCall\", \"type\": \"object\"}\n\n");
        result.push_str("For each function call return a json object with function name and arguments within <tool_call></tool_call> XML tags as follows:\n");
        result.push_str(
            "<tool_call>\n{\"name\": <function-name>, \"arguments\": <args-dict>}\n</tool_call>",
        );

        result
    }

    /// Parse Hermes-style tool calls from response text
    fn parse_hermes_tool_calls(&self, text: &str) -> Vec<(String, String, serde_json::Value)> {
        let mut tool_calls = Vec::new();

        // Find all <tool_call> ... </tool_call> blocks
        let mut remaining = text;
        while let Some(start) = remaining.find("<tool_call>") {
            if let Some(end) = remaining[start..].find("</tool_call>") {
                let tool_call_content = &remaining[start + 11..start + end];
                let trimmed = tool_call_content.trim();

                // Parse the JSON inside
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if let (Some(name), Some(arguments)) = (
                        parsed.get("name").and_then(|v| v.as_str()),
                        parsed.get("arguments"),
                    ) {
                        let id = format!(
                            "call_{}",
                            &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]
                        );
                        tool_calls.push((id, name.to_string(), arguments.clone()));
                    }
                }

                remaining = &remaining[start + end + 12..];
            } else {
                break;
            }
        }

        tool_calls
    }

    /// Extract thinking content from Qwen3 response
    fn extract_thinking(&self, text: &str) -> (Option<String>, String) {
        if !self.thinking_config.enabled {
            return (None, text.to_string());
        }

        // Look for <think> ... </think> blocks
        if let Some(start) = text.find("<think>") {
            if let Some(end) = text.find("</think>") {
                let thinking = text[start + 7..end].trim().to_string();
                let before = &text[..start];
                let after = &text[end + 8..];
                let remaining = format!("{}{}", before.trim(), after.trim());
                return (Some(thinking), remaining);
            }
        }

        (None, text.to_string())
    }

    /// Format tools in native Qwen format (with Unicode markers)
    fn format_native_qwen_tools(&self, tools: &[Tool]) -> String {
        let mut result = String::from(
            "# Tools\n\nYou may call one or more functions to assist with the user query.\n\n\
             You are provided with function signatures within <tool_info></tool_info> XML tags:\n\
             <tool_info>\n",
        );

        for tool in tools {
            // Format each tool in Qwen-Agent style
            result.push_str(&format!(
                "### {}\n\n{}: {} Parameters: {} Format the arguments as a JSON object.\n\n",
                tool.name,
                tool.name,
                tool.description,
                serde_json::to_string(&tool.input_schema).unwrap_or_default()
            ));
        }

        result.push_str("</tool_info>\n\n");
        result.push_str(&format!(
            "For each function call, return a line with the function name prefixed by '{}:', \
             followed by a line with arguments prefixed by '{}:'.\n\
             Example:\n\
             {}: function_name\n\
             {}: {{\"arg1\": \"value1\"}}\n\n\
             When you have received the results and are ready to respond to the user, \
             output '{}:' followed by your final response.",
            FN_NAME, FN_ARGS, FN_NAME, FN_ARGS, FN_EXIT
        ));

        result
    }

    /// Parse native Qwen function calls (with Unicode markers)
    fn parse_native_qwen_tool_calls(&self, text: &str) -> Vec<(String, String, serde_json::Value)> {
        let mut tool_calls = Vec::new();

        // Split by function marker and parse each call
        let parts: Vec<&str> = text.split(FN_NAME).collect();

        for part in parts.iter().skip(1) {
            // Skip the first part (before any function call)
            let part = part.trim();

            // Extract function name (after ": ")
            if let Some(colon_pos) = part.find(':') {
                let after_colon = &part[colon_pos + 1..].trim_start();

                // Find the function name (up to newline or FN_ARGS)
                let fn_name = if let Some(newline_pos) = after_colon.find('\n') {
                    after_colon[..newline_pos].trim().to_string()
                } else if let Some(args_pos) = after_colon.find(FN_ARGS) {
                    after_colon[..args_pos].trim().to_string()
                } else {
                    after_colon.trim().to_string()
                };

                // Extract arguments
                if let Some(args_start) = part.find(FN_ARGS) {
                    let args_section = &part[args_start + FN_ARGS.len()..];
                    let args_text = if let Some(colon) = args_section.find(':') {
                        let after_args_colon = &args_section[colon + 1..];
                        // Find end of JSON (next marker or end of string)
                        let end_pos = after_args_colon
                            .find(FN_NAME)
                            .or_else(|| after_args_colon.find(FN_RESULT))
                            .or_else(|| after_args_colon.find(FN_EXIT))
                            .unwrap_or(after_args_colon.len());
                        after_args_colon[..end_pos].trim()
                    } else {
                        ""
                    };

                    // Parse arguments JSON
                    if !fn_name.is_empty() && !args_text.is_empty() {
                        match serde_json::from_str::<serde_json::Value>(args_text) {
                            Ok(args) => {
                                let id = format!(
                                    "call_{}",
                                    &uuid::Uuid::new_v4().to_string().replace('-', "")[..24]
                                );
                                tool_calls.push((id, fn_name, args));
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse native Qwen tool arguments: {}", e);
                            }
                        }
                    }
                }
            }
        }

        tool_calls
    }

    /// Format tool result for native Qwen format
    fn format_native_qwen_result(&self, result: &str) -> String {
        format!("\n{}: {}\n{}:", FN_RESULT, result, FN_EXIT)
    }

    /// Remove incomplete markers from streamed text
    fn clean_incomplete_markers(&self, text: &str) -> String {
        let markers = [FN_NAME, FN_ARGS, FN_RESULT, FN_EXIT];
        let mut result = text.to_string();

        // Remove partial markers at the end of text
        // Handle multi-byte Unicode characters properly
        for marker in &markers {
            let marker_chars: Vec<char> = marker.chars().collect();
            for i in 1..marker_chars.len() {
                let partial: String = marker_chars[..i].iter().collect();
                if result.ends_with(&partial) {
                    let new_len = result.len() - partial.len();
                    result = result[..new_len].to_string();
                    break;
                }
            }
        }

        result
    }

    /// Convert our generic request to Qwen-specific format
    fn to_qwen_request(&self, request: LLMRequest) -> QwenRequest {
        let mut messages = Vec::new();
        let mut system_content = String::new();

        // Add system message with Hermes tool instructions if using Hermes parser
        if let Some(system) = &request.system {
            system_content = system.clone();
        }

        // Add tool instructions to system prompt based on parser type
        match self.tool_parser {
            ToolCallParser::Hermes => {
                if let Some(tools) = &request.tools {
                    if !tools.is_empty() {
                        let hermes_tools = self.format_hermes_tools(tools);
                        if system_content.is_empty() {
                            system_content = hermes_tools;
                        } else {
                            system_content = format!("{}\n\n{}", hermes_tools, system_content);
                        }
                    }
                }
            }
            ToolCallParser::NativeQwen => {
                if let Some(tools) = &request.tools {
                    if !tools.is_empty() {
                        let native_tools = self.format_native_qwen_tools(tools);
                        if system_content.is_empty() {
                            system_content = native_tools;
                        } else {
                            system_content = format!("{}\n\n{}", native_tools, system_content);
                        }
                    }
                }
            }
            ToolCallParser::OpenAI => {
                // OpenAI format uses the tools field in the request, not system prompt
            }
        }

        // Add thinking mode instruction
        if self.thinking_config.enabled {
            let thinking_instruction = if let Some(budget) = self.thinking_config.budget_tokens {
                format!("\n\nIMPORTANT: You have thinking mode enabled. Use <think></think> tags to show your reasoning process. Budget: {} tokens for thinking.", budget)
            } else {
                "\n\nIMPORTANT: You have thinking mode enabled. Use <think></think> tags to show your reasoning process before providing your final answer.".to_string()
            };
            system_content.push_str(&thinking_instruction);
        }

        if !system_content.is_empty() {
            messages.push(QwenMessage {
                role: "system".to_string(),
                content: Some(system_content),
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
                        tracing::warn!("Image content blocks not yet supported for Qwen");
                    }
                }
            }

            // Handle assistant messages with tool calls
            if !tool_uses.is_empty() {
                match self.tool_parser {
                    ToolCallParser::Hermes => {
                        // Format as Hermes-style tool calls in text
                        let mut content = text_parts.join("\n");
                        for (_, name, input) in tool_uses {
                            content.push_str(&format!(
                                "\n<tool_call>\n{{\"name\": \"{}\", \"arguments\": {}}}\n</tool_call>",
                                name,
                                serde_json::to_string(&input).unwrap_or_default()
                            ));
                        }
                        messages.push(QwenMessage {
                            role: role.to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                    ToolCallParser::NativeQwen => {
                        // Format as native Qwen-style tool calls with Unicode markers
                        let mut content = text_parts.join("\n");
                        for (_, name, input) in tool_uses {
                            content.push_str(&format!(
                                "\n{}: {}\n{}: {}",
                                FN_NAME,
                                name,
                                FN_ARGS,
                                serde_json::to_string(&input).unwrap_or_default()
                            ));
                        }
                        messages.push(QwenMessage {
                            role: role.to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                    ToolCallParser::OpenAI => {
                        // OpenAI-style tool calls
                        let qwen_tool_calls = tool_uses
                            .into_iter()
                            .map(|(id, name, input)| QwenToolCall {
                                id,
                                r#type: "function".to_string(),
                                function: QwenFunctionCall {
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

                        messages.push(QwenMessage {
                            role: role.to_string(),
                            content: content_str,
                            tool_calls: Some(qwen_tool_calls),
                            tool_call_id: None,
                        });
                    }
                }
            }
            // Handle tool result messages
            else if !tool_results.is_empty() {
                match self.tool_parser {
                    ToolCallParser::Hermes => {
                        // Format tool results as user message with context
                        for (tool_use_id, content) in tool_results {
                            messages.push(QwenMessage {
                                role: "user".to_string(),
                                content: Some(format!(
                                    "<tool_response>\nTool call ID: {}\nResult: {}\n</tool_response>",
                                    tool_use_id, content
                                )),
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        }
                    }
                    ToolCallParser::NativeQwen => {
                        // Format tool results with native Qwen markers
                        for (_tool_use_id, content) in tool_results {
                            messages.push(QwenMessage {
                                role: "user".to_string(),
                                content: Some(self.format_native_qwen_result(&content)),
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        }
                    }
                    ToolCallParser::OpenAI => {
                        // OpenAI-style tool results
                        for (tool_use_id, content) in tool_results {
                            messages.push(QwenMessage {
                                role: "tool".to_string(),
                                content: Some(content),
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id),
                            });
                        }
                    }
                }
            }
            // Handle regular text messages
            else {
                let content_str = if text_parts.is_empty() {
                    Some(String::new())
                } else {
                    Some(text_parts.join("\n"))
                };

                messages.push(QwenMessage {
                    role: role.to_string(),
                    content: content_str,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        // Convert tools to OpenAI format (only if not using Hermes)
        let tools = if self.tool_parser == ToolCallParser::OpenAI {
            request.tools.map(|tools| {
                tools
                    .iter()
                    .map(|tool| QwenTool {
                        r#type: "function".to_string(),
                        function: QwenFunction {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            parameters: tool.input_schema.clone(),
                        },
                    })
                    .collect()
            })
        } else {
            None // Hermes-style uses system prompt instead
        };

        QwenRequest {
            model: request.model,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: Some(request.stream),
            tools,
        }
    }

    /// Convert Qwen response to our generic format
    #[allow(clippy::wrong_self_convention)]
    fn from_qwen_response(&self, response: QwenResponse) -> LLMResponse {
        let choice = response
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| QwenChoice {
                index: 0,
                message: QwenMessage {
                    role: "assistant".to_string(),
                    content: Some(String::new()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("error".to_string()),
            });

        let mut content_blocks = Vec::new();
        let mut has_tool_calls = false;

        // Process content text
        if let Some(content) = choice.message.content {
            if !content.is_empty() {
                // Extract thinking if enabled
                let (thinking, remaining) = self.extract_thinking(&content);

                if let Some(think_content) = thinking {
                    tracing::info!("🧠 Qwen3 thinking: {}", think_content);
                    // Optionally add thinking as a separate content block
                    content_blocks.push(ContentBlock::Text {
                        text: format!("💭 *Thinking:* {}", think_content),
                    });
                }

                // Parse tool calls based on parser type
                match self.tool_parser {
                    ToolCallParser::Hermes => {
                        let hermes_calls = self.parse_hermes_tool_calls(&remaining);

                        if !hermes_calls.is_empty() {
                            has_tool_calls = true;

                            // Remove tool_call tags from text for display
                            let mut clean_text = remaining.clone();
                            while let Some(start) = clean_text.find("<tool_call>") {
                                if let Some(end) = clean_text.find("</tool_call>") {
                                    clean_text = format!(
                                        "{}{}",
                                        &clean_text[..start],
                                        &clean_text[end + 12..]
                                    );
                                } else {
                                    break;
                                }
                            }
                            let clean_text = clean_text.trim();

                            if !clean_text.is_empty() {
                                content_blocks.push(ContentBlock::Text {
                                    text: clean_text.to_string(),
                                });
                            }

                            // Add tool use blocks
                            for (id, name, input) in hermes_calls {
                                tracing::debug!("Parsed Hermes tool call: {} with id {}", name, id);
                                content_blocks.push(ContentBlock::ToolUse { id, name, input });
                            }
                        } else if !remaining.is_empty() {
                            content_blocks.push(ContentBlock::Text { text: remaining });
                        }
                    }
                    ToolCallParser::NativeQwen => {
                        let native_calls = self.parse_native_qwen_tool_calls(&remaining);

                        if !native_calls.is_empty() {
                            has_tool_calls = true;

                            // Remove native Qwen markers from text for display
                            let mut clean_text = remaining.clone();
                            // Remove function call blocks
                            while let Some(start) = clean_text.find(FN_NAME) {
                                let end_pos = clean_text[start..]
                                    .find(FN_RESULT)
                                    .or_else(|| clean_text[start..].find(FN_EXIT))
                                    .map(|p| start + p)
                                    .unwrap_or(clean_text.len());
                                clean_text =
                                    format!("{}{}", &clean_text[..start], &clean_text[end_pos..]);
                            }
                            // Also remove trailing markers
                            clean_text = clean_text.replace(FN_RESULT, "").replace(FN_EXIT, "");
                            let clean_text = self.clean_incomplete_markers(&clean_text);
                            let clean_text = clean_text.trim();

                            if !clean_text.is_empty() {
                                content_blocks.push(ContentBlock::Text {
                                    text: clean_text.to_string(),
                                });
                            }

                            // Add tool use blocks
                            for (id, name, input) in native_calls {
                                tracing::debug!(
                                    "Parsed native Qwen tool call: {} with id {}",
                                    name,
                                    id
                                );
                                content_blocks.push(ContentBlock::ToolUse { id, name, input });
                            }
                        } else if !remaining.is_empty() {
                            // Clean any markers from text
                            let clean = self.clean_incomplete_markers(&remaining);
                            if !clean.is_empty() {
                                content_blocks.push(ContentBlock::Text { text: clean });
                            }
                        }
                    }
                    ToolCallParser::OpenAI => {
                        // OpenAI format doesn't embed tool calls in text
                        if !remaining.is_empty() {
                            content_blocks.push(ContentBlock::Text { text: remaining });
                        }
                    }
                }
            }
        }

        // Convert OpenAI-style tool_calls to ToolUse content blocks
        if let Some(tool_calls) = choice.message.tool_calls {
            if !tool_calls.is_empty() {
                has_tool_calls = true;
                tracing::debug!(
                    "Converting {} tool calls from Qwen response",
                    tool_calls.len()
                );
                for tool_call in tool_calls {
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
        }

        // Map finish_reason to StopReason
        let stop_reason = if has_tool_calls {
            Some(StopReason::ToolUse)
        } else {
            choice
                .finish_reason
                .and_then(|reason| match reason.as_str() {
                    "stop" => Some(StopReason::EndTurn),
                    "length" => Some(StopReason::MaxTokens),
                    "tool_calls" | "function_call" => Some(StopReason::ToolUse),
                    _ => None,
                })
        };

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

        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<u64>().ok()));

        if let Ok(error_body) = response.json::<QwenErrorResponse>().await {
            let message = if status == 429 {
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
                    error_type: error_body.error.error_type,
                }
            };
        }

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
impl Provider for QwenProvider {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        use super::retry::{retry_with_backoff, RetryConfig};

        let qwen_request = self.to_qwen_request(request);
        let retry_config = RetryConfig::default();

        let tool_count = qwen_request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        tracing::debug!(
            "Sending Qwen request to {} with model {} and {} tools (parser: {:?})",
            self.base_url,
            qwen_request.model,
            tool_count,
            self.tool_parser
        );

        if self.tool_parser == ToolCallParser::Hermes {
            tracing::info!("🔧 Using Hermes-style tool calling for Qwen");
        }

        if self.thinking_config.enabled {
            tracing::info!("🧠 Qwen3 thinking mode enabled");
        }

        retry_with_backoff(
            || async {
                let response = self
                    .client
                    .post(&self.base_url)
                    .headers(self.headers())
                    .json(&qwen_request)
                    .send()
                    .await?;

                if !response.status().is_success() {
                    return Err(self.handle_error(response).await);
                }

                let qwen_response: QwenResponse = response.json().await?;
                Ok(self.from_qwen_response(qwen_response))
            },
            &retry_config,
        )
        .await
    }

    async fn stream(&self, request: LLMRequest) -> Result<ProviderStream> {
        use super::retry::{retry_with_backoff, RetryConfig};

        let mut qwen_request = self.to_qwen_request(request);
        qwen_request.stream = Some(true);
        let retry_config = RetryConfig::default();

        tracing::debug!(
            "Starting Qwen stream to {} with model {}",
            self.base_url,
            qwen_request.model
        );

        let response = retry_with_backoff(
            || async {
                let response = self
                    .client
                    .post(&self.base_url)
                    .headers(self.headers())
                    .json(&qwen_request)
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

                    for line in text.lines() {
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            if json_str == "[DONE]" {
                                return StreamEvent::MessageStop;
                            }

                            if let Ok(chunk) = serde_json::from_str::<QwenStreamChunk>(json_str) {
                                if let Some(choice) = chunk.choices.first() {
                                    if let Some(ref delta) = choice.delta {
                                        if let Some(ref content) = delta.content {
                                            if !content.is_empty() {
                                                return StreamEvent::ContentBlockDelta {
                                                    index: 0,
                                                    delta: ContentDelta::TextDelta {
                                                        text: content.clone(),
                                                    },
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

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
        // Qwen-VL models support vision, but we'll add this later
        false
    }

    fn name(&self) -> &str {
        "qwen"
    }

    fn default_model(&self) -> &str {
        self.custom_default_model.as_deref().unwrap_or("qwen3-8b")
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            // Qwen3 models
            "qwen3-235b-a22b".to_string(),
            "qwen3-32b".to_string(),
            "qwen3-14b".to_string(),
            "qwen3-8b".to_string(),
            // Qwen2.5 Coder models
            "qwen2.5-coder-32b-instruct".to_string(),
            "qwen2.5-coder-14b-instruct".to_string(),
            "qwen2.5-coder-7b-instruct".to_string(),
            // Qwen2.5 base models
            "qwen2.5-72b-instruct".to_string(),
            "qwen2.5-32b-instruct".to_string(),
            "qwen2.5-14b-instruct".to_string(),
            "qwen2.5-7b-instruct".to_string(),
            // Qwen Max (DashScope)
            "qwen-max".to_string(),
            "qwen-plus".to_string(),
            "qwen-turbo".to_string(),
        ]
    }

    fn validate_model(&self, model: &str) -> bool {
        // Accept any model for local deployments
        if self.api_key == "not-needed" {
            return true;
        }
        self.supported_models().contains(&model.to_string()) || model.starts_with("qwen")
    }

    fn context_window(&self, model: &str) -> Option<u32> {
        match model {
            // Qwen3 models
            "qwen3-235b-a22b" => Some(131_072),
            "qwen3-32b" => Some(131_072),
            "qwen3-14b" => Some(131_072),
            "qwen3-8b" => Some(131_072),
            // Qwen2.5 Coder models
            "qwen2.5-coder-32b-instruct" => Some(131_072),
            "qwen2.5-coder-14b-instruct" => Some(131_072),
            "qwen2.5-coder-7b-instruct" => Some(131_072),
            // Qwen2.5 base models
            "qwen2.5-72b-instruct" => Some(131_072),
            "qwen2.5-32b-instruct" => Some(131_072),
            "qwen2.5-14b-instruct" => Some(131_072),
            "qwen2.5-7b-instruct" => Some(131_072),
            // DashScope models
            "qwen-max" => Some(32_768),
            "qwen-plus" => Some(131_072),
            "qwen-turbo" => Some(131_072),
            _ => Some(32_768), // Conservative default
        }
    }

    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        // DashScope pricing (as of 2025) in USD per million tokens
        // Local models have no cost
        if self.api_key == "not-needed" {
            return 0.0;
        }

        let (input_cost, output_cost) = match model {
            "qwen-max" => (2.4, 9.6),   // Premium tier
            "qwen-plus" => (0.8, 2.0),  // Standard tier
            "qwen-turbo" => (0.3, 0.6), // Economy tier
            _ => return 0.0,            // Unknown/local models
        };

        let input_cost_total = (input_tokens as f64 / 1_000_000.0) * input_cost;
        let output_cost_total = (output_tokens as f64 / 1_000_000.0) * output_cost;

        input_cost_total + output_cost_total
    }
}

// ============================================================================
// Qwen API Types (OpenAI-compatible)
// ============================================================================

#[derive(Debug, Clone, Serialize)]
struct QwenRequest {
    model: String,
    messages: Vec<QwenMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<QwenTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QwenMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<QwenToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QwenToolCall {
    id: String,
    r#type: String,
    function: QwenFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QwenFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
struct QwenTool {
    r#type: String,
    function: QwenFunction,
}

#[derive(Debug, Clone, Serialize)]
struct QwenFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct QwenResponse {
    id: String,
    model: String,
    choices: Vec<QwenChoice>,
    usage: QwenUsage,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct QwenChoice {
    index: u32,
    message: QwenMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct QwenUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct QwenStreamChunk {
    id: String,
    choices: Vec<QwenStreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct QwenStreamChoice {
    index: u32,
    delta: Option<QwenMessageDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct QwenMessageDelta {
    role: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct QwenErrorResponse {
    error: QwenError,
}

#[derive(Debug, Clone, Deserialize)]
struct QwenError {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_provider_creation() {
        let provider = QwenProvider::dashscope_intl("test-key".to_string());
        assert_eq!(provider.name(), "qwen");
        assert_eq!(provider.base_url, DASHSCOPE_INTL_URL);
    }

    #[test]
    fn test_local_provider_creation() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string());
        assert_eq!(provider.api_key, "not-needed");
        assert_eq!(provider.tool_parser, ToolCallParser::Hermes);
    }

    #[test]
    fn test_tool_parser_configuration() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::OpenAI);
        assert_eq!(provider.tool_parser, ToolCallParser::OpenAI);
    }

    #[test]
    fn test_thinking_mode_configuration() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_thinking(true)
            .with_thinking_budget(5000);
        assert!(provider.thinking_config.enabled);
        assert_eq!(provider.thinking_config.budget_tokens, Some(5000));
    }

    #[test]
    fn test_hermes_tool_call_parsing() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string());

        let text = r#"I'll help you read that file.
<tool_call>
{"name": "read_file", "arguments": {"path": "/home/user/test.txt"}}
</tool_call>"#;

        let calls = provider.parse_hermes_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[0].2["path"], "/home/user/test.txt");
    }

    #[test]
    fn test_multiple_hermes_tool_calls() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string());

        let text = r#"Let me read and then write.
<tool_call>
{"name": "read_file", "arguments": {"path": "input.txt"}}
</tool_call>
<tool_call>
{"name": "write_file", "arguments": {"path": "output.txt", "content": "done"}}
</tool_call>"#;

        let calls = provider.parse_hermes_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[1].1, "write_file");
    }

    #[test]
    fn test_thinking_extraction() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_thinking(true);

        let text = r#"<think>
The user wants me to analyze this code. Let me think about the best approach...
</think>
Here's my analysis of the code."#;

        let (thinking, remaining) = provider.extract_thinking(text);
        assert!(thinking.is_some());
        assert!(thinking.unwrap().contains("analyze this code"));
        assert!(remaining.contains("Here's my analysis"));
        assert!(!remaining.contains("<think>"));
    }

    #[test]
    fn test_supported_models() {
        let provider = QwenProvider::dashscope_intl("test-key".to_string());
        let models = provider.supported_models();
        assert!(models.contains(&"qwen3-8b".to_string()));
        assert!(models.contains(&"qwen2.5-coder-14b-instruct".to_string()));
        assert!(models.contains(&"qwen-max".to_string()));
    }

    #[test]
    fn test_context_window() {
        let provider = QwenProvider::dashscope_intl("test-key".to_string());
        assert_eq!(provider.context_window("qwen3-8b"), Some(131_072));
        assert_eq!(provider.context_window("qwen-max"), Some(32_768));
    }

    #[test]
    fn test_calculate_cost_local() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string());
        let cost = provider.calculate_cost("qwen3-8b", 1000, 1000);
        assert_eq!(cost, 0.0); // Local models are free
    }

    #[test]
    fn test_calculate_cost_cloud() {
        let provider = QwenProvider::dashscope_intl("test-key".to_string());
        let cost = provider.calculate_cost("qwen-turbo", 1_000_000, 1_000_000);
        // (1M * 0.3) + (1M * 0.6) = 0.3 + 0.6 = 0.9
        assert!((cost - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_custom_default_model() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_default_model("qwen2.5-coder-14b-instruct".to_string());
        assert_eq!(provider.default_model(), "qwen2.5-coder-14b-instruct");
    }

    #[test]
    fn test_hermes_tools_format() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string());

        let tools = vec![Tool {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        }];

        let formatted = provider.format_hermes_tools(&tools);
        assert!(formatted.contains("<tools>"));
        assert!(formatted.contains("</tools>"));
        assert!(formatted.contains("read_file"));
        assert!(formatted.contains("<tool_call>"));
    }

    #[test]
    fn test_native_qwen_parser_configuration() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);
        assert_eq!(provider.tool_parser, ToolCallParser::NativeQwen);
    }

    #[test]
    fn test_native_qwen_tool_call_parsing() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);

        let text = format!(
            "I'll read that file for you.\n{}: read_file\n{}: {{\"path\": \"/home/user/test.txt\"}}",
            FN_NAME, FN_ARGS
        );

        let calls = provider.parse_native_qwen_tool_calls(&text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[0].2["path"], "/home/user/test.txt");
    }

    #[test]
    fn test_multiple_native_qwen_tool_calls() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);

        let text = format!(
            "Let me read and write.\n{}: read_file\n{}: {{\"path\": \"input.txt\"}}\n{}: write_file\n{}: {{\"path\": \"output.txt\", \"content\": \"done\"}}",
            FN_NAME, FN_ARGS, FN_NAME, FN_ARGS
        );

        let calls = provider.parse_native_qwen_tool_calls(&text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[1].1, "write_file");
    }

    #[test]
    fn test_native_qwen_tools_format() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);

        let tools = vec![Tool {
            name: "bash".to_string(),
            description: "Execute shell commands".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                },
                "required": ["command"]
            }),
        }];

        let formatted = provider.format_native_qwen_tools(&tools);
        assert!(formatted.contains("<tool_info>"));
        assert!(formatted.contains("</tool_info>"));
        assert!(formatted.contains("bash"));
        assert!(formatted.contains(FN_NAME));
        assert!(formatted.contains(FN_ARGS));
    }

    #[test]
    fn test_native_qwen_result_format() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);

        let result = provider.format_native_qwen_result("File content here");
        assert!(result.contains(FN_RESULT));
        assert!(result.contains(FN_EXIT));
        assert!(result.contains("File content here"));
    }

    #[test]
    fn test_clean_incomplete_markers() {
        let provider = QwenProvider::local("http://localhost:8000/v1/chat/completions".to_string())
            .with_tool_parser(ToolCallParser::NativeQwen);

        // Test with incomplete marker at end
        let text = "Some text ✿FUN";
        let cleaned = provider.clean_incomplete_markers(text);
        assert_eq!(cleaned, "Some text ");

        // Test with complete text
        let text = "Complete text";
        let cleaned = provider.clean_incomplete_markers(text);
        assert_eq!(cleaned, "Complete text");
    }

    #[test]
    fn test_stop_words_defined() {
        // Verify stop words are correctly defined
        assert_eq!(QWEN_FN_STOP_WORDS.len(), 2);
        assert!(QWEN_FN_STOP_WORDS.contains(&"✿RESULT✿"));
        assert!(QWEN_FN_STOP_WORDS.contains(&"✿RETURN✿"));
    }
}
