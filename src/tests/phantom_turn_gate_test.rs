//! Regression: phantom detector must NOT fire on a clean text-only
//! confirmation that follows a turn of real tool execution.
//!
//! Background: a session ran `git push` (and several other tools)
//! successfully and the model wrapped up with "Pushed (25a2da5).
//! Favicon replaced, deploy complete." The phantom detector saw a
//! past-tense action claim ("Pushed") with zero tool_use blocks in
//! that final iteration and replaced the wrap-up with a self-heal
//! abort notice — a false positive on a turn that had already
//! completed correctly.
//!
//! Fix: track `tools_executed_this_turn` in the tool loop and skip
//! every phantom branch once any tool has run in the current turn.
//! This file pins the behavior.
//!
//! Mocks:
//! - Provider call 1 → returns ContentBlock::ToolUse for "test_tool"
//! - Provider call 2 → returns past-tense text "Pushed (abc123). All done."
//!   (this text matches `has_past_tense_action_claim` and would fire
//!   the phantom detector if the turn-gate were absent)

use crate::brain::agent::service::AgentService;
use crate::brain::provider::{
    ContentBlock, ContentDelta, LLMRequest, LLMResponse, MessageDelta, Provider, ProviderStream,
    Role, StopReason, StreamEvent, StreamMessage, TokenUsage,
};
use crate::brain::tools::ToolRegistry;
use crate::db::Database;
use crate::services::{MessageService, ServiceContext, SessionService};
use crate::tests::agent_service_mocks::MockTool;
use async_trait::async_trait;
use std::sync::Arc;

struct ToolThenPastTenseProvider {
    call_count: std::sync::Mutex<usize>,
}

impl ToolThenPastTenseProvider {
    fn new() -> Self {
        Self {
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait]
impl Provider for ToolThenPastTenseProvider {
    fn name(&self) -> &str {
        "tool-then-past-tense"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(8192)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }

    async fn complete(&self, _req: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        let mut n = self.call_count.lock().unwrap();
        *n += 1;
        let (content, stop) = if *n == 1 {
            (
                vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "test_tool".to_string(),
                    input: serde_json::json!({"message": "hi"}),
                }],
                StopReason::ToolUse,
            )
        } else {
            (
                vec![ContentBlock::Text {
                    text: "Pushed (abc123). Favicon replaced, deploy complete.".to_string(),
                }],
                StopReason::EndTurn,
            )
        };
        Ok(LLMResponse {
            id: format!("turn-gate-{n}"),
            model: "mock-model".to_string(),
            content,
            stop_reason: Some(stop),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                ..Default::default()
            },
        })
    }

    async fn stream(&self, req: LLMRequest) -> crate::brain::provider::Result<ProviderStream> {
        let response = self.complete(req).await?;
        let model = response.model.clone();
        let mut events = vec![Ok(StreamEvent::MessageStart {
            message: StreamMessage {
                id: response.id.clone(),
                model: model.clone(),
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
}

#[tokio::test]
async fn phantom_skipped_after_successful_tool_execution() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());

    let provider = Arc::new(ToolThenPastTenseProvider::new());
    let registry = ToolRegistry::new();
    registry.register(Arc::new(MockTool));

    let agent_service = AgentService::new_for_test(provider, context.clone())
        .await
        .with_tool_registry(Arc::new(registry))
        .with_auto_approve_tools(true);

    let session_service = SessionService::new(context.clone());
    let session = session_service
        .create_session(Some("Phantom Turn Gate Test".to_string()))
        .await
        .unwrap();

    let response = agent_service
        .send_message_with_tools(session.id, "Push the favicon fix.".to_string(), None)
        .await
        .unwrap();

    // The final response should be the model's clean wrap-up, NOT the
    // phantom self-heal notice. Without the turn-level gate the
    // `has_past_tense_action_claim` match on "Pushed" would replace
    // this text with `[self-heal] Aborted —…`.
    assert!(
        response.content.contains("Pushed (abc123)"),
        "final response must preserve the model's wrap-up text after tool execution. \
         Got: {:?}",
        response.content
    );
    assert!(
        !response.content.contains("[self-heal]"),
        "phantom detector mis-fired on legitimate tool-success confirmation. \
         Got: {:?}",
        response.content
    );

    let message_service = MessageService::new(context);
    let messages = message_service
        .list_messages_for_session(session.id)
        .await
        .unwrap();
    let assistant = messages
        .iter()
        .find(|m| m.role == "assistant")
        .expect("at least one assistant message");
    assert!(
        !assistant.content.contains("[self-heal]"),
        "phantom abort leaked into DB row:\n{}",
        assistant.content
    );
}
