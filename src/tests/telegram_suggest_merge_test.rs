//! Suggestion controls ride the reply bubble (#1204).
//!
//! Every turn ending in `suggest_options` used to produce TWO messages: the
//! formatted reply, then a standalone plain `💡 Suggested next:` bubble
//! carrying the keyboard, because the picker bypassed the rich pipeline
//! entirely. The controls now attach to the delivered answer.
//!
//! The layout cases below were lifted out of `suggest_options.rs`: the house
//! rule is that no source file carries a `#[cfg(test)] mod tests` block.

use uuid::Uuid;

use crate::channels::telegram::suggest_options::{
    FOLLOWUP_PREFIX, MAX_BUTTON_CHARS, MAX_NUMBERS_PER_ROW, SHARED_ROW_MAX_CHARS, SuggestLayout,
    folded_list_html, pick_layout, suggestion_rows_rich_html,
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
    assert!(o.iter().all(|s| s.chars().count() <= SHARED_ROW_MAX_CHARS));
    assert!(o.len() > MAX_NUMBERS_PER_ROW);
    assert_eq!(pick_layout(&o), SuggestLayout::Column);
}

#[test]
fn test_one_long_label_kills_the_shared_row() {
    let o = vec!["Yes".to_string(), "x".repeat(SHARED_ROW_MAX_CHARS + 1)];
    assert_eq!(pick_layout(&o), SuggestLayout::Column);
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
    let session = Uuid::new_v4();
    let html = suggestion_rows_rich_html(&opts(&["Ship it", "Hold"]), session);

    for i in 0..2 {
        let expected = format!("{FOLLOWUP_PREFIX}{session}:{i}");
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
    assert!(
        !html.contains("Ship it\" data="),
        "option text must not ride in callback data"
    );
}

#[test]
fn test_callback_data_stays_within_the_cap_at_the_worst_index() {
    // A uuid is 36 chars and the prefix 9, so the cap is only ever threatened
    // by the index. Pin the widest realistic one.
    let session = Uuid::new_v4();
    let widest = format!("{FOLLOWUP_PREFIX}{session}:{}", usize::from(u8::MAX));
    assert!(
        widest.len() <= 64,
        "#1204: {} bytes exceeds the callback_data cap",
        widest.len()
    );
}
