use super::*;
use crate::brain::provider::LLMRequest;
use crate::brain::provider::Message;
use crate::brain::provider::Role;

#[test]
fn test_message_creation() {
    let user_msg = Message::user("Hello");
    assert_eq!(user_msg.role, Role::User);
    assert_eq!(user_msg.content.len(), 1);

    let assistant_msg = Message::assistant("Hi there");
    assert_eq!(assistant_msg.role, Role::Assistant);
}

#[test]
fn test_llm_request_builder() {
    let request = LLMRequest::new("claude-3-sonnet-20240229", vec![Message::user("Test")])
        .with_system("You are helpful")
        .with_temperature(0.7)
        .with_max_tokens(1000)
        .with_streaming();

    assert_eq!(request.model, "claude-3-sonnet-20240229");
    assert!(request.system.is_some());
    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_tokens, Some(1000));
    assert!(request.stream);
}

#[test]
fn test_token_usage() {
    let usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 200,
        ..Default::default()
    };
    assert_eq!(usage.total(), 300);
}
