//! Tests for the #1226 K fix: an option-surface halt strands the final
//! answer in the collapsed flow because the flow ends on the
//! suggest_options Tool entry, and the stock popper only lifts TRAILING
//! Text runs.
//!
//! Incident (smoke v3, 2026-08-28): the model delivered its final answer
//! mid-turn, then called suggest_options; the turn halted with
//! `options_pending`, the promote branch asked for the folded final, got
//! None silently, and the answer stayed imprisoned (badly formatted) in
//! the flow block while the buttons landed standalone.
//!
//! The fix gives `pop_trailing_folded_texts` an options-aware gate: when
//! options are pending it lifts the trailing Tool run aside, pops the
//! Text run immediately before it, and RESTORES the Tool run on top —
//! only the answer text leaves the block, the tool history stays. The
//! gate never fires on turns that did not halt on an option surface.

use crate::channels::telegram::flow::{FlowEntry, pop_trailing_folded_texts};

#[test]
fn options_pending_reclaims_answer_before_trailing_tool() {
    // The K shape: answer folded, then suggest_options appends a Tool
    // entry LAST. The gate must reach past the tool and pop the answer.
    let mut entries = vec![
        FlowEntry::Text("the substantive answer".into()),
        FlowEntry::Tool(4),
    ];
    let reclaimed = pop_trailing_folded_texts(&mut entries, true).expect("reclaims");
    assert_eq!(reclaimed, "the substantive answer");
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
    let reclaimed = pop_trailing_folded_texts(&mut entries, true).expect("reclaims");
    assert_eq!(reclaimed, "answer part one\n\nanswer part two");
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
    let reclaimed = pop_trailing_folded_texts(&mut entries, true).expect("reclaims");
    assert_eq!(reclaimed, "answer");
    assert_eq!(entries.len(), 2, "both tools restored");
    assert!(matches!(entries[0], FlowEntry::Tool(1)));
    assert!(matches!(entries[1], FlowEntry::Tool(2)));
}

#[test]
fn options_pending_without_preceding_text_changes_nothing() {
    // No Text run before the trailing tools: nothing to reclaim, and the
    // entries must come back untouched (no mutation on None).
    let mut entries = vec![FlowEntry::Tool(3), FlowEntry::Tool(4)];
    assert_eq!(pop_trailing_folded_texts(&mut entries, true), None);
    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[0], FlowEntry::Tool(3)));
    assert!(matches!(entries[1], FlowEntry::Tool(4)));
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
    assert_eq!(pop_trailing_folded_texts(&mut entries, false), None);
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
    let reclaimed = pop_trailing_folded_texts(&mut entries, false).expect("reclaims");
    assert_eq!(reclaimed, "plain final answer");
    assert_eq!(entries.len(), 1);
}
