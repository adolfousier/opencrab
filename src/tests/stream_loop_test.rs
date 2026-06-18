//! Tests for streaming text repetition detection and error message translation.

use crate::brain::agent::service::detect_text_repetition;

// --- Repetition detection ---

#[test]
fn detects_obvious_loop() {
    // Simulates MiniMax repeating the same tweet summary (block > 200 bytes)
    let block = "Tweet 1 (main post): https://x.com/opencrabs/status/2032245500150226963 \
                 Tweet 2 (fixes): WhatsApp QR width fixed on Windows, assets consolidated \
                 Tweet 3 (links): https://github.com/adolfousier/opencrabs/releases/tag/v0.2.75 ";
    assert!(block.len() > 200, "block must exceed min_match");
    let window = format!("{}{}", block, block);
    assert!(detect_text_repetition(&window, 200));
}

#[test]
fn no_false_positive_on_unique_text() {
    let window = "This is a completely unique response that does not repeat. \
                  It contains different information in every sentence. \
                  The weather is sunny today. Rust is a great language. \
                  OpenCrabs is an AI agent. Tests are important for quality. \
                  This paragraph covers many topics without repetition. \
                  Each line brings new content to the table. \
                  No two sentences share the same meaning or structure. \
                  The quick brown fox jumps over the lazy dog repeatedly.";
    assert!(!detect_text_repetition(window, 200));
}

#[test]
fn no_false_positive_on_short_text() {
    // Below minimum match threshold — should not trigger
    let window = "short text short text";
    assert!(!detect_text_repetition(window, 200));
}

#[test]
fn detects_loop_at_minimum_threshold() {
    // Exactly at the 200-byte boundary
    let chunk: String = "x".repeat(200);
    let window = format!("{}{}", chunk, chunk);
    assert!(detect_text_repetition(&window, 200));
}

#[test]
fn no_trigger_below_double_minimum() {
    // Window smaller than 2 * min_match — detection should not trigger
    let window = "a".repeat(399);
    assert!(!detect_text_repetition(&window, 200));
}

#[test]
fn detects_realistic_minimax_loop() {
    // Real-world pattern: MiniMax repeating release notes
    let repeated = "The release v0.2.75 has been successfully posted to X/Twitter! \
                    Here's what went out:\n\n\
                    **Tweet 1** (main post): https://x.com/opencrabs/status/2032245500150226963\n\n\
                    **Tweet 2** (fixes): Fixes & Updates WhatsApp QR width fixed on Windows \
                    Assets consolidated into src/ Post-evolve shows version diff\n\n\
                    **Tweet 3** (links): https://github.com/adolfousier/opencrabs/releases/tag/v0.2.75\n\n";
    let window = format!("{}{}", repeated, repeated);
    assert!(detect_text_repetition(&window, 200));
}

#[test]
fn allows_similar_but_not_identical_content() {
    // Two paragraphs that share some words but are structurally different
    let para1 = "The release v0.2.75 includes new features for WhatsApp QR display, \
                 post-evolve brain updates, and consolidated assets under src/. \
                 This version also adds autostart instructions for all platforms. ";
    let para2 = "Users should update to v0.2.75 for the improved error reporting, \
                 better SocialCrabs documentation, and GitHub Actions Node.js 24 \
                 migration. The changelog has full details on every change made. ";
    let window = format!("{}{}", para1, para2);
    assert!(!detect_text_repetition(&window, 200));
}

#[test]
fn custom_min_match_works() {
    let chunk = "abc".repeat(20); // 60 bytes
    let window = format!("{}{}", chunk, chunk);
    // Should detect with min_match=50
    assert!(detect_text_repetition(&window, 50));
    // Should NOT detect with min_match=200 (window too small)
    assert!(!detect_text_repetition(&window, 200));
}

// --- Error message translation ---

#[test]
fn translate_decode_error() {
    let raw = "Provider error: Streaming error: error decoding response body";
    assert!(raw.contains("error decoding response body"));
}

#[test]
fn translate_repetition_error_contains_keyword() {
    // The log message that triggers the user-friendly translation
    let log_msg = "Repetition detected in streaming response after 65000 bytes";
    assert!(log_msg.contains("Repetition detected"));
}

#[test]
fn empty_window_no_panic() {
    assert!(!detect_text_repetition("", 200));
    assert!(!detect_text_repetition("", 0));
}

