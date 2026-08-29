//! Tests for the #1226 K fix and the #31 trailer reclaim.
//!
//! #1226 incident (smoke v3, 2026-08-28): the model delivered its final
//! answer mid-turn, then called suggest_options; the turn halted with
//! `options_pending`, the promote branch asked for the folded final, got
//! None silently, and the answer stayed imprisoned (badly formatted) in
//! the flow block while the buttons landed standalone.
//!
//! The fix gives `pop_trailing_folded_texts` an options-aware gate: when
//! options are pending it lifts the trailing Tool run aside, pops the Text
//! run immediately before it, and RESTORES the Tool run on top — only the
//! answer text leaves the block, the tool history stays. The gate never
//! fires on turns that did not halt on an option surface.
//!
//! #31 extension (smoke v4 abandonment): after the halt, the model's
//! text-only sign-off iteration folds ANOTHER Text run into the flow —
//! AFTER the suggest_options Tool entry. The gate now returns BOTH runs:
//! `(host, trailer)` — the answer before the Tool, the sign-off after it.
//! Nothing is discarded; `settle_options_reclaim` arbitrates against
//! `response.content` (host wins the content slot, duplicates die,
//! an unfolded content leftover is promoted to the trailer).

use crate::channels::telegram::flow::{
    pop_trailing_folded_texts, settle_options_reclaim, FlowEntry,
};

#[test]
fn options_pending_reclaims_answer_before_trailing_tool() {
    // The K shape: answer folded, then suggest_options appends a Tool
    // entry LAST. The gate must reach past the tool and pop the answer.
    let mut entries = vec![
        FlowEntry::Text("the substantive answer".into()),
        FlowEntry::Tool(4),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host.as_deref(), Some("the substantive answer"));
    assert_eq!(trailer, None, "no post-halt run folded");
    // The Tool entry is RESTORED — the flow block keeps its tool history.
    assert_eq!(entries.len(), 1, "tool entry restored after reclaim");
    assert!(matches!(entries[0], FlowEntry::Tool(4)));
}

#[test]
fn options_pending_pops_whole_run_before_tool() {
    // Multi-part answer (#478 run semantics carry across the gate): both
    // parts come out, joined in order; earlier narration behind an older
    // tool stays in the block.
    let mut entries = vec![
        FlowEntry::Text("mid-turn narration".into()),
        FlowEntry::Tool(2),
        FlowEntry::Text("answer part one".into()),
        FlowEntry::Text("answer part two".into()),
        FlowEntry::Tool(5),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host.as_deref(), Some("answer part one\n\nanswer part two"));
    assert_eq!(trailer, None);
    assert_eq!(entries.len(), 3, "narration + old tool + suggest tool stay");
    assert!(matches!(entries[0], FlowEntry::Text(_)));
    assert!(matches!(entries[1], FlowEntry::Tool(2)));
    assert!(matches!(entries[2], FlowEntry::Tool(5)));
}

#[test]
fn options_pending_restores_multiple_trailing_tools_in_order() {
    let mut entries = vec![
        FlowEntry::Text("answer".into()),
        FlowEntry::Tool(1),
        FlowEntry::Tool(2),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host.as_deref(), Some("answer"));
    assert_eq!(trailer, None);
    assert_eq!(entries.len(), 2, "both tools restored");
    assert!(matches!(entries[0], FlowEntry::Tool(1)));
    assert!(matches!(entries[1], FlowEntry::Tool(2)));
}

#[test]
fn options_pending_without_preceding_text_changes_nothing() {
    // No Text run before the trailing tools: nothing to reclaim, and the
    // entries must come back untouched (no mutation on None).
    let mut entries = vec![FlowEntry::Tool(3), FlowEntry::Tool(4)];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host, None);
    assert_eq!(trailer, None);
    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[0], FlowEntry::Tool(3)));
    assert!(matches!(entries[1], FlowEntry::Tool(4)));
}

#[test]
fn options_pending_pops_host_and_trailer_around_tool() {
    // #31 both-runs shape: the sign-off folds AFTER the suggest_options
    // Tool entry. The gate returns host AND trailer; the Tool entry is
    // restored so the flow block keeps its tool history.
    let mut entries = vec![
        FlowEntry::Text("mid-turn narration".into()),
        FlowEntry::Tool(2),
        FlowEntry::Text("the substantive answer".into()),
        FlowEntry::Tool(4),
        FlowEntry::Text("trailer line one".into()),
        FlowEntry::Text("trailer line two".into()),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host.as_deref(), Some("the substantive answer"));
    assert_eq!(
        trailer.as_deref(),
        Some("trailer line one\n\ntrailer line two"),
        "post-halt run joins in order"
    );
    assert_eq!(entries.len(), 3, "narration + old tool + suggest tool stay");
    assert!(matches!(entries[0], FlowEntry::Text(_)));
    assert!(matches!(entries[1], FlowEntry::Tool(2)));
    assert!(matches!(entries[2], FlowEntry::Tool(4)));
}

