//! Shared mock providers, mock tools, and helpers for agent-service tests.
//!
//! Moved out of the inline `src/brain/agent/service/tests/` module so all tests
//! live under `src/tests/`. Items are `pub(crate)` so the standalone test
//! modules can import them.

use crate::brain::agent::service::AgentService;
use crate::brain::provider::{
    ContentBlock, LLMRequest, LLMResponse, Provider, ProviderStream, Role, StopReason, TokenUsage,
};
use crate::db::Database;
use crate::services::{ServiceContext, SessionService};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

/// Mock provider for testing — returns simple text-only responses
pub(crate) struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn complete(&self, _request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        Ok(LLMResponse {
            id: "test-response-1".to_string(),
            model: "mock-model".to_string(),
            content: vec![ContentBlock::Text {
                text: "This is a test response".to_string(),
            }],
            stop_reason: Some(StopReason::EndTurn),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                ..Default::default()
            },
            streaming_active_secs: None,
            tool_text_leak: false,
        })
    }

    async fn stream(&self, request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        use crate::brain::provider::{ContentDelta, MessageDelta, StreamEvent, StreamMessage};

        let response = self.complete(request).await?;
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: response.model.clone(),
                role: Role::Assistant,
                usage: response.usage,
            },
        })];
        for (i, block) in response.content.iter().enumerate() {
            if let ContentBlock::Text { text } = block {
                events.push(Ok(StreamEvent::ContentBlockStart {
                    index: i,
                    content_block: ContentBlock::Text {
                        text: String::new(),
                    },
                }));
                events.push(Ok(StreamEvent::ContentBlockDelta {
                    index: i,
                    delta: ContentDelta::TextDelta { text: text.clone() },
                }));
                events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
            }
        }
        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: response.stop_reason,
                stop_sequence: None,
            },
            usage: response.usage,
        }));
        events.push(Ok(StreamEvent::MessageStop));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.001
    }
}

/// Mock provider that simulates tool use — first call returns tool_use, second returns text
pub(crate) struct MockProviderWithTools {
    call_count: std::sync::Mutex<usize>,
}

impl MockProviderWithTools {
    pub(crate) fn new() -> Self {
        Self {
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait]
impl Provider for MockProviderWithTools {
    async fn complete(&self, _request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let call_num = *count;

        if call_num == 1 {
            Ok(LLMResponse {
                id: "test-response-1".to_string(),
                model: "mock-model".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text: "I'll use the test tool.".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "test_tool".to_string(),
                        input: serde_json::json!({"message": "test"}),
                    },
                ],
                stop_reason: Some(StopReason::ToolUse),
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        } else {
            Ok(LLMResponse {
                id: "test-response-2".to_string(),
                model: "mock-model".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Tool execution completed successfully.".to_string(),
                }],
                stop_reason: Some(StopReason::EndTurn),
                usage: TokenUsage {
                    input_tokens: 15,
                    output_tokens: 25,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        }
    }

    async fn stream(&self, request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        use crate::brain::provider::{ContentDelta, MessageDelta, StreamEvent, StreamMessage};

        let response = self.complete(request).await?;
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: response.model.clone(),
                role: Role::Assistant,
                usage: response.usage,
            },
        })];

        for (i, block) in response.content.iter().enumerate() {
            match block {
                ContentBlock::Text { text } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::Text {
                            text: String::new(),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::TextDelta { text: text.clone() },
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::Value::Object(Default::default()),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::InputJsonDelta {
                            partial_json: serde_json::to_string(input).unwrap_or_default(),
                        },
                    }));
                }
                _ => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: block.clone(),
                    }));
                }
            }
            events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
        }

        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: response.stop_reason,
                stop_sequence: None,
            },
            usage: response.usage,
        }));
        events.push(Ok(StreamEvent::MessageStop));

        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn name(&self) -> &str {
        "mock-with-tools"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.001
    }
}

/// Mock tool that always succeeds, does NOT require approval
pub(crate) struct MockTool;

#[async_trait]
impl crate::brain::tools::Tool for MockTool {
    fn name(&self) -> &str {
        "test_tool"
    }

    fn description(&self) -> &str {
        "A test tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            }
        })
    }

    fn capabilities(&self) -> Vec<crate::brain::tools::ToolCapability> {
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &crate::brain::tools::ToolExecutionContext,
    ) -> crate::brain::tools::Result<crate::brain::tools::ToolResult> {
        Ok(crate::brain::tools::ToolResult::success(
            "Tool executed successfully".to_string(),
        ))
    }
}