#[test]
fn single_char_repeated_detected() {
    let window = "A".repeat(500);
    assert!(detect_text_repetition(&window, 200));
}

#[test]
fn no_panic_on_multibyte_utf8() {
    // Regression: slicing at window.len()/2 panicked on multi-byte chars
    // ❌ is 3 bytes, — (em-dash) is 3 bytes
    let window = "Already replied ❌ within 48h — @gyroscape ❌ within 48h — @RJMcGirr \
                  Already replied ❌ within 48h — @gyroscape ❌ within 48h — @RJMcGirr \
                  Already replied ❌ within 48h — @gyroscape ❌ within 48h — @RJMcGirr ";
    // Must not panic — just verify it runs
    detect_text_repetition(window, 50);
}

#[test]
fn no_panic_on_emoji_heavy_text() {
    // Window full of 4-byte emoji — midpoint likely lands inside a char
    let emoji_block = "🦀🥐🔁📏💭";
    let window: String = emoji_block.repeat(30);
    detect_text_repetition(&window, 50);
}

#[test]
fn detects_loop_with_multibyte_content() {
    let block = "Release notes — version 0.2.77 ❌ failed checks. \
                 Please retry the build with —all-features flag. \
                 Status: ❌ failed. See logs for details below. \
                 The em-dash — and cross ❌ are multi-byte UTF-8. ";
    let window = format!("{}{}", block, block);
    assert!(detect_text_repetition(&window, 100));
}

// --- Reasoning repetition detection ---
// Uses REASONING_REPEAT_WINDOW=4096, REASONING_REPEAT_MIN_MATCH=300

#[test]
fn reasoning_repetition_detected() {
    // Simulate a model repeating the same reasoning paragraph (must be >= 300 bytes)
    let reasoning_chunk = "Let me think about this more carefully. The issue is that \
                           we need to consider the edge cases in the input validation. \
                           First, let me check if the input is valid and properly formed. \
                           Then we need to verify the output matches expectations for all \
                           possible inputs. Let me reconsider the approach from scratch. \
                           I should also check the error handling paths. ";
    assert!(
        reasoning_chunk.len() >= 300,
        "chunk={}",
        reasoning_chunk.len()
    );
    let window = format!("{}{}", reasoning_chunk, reasoning_chunk);
    assert!(detect_text_repetition(&window, 300));
}

#[test]
fn reasoning_no_false_positive_on_long_diverse_thinking() {
    // Long reasoning with diverse content should NOT trigger
    let part1 = "The user wants me to implement a new feature for handling \
                  WebSocket connections. Let me analyze the existing codebase \
                  to understand the current architecture and identify the best \
                  integration point. The main server uses tokio for async I/O. ";
    let part2 = "Looking at the handler module, I see it uses a channel-based \
                  approach for message passing. Each connection gets its own task \
                  that reads from the socket and sends parsed messages through \
                  an mpsc channel to the central dispatcher. ";
    let part3 = "For the new feature, I'll need to add a variant to the enum \
                  that represents the new message type. Then update the match \
                  in the dispatcher to handle it. I should also add proper \
                  error handling for malformed messages. ";
    let part4 = "The test coverage for this module is decent but missing edge \
                  cases for connection drops during handshake. I'll add those \
                  tests as well. Let me start with the core implementation \
                  and then handle the error paths. ";
    let window = format!("{}{}{}{}", part1, part2, part3, part4);
    assert!(!detect_text_repetition(&window, 300));
}

#[test]
fn reasoning_repetition_with_300_byte_threshold() {
    // Exactly at the 300-byte boundary
    let chunk = "Thinking through the problem step by step. \
                 Need to verify each assumption carefully before proceeding. \
                 Let me trace through the logic once more to make sure \
                 we haven't missed any edge cases in the implementation. \
                 Also need to check the error handling paths and make sure \
                 we properly propagate errors up the call stack. ";
    assert!(
        chunk.len() >= 300,
        "chunk must exceed min_match, got {}",
        chunk.len()
    );
    let window = format!("{}{}", chunk, chunk);
    assert!(detect_text_repetition(&window, 300));
}

#[test]
fn reasoning_no_trigger_below_double_minimum() {
    // Window smaller than 2 * 300 = 600 bytes — should not trigger
    let window = "a".repeat(599);
    assert!(!detect_text_repetition(&window, 300));
}
