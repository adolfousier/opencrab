//! Token counting using tiktoken (cl100k_base encoding).
//!
//! Uses OpenAI's cl100k_base BPE tokenizer for accurate token estimation.
//! While Anthropic uses their own tokenizer internally, cl100k_base provides
//! a much closer approximation than chars/N heuristics (~5-10% variance vs ~30-50%).
//!
//! The tokenizer is initialized lazily via `once_cell` and reused across all calls.

use once_cell::sync::Lazy;
use tiktoken_rs::CoreBPE;

/// Global tokenizer instance — initialized once, reused everywhere.
/// cl100k_base is used by GPT-4, GPT-3.5-turbo, and text-embedding-ada-002.
/// It's the closest publicly available tokenizer to what Anthropic uses.
static TOKENIZER: Lazy<CoreBPE> =
    Lazy::new(|| tiktoken_rs::cl100k_base().expect("Failed to initialize cl100k_base tokenizer"));

/// Count tokens in a string using cl100k_base BPE encoding.
///
/// This is the single source of truth for token estimation across the entire
/// codebase. No more chars/3, chars/4, or any other heuristic.
///
/// # Returns
/// Actual BPE token count (minimum 1 for non-empty strings, 0 for empty).
pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let tokens = TOKENIZER.encode_ordinary(text);
    tokens.len().max(1)
}

/// Count tokens for a message with structural overhead.
///
/// Each message has ~4 tokens of overhead for role tags and separators.
pub fn count_message_tokens(text: &str) -> usize {
    count_tokens(text) + 4
}