/// Mock tool that requires approval before execution
pub(crate) struct MockToolRequiresApproval;

#[async_trait]
impl crate::brain::tools::Tool for MockToolRequiresApproval {
    fn name(&self) -> &str {
        "approval_tool"
    }

    fn description(&self) -> &str {
        "A tool that requires approval"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string"}
            }
        })
    }

    fn capabilities(&self) -> Vec<crate::brain::tools::ToolCapability> {
        vec![crate::brain::tools::ToolCapability::ExecuteShell]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &crate::brain::tools::ToolExecutionContext,
    ) -> crate::brain::tools::Result<crate::brain::tools::ToolResult> {
        Ok(crate::brain::tools::ToolResult::success(
            "Approval tool executed".to_string(),
        ))
    }
}

/// Mock provider with configurable name and model — tracks requested model in responses
pub(crate) struct MockProviderWithModel {
    provider_name: String,
    model_name: String,
}

impl MockProviderWithModel {
    pub(crate) fn new(provider_name: &str, model_name: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            model_name: model_name.to_string(),
        }
    }
}

#[async_trait]
impl Provider for MockProviderWithModel {
    async fn complete(&self, request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        // Use the model from the request (what the caller asked for), falling back to our default
        let model = if request.model.is_empty() {
            self.model_name.clone()
        } else {
            request.model.clone()
        };

        Ok(LLMResponse {
            id: format!("resp-{}", self.provider_name),
            model,
            content: vec![ContentBlock::Text {
                text: format!("Response from {}", self.provider_name),
            }],
            stop_reason: Some(StopReason::EndTurn),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                ..Default::default()
            },
            streaming_active_secs: None,
            tool_text_leak: false,
        })
    }

    async fn stream(&self, request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        use crate::brain::provider::{ContentDelta, MessageDelta, StreamEvent, StreamMessage};

        let response = self.complete(request).await?;
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: response.model.clone(),
                role: Role::Assistant,
                usage: response.usage,
            },
        })];
        for (i, block) in response.content.iter().enumerate() {
            if let ContentBlock::Text { text } = block {
                events.push(Ok(StreamEvent::ContentBlockStart {
                    index: i,
                    content_block: ContentBlock::Text {
                        text: String::new(),
                    },
                }));
                events.push(Ok(StreamEvent::ContentBlockDelta {
                    index: i,
                    delta: ContentDelta::TextDelta { text: text.clone() },
                }));
                events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
            }
        }
        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: response.stop_reason,
                stop_sequence: None,
            },
            usage: response.usage,
        }));
        events.push(Ok(StreamEvent::MessageStop));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn name(&self) -> &str {
        &self.provider_name
    }

    fn default_model(&self) -> &str {
        &self.model_name
    }

    fn supported_models(&self) -> Vec<String> {
        vec![self.model_name.clone()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.001
    }
}

/// Mock provider that returns tool calls for a named tool (configurable)
pub(crate) struct MockProviderWithNamedTool {
    tool_name: String,
    call_count: std::sync::Mutex<usize>,
}

impl MockProviderWithNamedTool {
    pub(crate) fn new(tool_name: &str) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait]
impl Provider for MockProviderWithNamedTool {
    async fn complete(&self, _request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let call_num = *count;

        if call_num == 1 {
            Ok(LLMResponse {
                id: "test-response-1".to_string(),
                model: "mock-model".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text: format!("Using tool {}", self.tool_name),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-call-1".to_string(),
                        name: self.tool_name.clone(),
                        input: serde_json::json!({"action": "test"}),
                    },
                ],
                stop_reason: Some(StopReason::ToolUse),
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        } else {
            Ok(LLMResponse {
                id: "test-response-2".to_string(),
                model: "mock-model".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Done.".to_string(),
                }],
                stop_reason: Some(StopReason::EndTurn),
                usage: TokenUsage {
                    input_tokens: 15,
                    output_tokens: 25,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        }
    }

    async fn stream(&self, request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        use crate::brain::provider::{ContentDelta, MessageDelta, StreamEvent, StreamMessage};

        let response = self.complete(request).await?;
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: response.model.clone(),
                role: Role::Assistant,
                usage: response.usage,
            },
        })];
        for (i, block) in response.content.iter().enumerate() {
            match block {
                ContentBlock::Text { text } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::Text {
                            text: String::new(),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::TextDelta { text: text.clone() },
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::Value::Object(Default::default()),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::InputJsonDelta {
                            partial_json: serde_json::to_string(input).unwrap_or_default(),
                        },
                    }));
                }
                _ => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: block.clone(),
                    }));
                }
            }
            events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
        }
        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: response.stop_reason,
                stop_sequence: None,
            },
            usage: response.usage,
        }));
        events.push(Ok(StreamEvent::MessageStop));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn name(&self) -> &str {
        "mock-named-tool-provider"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.001
    }
}

