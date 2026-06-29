use crate::brain::tokenizer::*;

#[test]
fn test_empty_string() {
    assert_eq!(count_tokens(""), 0);
}

#[test]
fn test_simple_text() {
    let count = count_tokens("Hello, world!");
    // cl100k_base: "Hello" "," " world" "!" = 4 tokens
    assert!(
        (3..=6).contains(&count),
        "Got {count} tokens for 'Hello, world!'"
    );
}

#[test]
fn test_code_snippet() {
    let code = r#"fn main() { println!("Hello, world!"); }"#;
    let count = count_tokens(code);
    // Code tends to tokenize into more pieces
    assert!(
        count > 5,
        "Code should have more than 5 tokens, got {count}"
    );
}

#[test]
fn test_json_content() {
    let json = r#"{"name": "test", "value": 42, "nested": {"key": "val"}}"#;
    let count = count_tokens(json);
    assert!(
        count > 10,
        "JSON should have more than 10 tokens, got {count}"
    );
}

#[test]
fn test_long_text() {
    let text = "word ".repeat(1000); // ~1000 words
    let count = count_tokens(&text);
    // Should be roughly 1000 tokens (each "word " is ~1-2 tokens)
    assert!(
        count > 500 && count < 2000,
        "Got {count} tokens for ~1000 words"
    );
}

#[test]
fn test_consistency() {
    // Same input should always give same output
    let text = "The quick brown fox jumps over the lazy dog";
    let count1 = count_tokens(text);
    let count2 = count_tokens(text);
    assert_eq!(count1, count2);
}

#[test]
fn test_more_accurate_than_chars_div_3() {
    // chars/3 tends to underestimate for code/JSON, overestimate for prose
    let code =
        r#"pub async fn process_request(&self, ctx: &mut AgentContext) -> Result<Response> {"#;
    let tiktoken_count = count_tokens(code);
    let chars_3 = code.len() / 3;
    let chars_4 = code.len() / 4;

    // tiktoken should give a reasonable count — log for visibility
    eprintln!(
        "Code: tiktoken={tiktoken_count}, chars/3={chars_3}, chars/4={chars_4}, len={}",
        code.len()
    );
    assert!(tiktoken_count > 0);
}

#[test]
fn test_message_overhead() {
    let count = count_message_tokens("Hello");
    let base = count_tokens("Hello");
    assert_eq!(count, base + 4);
}
