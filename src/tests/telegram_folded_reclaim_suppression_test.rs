//! Tests for the #1152 fix: a final suppressed by intermediate-dedup must
//! NOT have its trailing folded run reclaimed as an orphan fragment.
//!
//! Incident: dedup suppressed the 1484-char final (it matched an already
//! delivered intermediate bubble), then the unconditional #478 reclaim
//! popped whatever trailing folded run remained — a 62-char tail chunk —
//! and delivered it as a raw orphan bubble below the full answer.
//!
//! The fix gates reclaim on the `suppressed_final` flag instead of trying
//! to re-verify coverage with `folded_duplicates_final`, because streaming
//! can leave a SUFFIX chunk folded in the block and that predicate
//! deliberately matches prefix overlap only. The tests below pin both
//! facts plus the exact strip semantics of `pop_trailing_folded_texts`.

use crate::channels::telegram::flow::{
    FlowEntry, folded_duplicates_final, pop_trailing_folded_texts,
};

#[test]
fn suffix_fragment_of_answer_is_not_a_predicate_duplicate() {
    // The #1152 incident shape: the block holds a TAIL chunk of the answer.
    // `folded_duplicates_final` returns false here — exactly why the reclaim
    // gate cannot use it to arbitrate coverage, and why discard is driven by
    // the suppression flag instead.
    let full = "Alpha bravo charlie delta echo foxtrot golf hotel india juliet";
    let tail = "golf hotel india juliet";
    assert!(!folded_duplicates_final(tail, full));
}

#[test]
fn prefix_head_still_counts_as_duplicate() {
    // Site B's guard (#300) keeps working: a truncated head of the final
    // folded into the block IS caught by the >=20-char prefix overlap.
    let full = "Alpha bravo charlie delta echo foxtrot golf hotel india juliet";
    let head = "Alpha bravo charlie delta echo";
    assert!(folded_duplicates_final(head, full));
}

#[test]
fn short_verbatim_copy_is_duplicate_at_any_length() {
    // Exact equality duplicates at any length (#316); normalization makes
    // whitespace-only differences irrelevant, mirroring the dedup normalizer.
    assert!(folded_duplicates_final("Done ✓", "Done   ✓"));
}

#[test]
fn pop_joins_whole_trailing_text_run() {
    // What take_folded_final strips after suppression: the ENTIRE trailing
    // run of Text entries, joined — multi-part answers included (#478).
    let mut entries = vec![
        FlowEntry::Tool(0),
        FlowEntry::Text("part one".to_string()),
        FlowEntry::Text("part two".to_string()),
    ];
    let joined = pop_trailing_folded_texts(&mut entries).expect("trailing run exists");
    assert_eq!(joined, "part one\n\npart two");
    assert_eq!(entries.len(), 1, "the Tool entry must survive");
}

#[test]
fn pop_stops_at_tool_entry() {
    // Mid-turn narration sits BEFORE later tool calls; only the trailing
    // run after the last tool is candidate answer material.
    let mut entries = vec![
        FlowEntry::Text("narration".to_string()),
        FlowEntry::Tool(1),
        FlowEntry::Text("tail".to_string()),
    ];
    let joined = pop_trailing_folded_texts(&mut entries).expect("tail run exists");
    assert_eq!(joined, "tail");
    assert_eq!(entries.len(), 2, "narration + tool stay in the block");
}

#[test]
fn pop_on_tool_ended_flow_returns_none() {
    // Flow ended on a tool call -> nothing folded to strip; delivery falls
    // through to response.content handling as before.
    let mut entries: Vec<FlowEntry> = vec![FlowEntry::Tool(3)];
    assert_eq!(pop_trailing_folded_texts(&mut entries), None);
}
