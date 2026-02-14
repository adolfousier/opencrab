//! Agent Context Management
//!
//! Manages conversation context including messages, system prompts,
//! and token tracking.

use crate::db::models::Message as DbMessage;
use crate::llm::provider::{ContentBlock, Message, Role};
use std::path::PathBuf;
use uuid::Uuid;

/// Agent context for a conversation
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Session ID
    pub session_id: Uuid,

    /// System prompt
    pub system_prompt: Option<String>,

    /// Conversation messages
    pub messages: Vec<Message>,

    /// Tracked files in the conversation
    pub tracked_files: Vec<TrackedFile>,

    /// Current token count estimate
    pub token_count: usize,

    /// Maximum context tokens
    pub max_tokens: usize,
}

/// A file tracked in the conversation
#[derive(Debug, Clone)]
pub struct TrackedFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub content: Option<String>,
    pub token_count: usize,
}

impl AgentContext {
    /// Create a new agent context for a session
    pub fn new(session_id: Uuid, max_tokens: usize) -> Self {
        Self {
            session_id,
            system_prompt: None,
            messages: Vec::new(),
            tracked_files: Vec::new(),
            token_count: 0,
            max_tokens,
        }
    }

    /// Set the system prompt
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.token_count += Self::estimate_tokens(&prompt);
        self.system_prompt = Some(prompt);
        self
    }

    /// Add a message to the context
    pub fn add_message(&mut self, message: Message) {
        // Estimate tokens for the message
        let tokens = self.estimate_message_tokens(&message);
        self.token_count += tokens;
        self.messages.push(message);
    }

    /// Convert database messages to LLM messages
    pub fn from_db_messages(
        session_id: Uuid,
        db_messages: Vec<DbMessage>,
        max_tokens: usize,
    ) -> Self {
        let mut context = Self::new(session_id, max_tokens);

        for db_msg in db_messages {
            let role = match db_msg.role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::User, // Default fallback
            };

            let message = Message {
                role,
                content: vec![ContentBlock::Text {
                    text: db_msg.content,
                }],
            };

            context.add_message(message);
        }

        context
    }

    /// Track a file in the conversation
    pub fn track_file(&mut self, file: TrackedFile) {
        self.token_count += file.token_count;
        self.tracked_files.push(file);
    }

    /// Check if context would exceed limit with additional tokens
    pub fn would_exceed_limit(&self, additional_tokens: usize) -> bool {
        self.token_count + additional_tokens > self.max_tokens
    }

    /// Estimate tokens for a message
    fn estimate_message_tokens(&self, message: &Message) -> usize {
        let mut tokens = 0;

        for content in &message.content {
            match content {
                ContentBlock::Text { text } => {
                    tokens += Self::estimate_tokens(text);
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    tokens += Self::estimate_tokens(name);
                    tokens += Self::estimate_tokens(&input.to_string());
                }
                ContentBlock::ToolResult { content, .. } => {
                    tokens += Self::estimate_tokens(content);
                }
                ContentBlock::Image { .. } => {
                    // Images use a fixed token count (approximate)
                    tokens += 1000;
                }
            }
        }

        // Add overhead for message structure
        tokens + 4
    }

    /// Simple token estimation (roughly 4 characters per token)
    fn estimate_tokens(text: &str) -> usize {
        (text.len() / 4).max(1)
    }

    /// Get the current token usage percentage
    pub fn usage_percentage(&self) -> f64 {
        (self.token_count as f64 / self.max_tokens as f64) * 100.0
    }

    /// Trim old messages if context is too large
    pub fn trim_to_fit(&mut self, required_space: usize) {
        while self.would_exceed_limit(required_space) && !self.messages.is_empty() {
            // Remove the oldest user/assistant message pair
            if let Some(first_msg) = self.messages.first() {
                let tokens = self.estimate_message_tokens(first_msg);
                self.token_count = self.token_count.saturating_sub(tokens);
                self.messages.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let session_id = Uuid::new_v4();
        let context = AgentContext::new(session_id, 4096);

        assert_eq!(context.session_id, session_id);
        assert_eq!(context.max_tokens, 4096);
        assert_eq!(context.token_count, 0);
        assert!(context.messages.is_empty());
    }

    #[test]
    fn test_add_message() {
        let session_id = Uuid::new_v4();
        let mut context = AgentContext::new(session_id, 4096);

        let message = Message::user("Hello, how are you?");
        context.add_message(message);

        assert_eq!(context.messages.len(), 1);
        assert!(context.token_count > 0);
    }

    #[test]
    fn test_system_prompt() {
        let session_id = Uuid::new_v4();
        let context = AgentContext::new(session_id, 4096)
            .with_system_prompt("You are a helpful assistant.".to_string());

        assert!(context.system_prompt.is_some());
        assert!(context.token_count > 0);
    }

    #[test]
    fn test_token_estimation() {
        let tokens = AgentContext::estimate_tokens("Hello world");
        assert!(tokens > 0);
        assert!(tokens < 10); // Should be around 2-3 tokens
    }

    #[test]
    fn test_would_exceed_limit() {
        let session_id = Uuid::new_v4();
        let mut context = AgentContext::new(session_id, 100);

        let message = Message::user("Hello");
        context.add_message(message);

        assert!(!context.would_exceed_limit(10));
        assert!(context.would_exceed_limit(1000));
    }

    #[test]
    fn test_usage_percentage() {
        let session_id = Uuid::new_v4();
        let mut context = AgentContext::new(session_id, 100);

        // Add message that uses ~50 tokens
        let long_text = "a".repeat(200); // ~50 tokens
        let message = Message::user(long_text);
        context.add_message(message);

        let usage = context.usage_percentage();
        assert!(usage > 0.0 && usage <= 100.0);
    }

    #[test]
    fn test_trim_to_fit() {
        let session_id = Uuid::new_v4();
        let mut context = AgentContext::new(session_id, 100);

        // Add several messages with longer text to ensure they exceed limit
        for i in 0..5 {
            let long_text = format!("This is a longer message {} that will use more tokens to ensure we actually need to trim", i);
            let message = Message::user(long_text);
            context.add_message(message);
        }

        let original_count = context.messages.len();
        context.trim_to_fit(10); // Require 10 tokens space, forcing trimming

        // Should have removed some messages
        assert!(context.messages.len() < original_count);
    }
}
