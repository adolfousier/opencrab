//! Cancelling a turn (Esc-Esc) must not lose the completion for HTTP/streaming
//! providers (#1468).
//!
//! The real-time live persister (#269) that writes streamed text to the DB as
//! it arrives is gated `if is_cli_provider`. For HTTP providers the assistant
//! row stays an empty placeholder until turn-end, so a cancel before then left
//! the row 0 chars, and `load_session` (called right after the abort to re-inline
//! tool calls) rebuilt the view from that empty row and wiped the in-memory copy
//! too: the whole visible completion vanished from screen AND disk. Confirmed on
//! `modelscope-qwen38max` with 0-length assistant rows in the live DB and abort
//! log lines carrying no "Persisted N chars" (the `content.is_empty()` early
//! return).
//!
//! `persist_streaming_state` now reconstructs the turn's assistant text from
//! `self.messages` via `collect_turn_assistant_text` and writes it when the DB
//! row is still empty. These pin that reconstruction, the novel logic. It is
//! pure over the message slice, so it needs no live terminal or DB.

use crate::tui::app::DisplayMessage;
use crate::tui::app::messaging::collect_turn_assistant_text;

fn dm(role: &str, content: &str) -> DisplayMessage {
    DisplayMessage {
        id: uuid::Uuid::new_v4(),
        role: role.to_string(),
        content: content.to_string(),
        timestamp: chrono::Utc::now(),
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: None,
        expanded: false,
        expanded_full: false,
        tool_group: None,
        duration_secs: None,
    }
}

#[test]
fn collects_assistant_text_after_the_last_user_message() {
    // A turn that streamed text, ran a tool, then streamed more: the visible
    // completion is the two assistant entries after the last user message.
    let msgs = vec![
        dm("user", "earlier question"),
        dm("assistant", "earlier answer"),
        dm("user", "the question being answered now"),
        dm("assistant", "first half of the completion"),
        dm("tool_group", "3 tool calls"),
        dm("assistant", "second half of the completion"),
    ];
    let text = collect_turn_assistant_text(&msgs);
    assert_eq!(
        text, "first half of the completion\n\nsecond half of the completion",
        "both assistant fragments of the in-flight turn must be reconstructed \
         in order, joined the way the DB row stores them"
    );
}

#[test]
fn never_pulls_text_from_a_previous_turn() {
    // The blast radius guard: if the reconstruction reached past the last user
    // message it would re-persist settled answers and duplicate history on
    // every cancel.
    let msgs = vec![
        dm("user", "q1"),
        dm("assistant", "answer to q1"),
        dm("user", "q2"),
        dm("assistant", "answer to q2"),
    ];
    let text = collect_turn_assistant_text(&msgs);
    assert_eq!(text, "answer to q2");
    assert!(
        !text.contains("answer to q1"),
        "text before the last user message belongs to a settled turn that is \
         already durable; re-persisting it would duplicate history"
    );
}

#[test]
fn empty_when_the_turn_produced_no_visible_text() {
    // Cancelled before anything streamed: nothing to save, and persist must
    // stay a no-op rather than write an empty row.
    let msgs = vec![dm("user", "a question cancelled before any output")];
    assert_eq!(collect_turn_assistant_text(&msgs), "");
}

#[test]
fn skips_blank_assistant_entries() {
    // Placeholders and whitespace-only fragments must not introduce separators
    // or empty content into the reconstructed text.
    let msgs = vec![
        dm("user", "q"),
        dm("assistant", "   "),
        dm("assistant", "real content"),
        dm("assistant", ""),
    ];
    assert_eq!(collect_turn_assistant_text(&msgs), "real content");
}

#[test]
fn collects_everything_when_there_is_no_user_message() {
    // Defensive: a transcript with no user row (e.g. a resumed or compacted
    // session) still reconstructs from the top rather than dropping the turn.
    let msgs = vec![
        dm("assistant", "continuation output"),
        dm("tool_group", "1 tool call"),
        dm("assistant", "more output"),
    ];
    assert_eq!(
        collect_turn_assistant_text(&msgs),
        "continuation output\n\nmore output"
    );
}

#[test]
fn ignores_tool_group_entries() {
    // Tool groups persist through their own marker path; the reconstruction is
    // for assistant prose only, so tool_group rows are skipped, not rendered.
    let msgs = vec![
        dm("user", "q"),
        dm("tool_group", "2 tool calls"),
        dm("assistant", "the answer"),
    ];
    assert_eq!(collect_turn_assistant_text(&msgs), "the answer");
}

// ── wiring sentinels (#1468) ────────────────────────────────────────────────
//
// The pure tests above only prove the reconstruction helper is correct. The
// actual bug was that `persist_streaming_state` never USED it: it looked at
// `streaming_response` alone, found it empty (the text had been promoted into
// `self.messages`), and early-returned, leaving the non-CLI placeholder row at
// 0 chars. These sentinels fail if that call site is removed again, so the fix
// cannot silently regress while the helper tests stay green. Source-level
// because `persist_streaming_state` is an async `App` method over a live DB
// with no in-process seam, the same approach as `cli_silent_failure_test`.

const MESSAGING_SRC: &str = include_str!("../tui/app/messaging.rs");

#[test]
fn persist_streaming_state_reconstructs_the_turn_from_messages() {
    assert!(
        MESSAGING_SRC
            .contains("let memory_turn_text = collect_turn_assistant_text(&self.messages);"),
        "persist_streaming_state must reconstruct the in-flight turn from \
         self.messages; without this call the HTTP-provider completion that was \
         promoted out of streaming_response is lost on cancel (#1468)"
    );
}

#[test]
fn persist_streaming_state_writes_the_full_turn_when_the_db_row_is_empty() {
    assert!(
        MESSAGING_SRC.contains("let db_empty = msg.content.trim().is_empty();"),
        "the cancel-persist must detect the empty non-CLI placeholder row"
    );
    assert!(
        MESSAGING_SRC.contains("if db_empty && !memory_turn_text.trim().is_empty() {"),
        "when the DB row is still empty (HTTP provider; the live persister is \
         CLI-only) the full reconstructed turn must be written, not skipped, \
         or the completion vanishes on Esc-Esc (#1468)"
    );
}