#[test]
fn options_pending_trailer_only_when_no_host_text() {
    // The answer arrived in response.content (never folded), but the
    // sign-off still folded after the Tool entry: trailer comes back
    // alone, host None, tool history intact.
    let mut entries = vec![
        FlowEntry::Tool(4),
        FlowEntry::Text("all set — sign-off".into()),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host, None);
    assert_eq!(trailer.as_deref(), Some("all set — sign-off"));
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0], FlowEntry::Tool(4)));
}

#[test]
fn options_pending_trailing_text_without_tool_is_host() {
    // Tolerance: no Tool entry trailed the run — the gate's precondition
    // (flow ends on the suggest Tool) does not hold, so the popped run is
    // the stock trailing answer, NEVER a trailer.
    let mut entries = vec![FlowEntry::Text("plain trailing answer".into())];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, true);
    assert_eq!(host.as_deref(), Some("plain trailing answer"));
    assert_eq!(trailer, None);
    assert!(entries.is_empty());
}

#[test]
fn gate_closed_keeps_tool_last_invariant() {
    // options_pending = false: a tool-ended flow still reclaims nothing —
    // the stock invariant (answer is in response.content) is untouched
    // for every turn that did not halt on an option surface.
    let mut entries = vec![
        FlowEntry::Text("the substantive answer".into()),
        FlowEntry::Tool(4),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, false);
    assert_eq!(host, None);
    assert_eq!(trailer, None);
    assert_eq!(entries.len(), 2, "nothing consumed");
}

#[test]
fn gate_closed_on_trailing_text_unchanged() {
    // Regression guard: the gate's plumbing must not disturb the stock
    // trailing-run pop when the flow ends on Text.
    let mut entries = vec![
        FlowEntry::Tool(0),
        FlowEntry::Text("plain final answer".into()),
    ];
    let (host, trailer) = pop_trailing_folded_texts(&mut entries, false);
    assert_eq!(host.as_deref(), Some("plain final answer"));
    assert_eq!(trailer, None);
    assert_eq!(entries.len(), 1);
}

#[test]
fn settle_host_wins_content_slot() {
    // The old prepend shipped the ack inside the answer bubble (#31
    // abandonment): host takes the content slot, the ack rides AFTER the
    // buttons as the trailer.
    let (text, trailer) = settle_options_reclaim(
        "all set, pushed".into(),
        Some("the substantive answer".into()),
        Some("sign-off paragraph".into()),
    );
    assert_eq!(text, "the substantive answer");
    assert_eq!(trailer.as_deref(), Some("sign-off paragraph"));
}

#[test]
fn settle_drops_trailer_duplicate_of_host() {
    // Provider double-copy (inferhub, smoke v4/S4): the sign-off run IS
    // the final text — no double-send.
    let (text, trailer) = settle_options_reclaim(
        "ack".into(),
        Some("the substantive answer".into()),
        Some("the substantive answer".into()),
    );
    assert_eq!(text, "the substantive answer");
    assert_eq!(trailer, None);
}

#[test]
fn settle_content_becomes_trailer_when_no_run_folded() {
    // Keep-never-discard: the popper found no trailer run (the ack never
    // folded into the flow) but content carries a non-duplicate leftover —
    // it BECOMES the trailer instead of vanishing.
    let (text, trailer) = settle_options_reclaim(
        "unfolded sign-off ack".into(),
        Some("the substantive answer".into()),
        None,
    );
    assert_eq!(text, "the substantive answer");
    assert_eq!(trailer.as_deref(), Some("unfolded sign-off ack"));
}

#[test]
fn settle_content_duplicate_of_host_not_promoted() {
    // The content leftover is a duplicate of the host (prefix overlap) —
    // promoting it would double-send the answer.
    let (text, trailer) = settle_options_reclaim(
        "the substantive answer".into(),
        Some("the substantive answer delivered in full".into()),
        None,
    );
    assert_eq!(text, "the substantive answer delivered in full");
    assert_eq!(trailer, None);
}

#[test]
fn settle_no_host_uses_content() {
    // Nothing folded before the Tool: content IS the final text, trailer
    // rides unchanged.
    let (text, trailer) =
        settle_options_reclaim("the whole answer".into(), None, Some("sign-off".into()));
    assert_eq!(text, "the whole answer");
    assert_eq!(trailer.as_deref(), Some("sign-off"));
}

#[test]
fn settle_no_host_no_trailer_is_stock() {
    // Plain shape: content only, no runs — the stock reclaim result.
    let (text, trailer) = settle_options_reclaim("the whole answer".into(), None, None);
    assert_eq!(text, "the whole answer");
    assert_eq!(trailer, None);
}
