//! Tests for `tool_name_heal::resolve_tool_name` — the self-heal that maps
//! a model's near-miss tool name to the registered one.
//!
//! Origin: issue #176 — a Mimo v2.5 model called `tg_send_message` instead
//! of the real `telegram_send`. The schema was in the request; the model
//! just guessed the wrong identifier. This heal routes the call instead of
//! erroring, the same spirit as the per-parameter PARAM_ALIASES heal.
//!
//! The matcher is deliberately conservative: it only heals on a unique,
//! high-confidence match, because routing to the wrong tool could fire an
//! unintended (possibly destructive) action.

use crate::brain::tools::tool_name_heal::resolve_tool_name;

fn registry() -> Vec<String> {
    [
        "telegram_send",
        "telegram_connect",
        "discord_send",
        "slack_send",
        "whatsapp_connect",
        "read_file",
        "write_file",
        "edit_file",
        "bash",
        "grep",
        "glob",
        "web_search",
        "analyze_image",
        "analyze_video",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn heals_the_exact_reported_case() {
    // The #176 case: tg → telegram, send present, message is extra.
    assert_eq!(
        resolve_tool_name("tg_send_message", &registry()).as_deref(),
        Some("telegram_send")
    );
}

#[test]
fn heals_common_telegram_send_variants() {
    for variant in [
        "telegram_send_message",
        "send_telegram_message",
        "tg_send",
        "telegram-send",
        "TelegramSend",
        "telegramSend",
    ] {
        assert_eq!(
            resolve_tool_name(variant, &registry()).as_deref(),
            Some("telegram_send"),
            "variant {variant:?} should heal to telegram_send"
        );
    }
}

#[test]
fn heals_single_char_typo() {
    // Levenshtein fallback: one transposition/edit.
    assert_eq!(
        resolve_tool_name("telegram_sned", &registry()).as_deref(),
        Some("telegram_send")
    );
}

#[test]
fn exact_name_passes_through() {
    assert_eq!(
        resolve_tool_name("telegram_send", &registry()).as_deref(),
        Some("telegram_send")
    );
}

#[test]
fn does_not_confuse_telegram_send_with_telegram_connect() {
    // `tg_connect` must heal to telegram_connect, not telegram_send.
    assert_eq!(
        resolve_tool_name("tg_connect", &registry()).as_deref(),
        Some("telegram_connect")
    );
}

#[test]
fn ambiguous_send_does_not_heal() {
    // Bare "send_message" could be telegram, discord, or slack — too
    // ambiguous to route safely. Must NOT heal.
    assert_eq!(resolve_tool_name("send_message", &registry()), None);
    assert_eq!(resolve_tool_name("send", &registry()), None);
}

#[test]
fn unrelated_name_does_not_heal() {
    // A genuinely unknown tool the model invented with no close match must
    // return None so the caller surfaces the normal NotFound error rather
    // than firing some random tool.
    assert_eq!(resolve_tool_name("launch_rockets", &registry()), None);
    assert_eq!(resolve_tool_name("delete_everything", &registry()), None);
}

#[test]
fn send_photo_does_not_route_to_telegram_send() {
    // `send_photo` shares only "send" with telegram_send (no "telegram"
    // token), so the token-overlap rule must NOT match it. It's also not a
    // registered tool here, so the correct answer is None.
    assert_eq!(resolve_tool_name("send_photo", &registry()), None);
}

#[test]
fn empty_inputs_are_safe() {
    assert_eq!(resolve_tool_name("", &registry()), None);
    assert_eq!(resolve_tool_name("telegram_send", &[]), None);
}

#[test]
fn does_not_cross_channels() {
    // `discord_send_message` heals to discord_send, never telegram_send.
    assert_eq!(
        resolve_tool_name("discord_send_message", &registry()).as_deref(),
        Some("discord_send")
    );
    // `wa_connect` → whatsapp_connect.
    assert_eq!(
        resolve_tool_name("wa_connect", &registry()).as_deref(),
        Some("whatsapp_connect")
    );
}

#[test]
fn short_unrelated_names_do_not_typo_match() {
    // "bash" vs "grep"/"glob" — short names must not edit-distance match
    // each other into a wrong route.
    let reg = registry();
    // "bish" is 1 edit from "bash" but nothing else — should heal to bash.
    assert_eq!(resolve_tool_name("bish", &reg).as_deref(), Some("bash"));
    // "xyz" is far from everything.
    assert_eq!(resolve_tool_name("xyz", &reg), None);
}
