//! Offline fixture-driven replay provider (#619).
//!
//! [`ReplayProvider`] implements the [`Provider`] trait by replaying an ordered
//! sequence of scripted assistant turns loaded from a JSON fixture. Each call to
//! `complete`/`stream` advances a cursor and returns the next turn, so the real
//! `tool_loop` runs deterministically with no network dependency. When the
//! script is exhausted it returns a terminal empty end-turn so the loop stops
//! rather than hanging.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;

use crate::brain::provider::{
    ContentBlock, ContentDelta, LLMRequest, LLMResponse, MessageDelta, Provider, ProviderStream,
    Result, Role, StopReason, StreamEvent, StreamMessage, TokenUsage,
};

/// A tool call in a scripted turn.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureToolCall {
    pub name: String,
    #[serde(default)]
    pub input: serde_json::Value,
    /// Optional stable id; a deterministic one is derived from the turn index
    /// when omitted.
    #[serde(default)]
    pub id: Option<String>,
}

/// One scripted assistant turn: optional narration text and/or a tool call.
/// A turn with a tool call stops with `ToolUse`; otherwise it ends the turn.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureTurn {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool: Option<FixtureToolCall>,
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

/// A full replay script: the model name to report and the ordered turns.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplayFixture {
    #[serde(default = "default_model")]
    pub model: String,
    pub turns: Vec<FixtureTurn>,
}

fn default_model() -> String {
    "replay-model".to_string()
}

impl ReplayFixture {
    /// Parse a fixture from a JSON string.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

/// Deterministic, offline provider that replays a [`ReplayFixture`].
pub struct ReplayProvider {
    model: String,
    turns: Vec<FixtureTurn>,
    cursor: Mutex<usize>,
}

impl ReplayProvider {
    /// Build a provider from a parsed fixture.
    pub fn new(fixture: ReplayFixture) -> Self {
        Self {
            model: fixture.model,
            turns: fixture.turns,
            cursor: Mutex::new(0),
        }
    }

    /// Convenience: build directly from fixture JSON.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        Ok(Self::new(ReplayFixture::from_json(json)?))
    }

    /// Number of turns already consumed. Useful for asserting the script was
    /// driven to completion.
    pub fn turns_consumed(&self) -> usize {
        *self.cursor.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Build the next `LLMResponse`, advancing the cursor. Past the end of the
    /// script it returns a terminal empty end-turn so callers never hang.
    fn next_response(&self) -> LLMResponse {
        let mut cursor = self.cursor.lock().unwrap_or_else(|e| e.into_inner());
        let idx = *cursor;
        let Some(turn) = self.turns.get(idx).cloned() else {
            return LLMResponse {
                id: format!("replay-exhausted-{idx}"),
                model: self.model.clone(),
                content: vec![ContentBlock::Text {
                    text: String::new(),
                }],
                stop_reason: Some(StopReason::EndTurn),
                usage: TokenUsage::default(),
                streaming_active_secs: None,
            };
        };
        *cursor += 1;
        drop(cursor);

        let mut content = Vec::new();
        if let Some(text) = turn.text.filter(|t| !t.is_empty()) {
            content.push(ContentBlock::Text { text });
        }
        let stop_reason = if let Some(tool) = turn.tool {
            let id = tool.id.unwrap_or_else(|| format!("replay-tool-{idx}"));
            content.push(ContentBlock::ToolUse {
                id,
                name: tool.name,
                input: tool.input,
            });
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };
        // Never emit an empty content vec — an end-turn with no text confuses
        // downstream block iteration.
        if content.is_empty() {
            content.push(ContentBlock::Text {
                text: String::new(),
            });
        }

        LLMResponse {
            id: format!("replay-{idx}"),
            model: self.model.clone(),
            content,
            stop_reason: Some(stop_reason),
            usage: TokenUsage {
                input_tokens: turn.input_tokens,
                output_tokens: turn.output_tokens,
                ..Default::default()
            },
            streaming_active_secs: None,
        }
    }
}

#[async_trait]
impl Provider for ReplayProvider {
    async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
        Ok(self.next_response())
    }

    async fn stream(&self, _request: LLMRequest) -> Result<ProviderStream> {
        let response = self.next_response();
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
        "replay"
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    fn supported_models(&self) -> Vec<String> {
        vec![self.model.clone()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(200_000)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}