/// Mock provider that returns two tool calls in a single response (for mixed approval tests)
pub(crate) struct MockProviderWithTwoToolCalls {
    tool_a: String,
    tool_b: String,
    call_count: std::sync::Mutex<usize>,
}

impl MockProviderWithTwoToolCalls {
    pub(crate) fn new(tool_a: &str, tool_b: &str) -> Self {
        Self {
            tool_a: tool_a.to_string(),
            tool_b: tool_b.to_string(),
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait]
impl Provider for MockProviderWithTwoToolCalls {
    async fn complete(&self, _request: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let call_num = *count;

        if call_num == 1 {
            Ok(LLMResponse {
                id: "test-response-1".to_string(),
                model: "mock-model".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text: "Using both tools".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-call-a".to_string(),
                        name: self.tool_a.clone(),
                        input: serde_json::json!({"action": "a"}),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-call-b".to_string(),
                        name: self.tool_b.clone(),
                        input: serde_json::json!({"message": "b"}),
                    },
                ],
                stop_reason: Some(StopReason::ToolUse),
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        } else {
            Ok(LLMResponse {
                id: "test-response-2".to_string(),
                model: "mock-model".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Both tools done.".to_string(),
                }],
                stop_reason: Some(StopReason::EndTurn),
                usage: TokenUsage {
                    input_tokens: 15,
                    output_tokens: 25,
                    ..Default::default()
                },
                streaming_active_secs: None,
                tool_text_leak: false,
            })
        }
    }

    async fn stream(&self, request: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        use crate::brain::provider::{ContentDelta, MessageDelta, StreamEvent, StreamMessage};

        let response = self.complete(request).await?;
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: response.model.clone(),
                role: Role::Assistant,
                usage: response.usage,
            },
        })];
        for (i, block) in response.content.iter().enumerate() {
            match block {
                ContentBlock::Text { text } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::Text {
                            text: String::new(),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::TextDelta { text: text.clone() },
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::Value::Object(Default::default()),
                        },
                    }));
                    events.push(Ok(StreamEvent::ContentBlockDelta {
                        index: i,
                        delta: ContentDelta::InputJsonDelta {
                            partial_json: serde_json::to_string(input).unwrap_or_default(),
                        },
                    }));
                }
                _ => {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: block.clone(),
                    }));
                }
            }
            events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
        }
        events.push(Ok(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: response.stop_reason,
                stop_sequence: None,
            },
            usage: response.usage,
        }));
        events.push(Ok(StreamEvent::MessageStop));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn name(&self) -> &str {
        "mock-two-tools"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.001
    }
}

// === Shared helpers ===

#[allow(dead_code)]
pub(crate) async fn create_test_service() -> (AgentService, Uuid) {
    create_test_service_with_provider(Arc::new(MockProvider)).await
}

/// Like [`create_test_service`] but also hands back the `SessionService` bound
/// to the same context, for tests that drive `handle_command` (which needs it).
#[allow(dead_code)]
pub(crate) async fn create_test_service_full() -> (AgentService, SessionService, Uuid) {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let agent_service = AgentService::new_for_test(Arc::new(MockProvider), context.clone()).await;
    let session_service = SessionService::new(context);
    let session = session_service
        .create_session(Some("Test Session".to_string()))
        .await
        .unwrap();
    (agent_service, session_service, session.id)
}

#[allow(dead_code)]
pub(crate) async fn create_test_service_with_provider(
    provider: Arc<dyn Provider>,
) -> (AgentService, Uuid) {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();

    let context = ServiceContext::new(pool);
    let agent_service = AgentService::new_for_test(provider, context.clone()).await;

    let session_service = SessionService::new(context);
    let session = session_service
        .create_session(Some("Test Session".to_string()))
        .await
        .unwrap();

    (agent_service, session.id)
}
