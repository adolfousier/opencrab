//! Surface relevant memory without the model having to ask (#799).
//!
//! MEMORY.md was written constantly and read almost never. #800 made reading
//! cheap, but a cheap read still has to be chosen, and the case that hurts most
//! is the one where the model does not know there is anything to look up. It
//! cannot decide to recall a correction it has forgotten exists.
//!
//! So recall runs on the ENTRY path instead: the user's message is matched
//! against MEMORY.md at turn start and anything relevant rides along with it.
//! This mirrors `hints.rs`, which already proved the shape works, except that
//! fires on the error path after a tool call has already missed.
//!
//! Deliberately conservative. This is paid on every single turn and competes
//! with the actual task for attention, so it stays silent unless the match is
//! good, and it never grows past a couple of short sections.

use crate::brain::brain_sections;

/// At most this many sections ride along with a user message.
const RECALL_MAX_SECTIONS: usize = 2;
/// Total character budget for injected recall.
const RECALL_MAX_CHARS: usize = 1200;
/// A section must match at least this many DISTINCT terms from the message.
///
/// One shared word is noise: on a long message almost every section would
/// match something. Two terms co-occurring is a signal. This is the main guard
/// against recall becoming per-turn bloat, which is the failure mode that would
/// make it worse than the problem it solves.
const RECALL_MIN_HITS: usize = 2;

/// Recall relevant to `user_message`, formatted for context, or `None`.
///
/// Pure: takes the file content, so the decision is testable without a home
/// directory or a populated MEMORY.md.
pub fn recall_from(memory: &str, user_message: &str) -> Option<String> {
    // A system continuation is the harness talking to itself (restart recovery,
    // nudges). Recall belongs to what the USER asked.
    if user_message.starts_with("[System:") {
        return None;
    }

    let matches = brain_sections::find_sections_with(
        memory,
        user_message,
        RECALL_MAX_SECTIONS,
        RECALL_MAX_CHARS,
        RECALL_MIN_HITS,
    );
    if matches.sections.is_empty() {
        return None;
    }

    let body = matches
        .sections
        .iter()
        .map(brain_sections::Section::render)
        .collect::<Vec<_>>()
        .join("\n\n");

    // Labelled as recall, not as something the user said. Without the framing
    // the model can mistake injected memory for part of the request.
    Some(format!(
        "─── from your MEMORY.md, possibly relevant ───\n{body}\n\
         [Recalled automatically. Load MEMORY.md with a query for more.]"
    ))
}

/// Read MEMORY.md and recall against it. `None` when the file is absent,
/// unreadable, or nothing matched well enough.
pub fn recall_for(user_message: &str) -> Option<String> {
    let path = crate::config::opencrabs_home().join("MEMORY.md");
    let memory = std::fs::read_to_string(path).ok()?;
    recall_from(&memory, user_message)
}
