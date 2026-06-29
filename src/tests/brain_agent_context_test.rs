use crate::brain::agent::context::*;
use crate::brain::provider::ContentBlock;
use crate::brain::provider::Message;
use uuid::Uuid;

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
fn test_system_brain() {
    let session_id = Uuid::new_v4();
    let context = AgentContext::new(session_id, 4096)
        .with_system_brain("You are a helpful assistant.".to_string());

    assert!(context.system_brain.is_some());
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
        let long_text = format!(
            "This is a longer message {} that will use more tokens to ensure we actually need to trim",
            i
        );
        let message = Message::user(long_text);
        context.add_message(message);
    }

    let original_count = context.messages.len();
    context.trim_to_fit(10); // Require 10 tokens space, forcing trimming

    // Should have removed some messages
    assert!(context.messages.len() < original_count);
}

#[test]
fn test_compact_with_summary_keeps_recent() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 10000);

    // Add 10 messages
    for i in 0..10 {
        context.add_message(Message::user(format!("Message {}", i)));
    }
    assert_eq!(context.messages.len(), 10);

    // Use 80% of max_tokens as budget — should keep all short messages
    let budget = (10000.0 * 0.80) as usize;
    context.compact_with_summary("Summary of old messages".to_string(), budget);

    // First message should be the compaction summary
    if let Some(ContentBlock::Text { text }) = context.messages[0].content.first() {
        assert!(text.contains("CONTEXT COMPACTION"));
        assert!(text.contains("Summary of old messages"));
    } else {
        panic!("First message should be a text compaction summary");
    }

    // Last kept message should be Message 9
    if let Some(ContentBlock::Text { text }) = context.messages.last().unwrap().content.first() {
        assert!(text.contains("Message 9"));
    } else {
        panic!("Last message should be Message 9");
    }
}

#[test]
fn test_compact_with_summary_recalculates_tokens() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 10000);

    // Add many large messages to build up token count
    for i in 0..20 {
        let big_text = format!("Large message {} {}", i, "x".repeat(400));
        context.add_message(Message::user(big_text));
    }

    let tokens_before = context.token_count;
    assert!(tokens_before > 0);

    // Very small budget — should drop most messages
    context.compact_with_summary("Brief summary".to_string(), 500);

    // Token count should be recalculated and less than before
    assert!(context.token_count < tokens_before);
    assert!(context.token_count > 0);
}

#[test]
fn test_compact_with_summary_fewer_messages_than_keep() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 10000);

    // Add only 2 messages — large budget should keep them all
    context.add_message(Message::user("Hello".to_string()));
    context.add_message(Message::user("World".to_string()));

    context.compact_with_summary("Summary".to_string(), 8000);

    // Should have: 1 summary + 2 original = 3
    assert_eq!(context.messages.len(), 3);
}

#[test]
fn test_compact_with_summary_preserves_system_brain_tokens() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 10000)
        .with_system_brain("You are an AI assistant".to_string());

    for i in 0..5 {
        context.add_message(Message::user(format!("Msg {}", i)));
    }

    context.compact_with_summary("Summary".to_string(), 500);

    // Token count should include system brain tokens
    let brain_tokens = AgentContext::estimate_tokens("You are an AI assistant");
    assert!(context.token_count >= brain_tokens);
}

#[test]
fn test_compact_empty_context() {
    let session_id = Uuid::new_v4();
    let mut context = AgentContext::new(session_id, 10000);

    // No messages, compact should still work
    context.compact_with_summary("Summary of nothing".to_string(), 8000);

    // Should have just the summary message
    assert_eq!(context.messages.len(), 1);
}
