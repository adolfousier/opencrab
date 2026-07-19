//! Offline evaluation harness for context engineering and memory quality.
//!
//! The harness is deliberately offline and deterministic: it replays canned
//! LLM responses from fixtures ([`replay`]) so the real agent loop can be
//! exercised reproducibly, with no network, clock, or randomness. This is the
//! foundation the context and memory evals build on (umbrella issue #618).

pub mod baseline;
pub mod before_after;
pub mod compaction;
pub mod live;
pub mod manifest;
pub mod panel;
pub mod produce;
pub mod recall;
pub mod replay;
pub mod runner;
pub mod scorer;
pub mod self_awareness;
