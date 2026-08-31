//! Suggestion controls ride the reply bubble (#1204).
//!
//! Every turn ending in `suggest_options` used to produce TWO messages: the
//! formatted reply, then a standalone plain `💡 Suggested next:` bubble
//! carrying the keyboard, because the picker bypassed the rich pipeline
//! entirely. The controls now attach to the delivered answer.
//!
//! The layout cases below were lifted out of `suggest_options.rs`: the house
//! rule is that no source file carries a `#[cfg(test)] mod tests` block.

use crate::channels::telegram::suggest_options::{
    BUTTON_PADDING_CHARS, FOLLOWUP_PREFIX, MAX_BUTTON_CHARS, MAX_NUMBERS_PER_ROW, ROW_BUDGET_CHARS,
    SuggestLayout, folded_list_html, pick_layout, suggestion_rows_rich_html,
};

fn opts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ── Layout ladder (lifted, unchanged in substance) ────────────────────────

#[test]
fn test_short_few_options_share_one_row() {
    assert_eq!(
        pick_layout(&opts(&["Yes", "No", "Skip"])),
        SuggestLayout::SharedRow
    );
}

#[test]
fn test_five_short_options_do_not_share_a_row() {
    // More than MAX_NUMBERS_PER_ROW tap targets in one row leaves each too
    // small for a finger, so they drop to the Column tier.
    let o = opts(&["alpha", "beta", "gamma", "delta", "eps"]);
    let cost: usize = o
        .iter()
        .map(|s| s.chars().count() + BUTTON_PADDING_CHARS)
        .sum();
    assert!(
        cost <= ROW_BUDGET_CHARS,
        "the budget alone would allow this row — the count is what kills it"
    );
    assert!(o.len() > MAX_NUMBERS_PER_ROW);
    assert_eq!(pick_layout(&o), SuggestLayout::Column);
}

#[test]
fn test_one_long_label_kills_the_shared_row() {
    // Budget model (#52): a label too wide for the row pushes the cost past
    // ROW_BUDGET_CHARS even with a short sibling.
    let o = vec![
        "Yes".to_string(),
        "x".repeat(ROW_BUDGET_CHARS - "Yes".len() - 2 * BUTTON_PADDING_CHARS + 1),
    ];
    let cost: usize = o
        .iter()
        .map(|s| s.chars().count() + BUTTON_PADDING_CHARS)
        .sum();
    assert_eq!(
        cost,
        ROW_BUDGET_CHARS + 1,
        "test label must pierce the budget"
    );
    assert_eq!(pick_layout(&o), SuggestLayout::Column);
}

#[test]
fn test_row_budget_boundary_is_inclusive() {
    // Re-calibrated 2026-08-31 (#53, rich-plane margin): pad=2 moves the
    // boundary — 4×7 (4×7 + 4×2 pad = 36) and 2×16 (36) share the row,
    // while 4×8 (40) and 2×17 (38) stack. The 2×17-out arm is the row that
    // fit full on Latin rich under pad=1 and clipped Cyrillic by half a
    // glyph per button.
    let edge4 = opts(&["abandon", "confirm", "decline", "restart"]);
    assert_eq!(pick_layout(&edge4), SuggestLayout::SharedRow);
    let edge2 = vec!["x".repeat(16), "x".repeat(16)];
    assert_eq!(pick_layout(&edge2), SuggestLayout::SharedRow);
    let over4 = opts(&["approved", "declined", "postpone", "deferred"]);
    assert_eq!(pick_layout(&over4), SuggestLayout::Column);
    let over2 = vec!["x".repeat(17), "x".repeat(17)];
    assert_eq!(pick_layout(&over2), SuggestLayout::Column);
}

#[test]
fn test_the_button_width_boundary_is_exclusive() {
    // Measured on a real client: MAX_BUTTON_CHARS fits one line, past it wraps.
    assert_eq!(
        pick_layout(&["x".repeat(MAX_BUTTON_CHARS)]),
        SuggestLayout::Column
    );
    assert_eq!(
        pick_layout(&["Ship it".to_string(), "x".repeat(MAX_BUTTON_CHARS + 1)]),
        SuggestLayout::NumberedProse
    );
}

#[test]
fn test_a_folded_list_carries_no_header_and_is_escaped() {
    let body = folded_list_html(&opts(&["Ship it", "Review & merge"]));
    assert!(
        !body.contains("Suggested next"),
        "#1204: the list rides under the answer, so it has no header of its own"
    );
    assert!(body.contains("1. Ship it"));
    assert!(
        body.contains("2. Review &amp; merge"),
        "escaping proves the shared renderer ran, not a private one: {body}"
    );
}

// ── Callback data ─────────────────────────────────────────────────────────

#[test]
fn test_callback_data_carries_the_index_not_the_text() {
    // Telegram caps callback_data at 64 BYTES and an option's text can exceed
    // that on its own, so the index is what travels and the stash resolves it.
    // #1217: the token is opaque (8 hex chars), NOT the session id — the tap
    // resolves the serving session from the stash entry, never from data the
    // client could forge or mix across overlapping keyboards.
    let token = "ab12cd34";
    let html = suggestion_rows_rich_html(&opts(&["Ship it", "Hold"]), token);

    for i in 0..2 {
        let expected = format!("{FOLLOWUP_PREFIX}{token}:{i}");
        assert!(
            html.contains(&expected),
            "missing callback data {expected} in {html}"
        );
        assert!(
            expected.len() <= 64,
            "#1204: callback data must fit Telegram's 64-byte cap, got {}",
            expected.len()
        );
    }
    // The option text must not ride in the payload (it can exceed the cap).
    assert!(
        !html.contains("Ship it\" data="),
        "option text must not ride in callback data"
    );
}

#[test]
fn test_callback_data_stays_within_the_cap_at_the_worst_index() {
    // Token form is 9 + 8 + 1 + idx chars; pin the widest realistic index
    // against the cap (#1204).
    let token = "ffffffff";
    let widest = format!("{FOLLOWUP_PREFIX}{token}:{}", usize::from(u8::MAX));
    assert!(
        widest.len() <= 64,
        "#1204: {} bytes exceeds the callback_data cap",
        widest.len()
    );
}
