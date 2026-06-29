use super::*;

#[test]
fn split_short_text_returns_single_chunk() {
    let text = "Hello world, this is short.";
    let chunks = split_for_tts(text, 1500);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], text);
}

#[test]
fn split_long_text_breaks_at_sentences() {
    let text = "First sentence here. Second sentence here. Third sentence is much longer and goes on for a while to push us over the limit.";
    let chunks = split_for_tts(text, 50);
    assert!(chunks.len() > 1);
    // Each chunk should be under the limit
    for chunk in &chunks {
        assert!(chunk.len() <= 50, "chunk too long: {} chars", chunk.len());
    }
    // All content should be preserved
    let rejoined: String = chunks.join(" ");
    assert!(rejoined.contains("First sentence"));
    assert!(rejoined.contains("Second sentence"));
    assert!(rejoined.contains("Third sentence"));
}

#[test]
fn split_respects_question_and_exclamation() {
    let text = "Is this working? Yes it is! Great news indeed. Another sentence.";
    let chunks = split_for_tts(text, 30);
    assert!(chunks.len() > 1);
    for chunk in &chunks {
        assert!(chunk.len() <= 30, "chunk too long: {} chars", chunk.len());
    }
}

#[test]
fn split_falls_back_to_word_boundary() {
    // No sentence breaks, just one long sentence
    let text = "word ".repeat(100); // 500 chars, no periods
    let chunks = split_for_tts(&text, 100);
    assert!(chunks.len() > 1);
    for chunk in &chunks {
        assert!(chunk.len() <= 100, "chunk too long: {} chars", chunk.len());
    }
}

#[test]
fn find_sentence_break_finds_last_period() {
    assert_eq!(find_sentence_break("Hello. World"), Some(7)); // ". " is 2 chars
    assert_eq!(find_sentence_break("Hello! World"), Some(7));
    assert_eq!(find_sentence_break("Hello? World"), Some(7));
    assert_eq!(find_sentence_break("No break here"), None);
}

#[test]
fn split_preserves_newlines() {
    let text =
        "Paragraph one.\n\nParagraph two.\n\nParagraph three with more text to exceed limit.";
    let chunks = split_for_tts(text, 30);
    assert!(chunks.len() > 1);
}

#[test]
fn split_empty_text() {
    let chunks = split_for_tts("", 1500);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "");
}

#[test]
fn split_exactly_at_limit() {
    let text = "a".repeat(1500);
    let chunks = split_for_tts(&text, 1500);
    assert_eq!(chunks.len(), 1);
}

#[test]
fn split_one_over_limit() {
    let text = "a".repeat(1501);
    let chunks = split_for_tts(&text, 1500);
    assert_eq!(chunks.len(), 2);
}

#[test]
fn split_realistic_long_response() {
    // Simulate a long assistant response by repeating meaningful text
    let base = "Here is a detailed explanation of how Rust's borrow checker works. The borrow checker is a compile-time feature that ensures memory safety. It tracks the lifetime of references and ensures they don't outlive the data they point to. ";
    let text = base.repeat(10); // ~2500 chars
    let chunks = split_for_tts(&text, 1500);
    assert!(
        chunks.len() >= 2,
        "expected >= 2 chunks, got {}",
        chunks.len()
    );
    for chunk in &chunks {
        assert!(chunk.len() <= 1500, "chunk too long: {} chars", chunk.len());
        assert!(!chunk.trim().is_empty(), "empty chunk");
    }
}
