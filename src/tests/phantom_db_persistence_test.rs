//! Regression: phantom self-heal scaffolding must NOT land in the DB
//! `messages.content` column.
//!
//! Background: discussion #86 / gist 85cfdc26 showed a Telegram session
//! with 34 phantom entries piled into the agent's history. Each new turn
//! reloaded the DB row, the phantom narrations re-entered LLM context as
//! assistant history, and the model hallucinated more phantoms from its
//! own corrections.
//!
//! Commit `c7814618` ("stop injecting phantom text into context") fixed
//! the in-memory `ConversationContext` for the live turn but left the
//! per-iteration `append_content` call in `tool_loop.rs:2589-2600`
//! unconditional, so phantom text still hit the DB. This file pins the
//! follow-up fix: phantom iterations skip the persist; the eventual
//! successful iteration (or `[self-heal] Aborted —…` notice) gets
//! persisted normally.

use crate::brain::agent::service::AgentService;
use crate::brain::provider::{
    ContentBlock, ContentDelta, LLMRequest, LLMResponse, MessageDelta, Provider, ProviderStream,
    Role, StopReason, StreamEvent, StreamMessage, TokenUsage,
};
use crate::db::Database;
use crate::services::{MessageService, ServiceContext, SessionService};
use async_trait::async_trait;
use std::sync::Arc;

/// First call: phantom narration (`Let me check git status…`) with zero
/// tool_use blocks. Second call: legitimate clean response.
struct PhantomThenRealProvider {
    call_count: std::sync::Mutex<usize>,
    phantom_text: String,
    real_text: String,
}

impl PhantomThenRealProvider {
    fn new(phantom_text: &str, real_text: &str) -> Self {
        Self {
            call_count: std::sync::Mutex::new(0),
            phantom_text: phantom_text.to_string(),
            real_text: real_text.to_string(),
        }
    }
}

#[async_trait]
impl Provider for PhantomThenRealProvider {
    fn name(&self) -> &str {
        "phantom-mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn complete(&self, _req: LLMRequest) -> crate::brain::provider::Result<LLMResponse> {
        let mut n = self.call_count.lock().unwrap();
        *n += 1;
        let text = if *n == 1 {
            self.phantom_text.clone()
        } else {
            self.real_text.clone()
        };
        Ok(LLMResponse {
            id: format!("phantom-mock-{n}"),
            model: "mock-model".to_string(),
            content: vec![ContentBlock::Text { text }],
            stop_reason: Some(StopReason::EndTurn),
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

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(8192)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

#[tokio::test]
async fn phantom_iteration_not_persisted_to_assistant_db_row() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());

    // Phantom narration matches an `INTENT_PHRASES` line-start so
    // `has_phantom_tool_intent_no_tools` returns true on iter 1.
    // Iter 2 returns a clean factual response with no phantom signatures.
    let provider = Arc::new(PhantomThenRealProvider::new(
        "Let me check the git status for you.",
        "Repo is clean, nothing to commit.",
    ));
    let agent_service = AgentService::new_for_test(provider, context.clone()).await;

    let session_service = SessionService::new(context.clone());
    let session = session_service
        .create_session(Some("Phantom DB Persist Test".to_string()))
        .await
        .unwrap();

    let response = agent_service
        .send_message_with_tools(session.id, "What is the git status?".to_string(), None)
        .await
        .unwrap();

    // The live response reflects the FINAL provider call (the legitimate
    // one), not the phantom — `send_message_with_tools` returns
    // `AgentResponse { content: String, ... }`.
    assert!(
        response.content.contains("Repo is clean"),
        "final response must come from iter 2: {:?}",
        response.content
    );

    // Now inspect the assistant DB row. This is what gets reloaded as
    // history on the next turn and on session reconnect.
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
        !assistant.content.contains("Let me check the git status"),
        "phantom narration leaked into DB → next-turn LLM context would see it.\n\
         Full content:\n{}",
        assistant.content
    );
    assert!(
        assistant.content.contains("Repo is clean"),
        "final clean response missing from DB:\n{}",
        assistant.content
    );
}

/// Sanity check: when iter 1 is NOT phantom, the iteration text is
/// persisted normally. Pins that the phantom-skip rule only fires on
/// real phantoms — legitimate text iterations stay in the DB.
#[tokio::test]
async fn non_phantom_iteration_persists_normally() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());

    // No `Let me` / `I'll` line-start → not phantom.
    let provider = Arc::new(PhantomThenRealProvider::new(
        "Sure — the repository is currently up to date.",
        "ignored-second-call",
    ));
    let agent_service = AgentService::new_for_test(provider, context.clone()).await;

    let session_service = SessionService::new(context.clone());
    let session = session_service
        .create_session(Some("Non-Phantom Persist Test".to_string()))
        .await
        .unwrap();

    agent_service
        .send_message_with_tools(session.id, "Status?".to_string(), None)
        .await
        .unwrap();

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
        assistant
            .content
            .contains("Sure — the repository is currently up to date"),
        "non-phantom text must be persisted:\n{}",
        assistant.content
    );
}
