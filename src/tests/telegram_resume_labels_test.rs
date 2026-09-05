//! Telegram resume: sender label and short session id formatting.

use crate::channels::telegram::TelegramState;
use crate::channels::telegram::resume::*;
use uuid::Uuid;

/// Owner ruling 2026-08-28: a session sitting in a DM with the bot must
/// be labelled with the BOT's username, never the reader's own name —
/// the reader IS the chat's human side, so handing it back as the
/// sender is useless.
#[tokio::test]
async fn dm_session_labels_the_bot_not_the_reader() {
    let state = TelegramState::new();
    state.set_bot_username("test_bot".to_owned()).await;
    let sender = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    state.register_session_chat(sender, 12345, None).await;
    let bot = teloxide::Bot::new("42:TEST");
    assert_eq!(
        sender_label(&state, &bot, sender, -100_999).await,
        "test_bot"
    );
}

/// Empty get_me cache (shouldn't happen post-boot): the DM arm must
/// degrade to the short session id, never to a getChat lookup that
/// would return the reader's own profile.
#[tokio::test]
async fn dm_session_without_cached_bot_username_falls_back_to_short_id() {
    let state = TelegramState::new();
    let sender = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    state.register_session_chat(sender, 12345, None).await;
    let bot = teloxide::Bot::new("42:TEST");
    assert_eq!(
        sender_label(&state, &bot, sender, -100_999).await,
        short_session_id(sender)
    );
}

#[test]
fn roll_line_joins_label_and_first_body_line() {
    let line = build_notify_roll_line("HQ", "*bzzt* status: green");
    assert_eq!(line, "📨 notify from HQ: *bzzt* status: green");
}

/// #61: only the FIRST body line is the announcement — the full text
/// reaches the session via the queue; a multiline body must not stack
/// into the roll line.
#[test]
fn roll_line_uses_only_first_body_line() {
    let line = build_notify_roll_line("ops", "first\nsecond\nthird");
    assert_eq!(line, "📨 notify from ops: first");
}

/// #61 dedupe: a body that carries its own "📨 notify from …:" echo
/// (hand-typed probe or quoted notify) must not render the phrase twice —
/// the envelope already names the sender. Header-shaped prefix up to the
/// first ':' is stripped (Alexey 2026-09-05, r4 smoke duplication).
#[test]
fn roll_line_strips_leading_self_echo_with_colon() {
    let line = build_notify_roll_line(
        "CLI tooling",
        "📨 notify from Smoke probe — 🔍 SMOKE r4: land on the roll",
    );
    assert_eq!(line, "📨 notify from CLI tooling: land on the roll");
}

#[test]
fn roll_line_strips_quoted_session_notify_echo() {
    let line = build_notify_roll_line("HQ", "📨 notify from HQ: build broke");
    assert_eq!(line, "📨 notify from HQ: build broke");
}

/// A "📨 notify from" line with no ':' can't be told apart from prose —
/// it must pass through untouched rather than eat content.
#[test]
fn roll_line_keeps_echo_without_colon() {
    let line = build_notify_roll_line("ops", "📨 notify from nobody");
    assert_eq!(line, "📨 notify from ops: 📨 notify from nobody");
}

/// Clean bodies are byte-identical through the strip pass.
#[test]
fn roll_line_untouched_for_clean_body() {
    let line = build_notify_roll_line("HQ", "*bzzt* status: green");
    assert_eq!(line, "📨 notify from HQ: *bzzt* status: green");
}

/// #61: labels are user data (topic/chat names). Same neutralization
/// the receipt card applies — angle brackets become single guillemets
/// before the line hits roll chrome that renders into HTML.
#[test]
fn roll_line_neutralizes_angle_brackets_in_label() {
    let line = build_notify_roll_line("<script>chat", "hello");
    assert!(!line.contains('<'), "no raw angle brackets: {line}");
    assert!(
        line.starts_with("📨 notify from ‹script›chat: hello"),
        "guillemet-swapped label: {line}"
    );
}

/// #61: the cap counts CHARS, not bytes — a Cyrillic/emoji-heavy
/// notify must truncate on a char boundary (no panics, no mojibake)
/// and mark the cut with an ellipsis.
#[test]
fn roll_line_caps_multibyte_on_char_boundary() {
    let label = "э".repeat(100);
    let body = "ж".repeat(100);
    let line = build_notify_roll_line(&label, &body);
    let count = line.chars().count();
    assert!(
        count <= NOTIFY_ROLL_LINE_MAX + 1,
        "cap + ellipsis, got {count}"
    );
    assert!(line.ends_with('…'), "cut marked with ellipsis");
}

/// #61: under the cap the line is verbatim — no ellipsis, no loss.
#[test]
fn roll_line_under_cap_has_no_ellipsis() {
    let line = build_notify_roll_line("ops", "short body");
    assert!(!line.ends_with('…'));
    assert!(line.contains("short body"));
}
