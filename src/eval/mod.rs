//! Offline evaluation harness for context engineering and memory quality.
//!
//! The harness is deliberately offline and deterministic: it replays canned
//! LLM responses from fixtures ([`replay`]) so the real agent loop can be
//! exercised reproducibly, with no network, clock, or randomness. This is the
//! foundation the context and memory evals build on (umbrella issue #618).

pub mod compaction;
pub mod manifest;
pub mod replay;
pub mod scorer;
