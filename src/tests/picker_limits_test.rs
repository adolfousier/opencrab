//! Paging and filtering the model picker.
//!
//! The picker sent the whole catalogue twice: enumerated in the message text,
//! and again as one button per model. With OpenRouter's several hundred, the
//! text alone passed Telegram's 4096 characters, `editMessageText` answered
//! MESSAGE_TOO_LONG, and the only trace was a log line. From the chat, the
//! picker did nothing at all.

use crate::channels::telegram::picker_limits::{
    MODEL_PAGE_SIZE, nav_callback_data, page_of, page_text, parse_nav_callback,
};

fn catalogue(n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| format!("some-vendor/model-name-v{i}"))
        .collect()
}

#[test]
fn a_page_holds_only_its_own_slice() {
    let models = catalogue(400);
    let page = page_of(&models, 2, None);

    assert_eq!(page.models.len(), MODEL_PAGE_SIZE);
    assert_eq!(page.models[0], models[2 * MODEL_PAGE_SIZE]);
    assert_eq!(page.total_pages, 20);
    assert_eq!(page.matched, 400);
}

#[test]
fn the_text_stays_inside_telegrams_limit() {
    let models = catalogue(400);
    let page = page_of(&models, 0, None);
    let text = page_text("OpenRouter", "gpt-5", models.len(), &page);

    assert!(
        text.chars().count() <= 4096,
        "still {} chars, Telegram would reject it again",
        text.chars().count()
    );
}

#[test]
fn a_filter_narrows_to_matching_names() {
    let mut models = catalogue(50);
    models.push("anthropic/claude-opus-5".to_string());
    models.push("anthropic/claude-sonnet-5".to_string());

    let page = page_of(&models, 0, Some("claude"));

    assert_eq!(page.matched, 2);
    assert_eq!(page.total_pages, 1);
    assert!(page.models.iter().all(|m| m.contains("claude")));
}

#[test]
fn filtering_ignores_case() {
    let models = vec!["Anthropic/Claude-Opus-5".to_string()];
    assert_eq!(page_of(&models, 0, Some("CLAUDE")).matched, 1);
}

#[test]
fn a_filter_matching_nothing_says_so_instead_of_showing_an_empty_list() {
    let models = catalogue(30);
    let page = page_of(&models, 0, Some("nonexistent"));
    let text = page_text("OpenRouter", "gpt-5", models.len(), &page);

    assert_eq!(page.matched, 0);
    assert!(text.contains("No model matches"), "got: {text}");
}

#[test]
fn a_stale_page_number_clamps_instead_of_showing_nothing() {
    // A Next button drawn before a filter was applied would otherwise land on
    // a page that no longer exists.
    let models = catalogue(30);
    let page = page_of(&models, 99, None);

    assert_eq!(page.page, 1, "clamped to the last real page");
    assert!(!page.models.is_empty());
}

#[test]
fn one_page_of_models_shows_no_navigation() {
    let models = catalogue(5);
    let page = page_of(&models, 0, None);

    assert_eq!(page.total_pages, 1);
    assert!(!page.has_prev() && !page.has_next());
}

#[test]
fn navigation_payload_survives_a_round_trip() {
    let data = nav_callback_data("custom:modelscope", 3, Some("qwen"));
    assert_eq!(
        parse_nav_callback(&data),
        Some((3, "custom:modelscope".to_string(), Some("qwen".to_string())))
    );
}

#[test]
fn navigation_payload_carries_a_provider_holding_a_colon() {
    // `custom:<name>` and `:free` model suffixes are why the separator is a
    // pipe; splitting on `:` folded them into each other.
    let data = nav_callback_data("custom:dialagram", 1, None);
    assert_eq!(
        parse_nav_callback(&data),
        Some((1, "custom:dialagram".to_string(), None))
    );
}

#[test]
fn an_overlong_filter_is_dropped_rather_than_truncated() {
    // Telegram caps callback_data at 64 bytes. A truncated filter would show
    // the wrong models; dropping it shows more than intended, which is safe.
    let long = "x".repeat(80);
    let data = nav_callback_data("openrouter", 2, Some(&long));

    assert!(data.len() <= 64, "payload is {} bytes", data.len());
    assert_eq!(
        parse_nav_callback(&data),
        Some((2, "openrouter".to_string(), None))
    );
}
