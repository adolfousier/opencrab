//! Regression: the phantom self-heal must catch a brief present-continuous
//! work announcement that ends the turn with no tool call.
//!
//! 2026-06-12: mimo-v2.5-pro replied just "Running checks now." (19 bytes),
//! emitted zero tool calls, and the turn dropped. The no-tools detector
//! early-returned at its `len() < 20` floor, so it never examined the text.
//! These announcements are now matched before the floor via a per-language
//! `work_announcement_re`, anchored and requiring an imminence marker (now /
//! ahora / agora / maintenant / сейчас / … / : ) so ordinary sentences that
//! merely open with a gerund are NOT flagged.

use crate::brain::agent::service::phantom::{
    has_phantom_tool_intent_no_tools, matches_work_announcement,
};

#[test]
fn english_running_checks_now_is_phantom() {
    assert!(has_phantom_tool_intent_no_tools("Running checks now."));
    assert!(has_phantom_tool_intent_no_tools("Checking the logs now…"));
    assert!(has_phantom_tool_intent_no_tools("Rebuilding now."));
    assert!(has_phantom_tool_intent_no_tools(
        "Running the test suite..."
    ));
    assert!(has_phantom_tool_intent_no_tools("Verifying the changes:"));
}

#[test]
fn work_announcement_detected_in_every_language() {
    assert!(
        has_phantom_tool_intent_no_tools("Ejecutando las pruebas ahora."),
        "es"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Executando os testes agora."),
        "pt"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Je vérifie les fichiers maintenant."),
        "fr"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Vérification en cours…"),
        "fr-noun"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Сейчас запускаю проверки."),
        "ru-front"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Запускаю проверки сейчас."),
        "ru-back"
    );
}

#[test]
fn work_announcement_rejects_plain_gerund_sentences() {
    // Opens with a gerund but is a statement, not an "about to act"
    // announcement — the regex must NOT match without an imminence marker.
    // (Tested against the regex directly: the public detector may still flag
    // some of these via unrelated pre-existing intent phrases — that's not
    // what this rule is responsible for.)
    assert!(!matches_work_announcement(
        "Reading the file is straightforward."
    ));
    assert!(!matches_work_announcement(
        "Running the suite revealed three failures."
    ));
    assert!(!matches_work_announcement(
        "Building blocks are fundamental here."
    ));
    assert!(!matches_work_announcement(
        "Checking is handled by the CI pipeline, not locally."
    ));
}

#[test]
fn work_announcement_matches_only_with_imminence_marker() {
    // The same gerund, with vs without the marker.
    assert!(matches_work_announcement("Running the tests now."));
    assert!(!matches_work_announcement("Running the tests helped."));
}
