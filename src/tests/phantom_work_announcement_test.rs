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
fn announcement_at_start_of_multi_sentence_turn_is_phantom() {
    // mimo leads with the announcement then keeps talking — still phantom.
    // (2026-06-12: this exact shape dropped because the v1 regex required the
    // announcement to be the whole line.)
    assert!(has_phantom_tool_intent_no_tools(
        "Running fmt, clippy, tests now. Then fetching fresh commits.\n\nClippy clean. Tests next."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Running the tests now, then I'll report back."
    ));
}

#[test]
fn now_as_an_adverb_is_not_an_announcement() {
    // "now" mid-sentence (not a sentence boundary) must NOT match.
    assert!(!matches_work_announcement(
        "Running the suite now takes about ten minutes on CI."
    ));
    assert!(!matches_work_announcement(
        "Building it now would break the release, so let's wait."
    ));
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

#[test]
fn now_followed_by_colon_is_phantom() {
    // "Checking runs now: I'll verify the schedule." — the colon after "now"
    // is an imminence marker just like a period or ellipsis. The model leads
    // with the announcement, adds a colon, then continues narrating instead
    // of dispatching a tool call.
    assert!(has_phantom_tool_intent_no_tools(
        "Checking cron runs now: I'll verify the schedule."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Running checks now: let me look at the recent results."
    ));
    assert!(matches_work_announcement("Verifying the deployment now:"));
    assert!(matches_work_announcement(
        "Building the project now: clippy is clean."
    ));
}

// ── #464: anchor evasions observed live (MiniMax M3, three within an hour) ──

#[test]
fn announcement_after_lead_clause_is_phantom() {
    // "Internet's back, " defeated the ^-anchored regex; the clause-tolerant
    // matcher must still see the announcement.
    assert!(has_phantom_tool_intent_no_tools(
        "Internet's back, pushing now."
    ));
    assert!(matches_work_announcement("Internet's back, pushing now."));
}

#[test]
fn announcement_after_lead_sentence_is_phantom() {
    // A whole sentence before the announcement: per-sentence matching.
    assert!(has_phantom_tool_intent_no_tools(
        "Apologies Adolfo, you're right. Pushing now."
    ));
}

#[test]
fn announcement_behind_react_directive_is_phantom() {
    // The <<react:>> directive prefixed the narration and broke the anchor.
    assert!(has_phantom_tool_intent_no_tools(
        "<<react:🔥>> Pushing the 3 commits now."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Pushing the 3 commits now."
    ));
}

#[test]
fn announcement_is_forward_intent_even_post_success() {
    // The post-success gate must treat a work announcement as forward
    // intent: it is a promise of work, never a completion ack (#464).
    use crate::brain::agent::service::phantom::has_forward_intent_post_success;
    assert!(has_forward_intent_post_success(
        "Pushing the 3 commits now."
    ));
    assert!(has_forward_intent_post_success(
        "Internet's back, pushing now."
    ));
    // Past-tense completion acks stay exempt.
    assert!(!has_forward_intent_post_success(
        "Pushed all three commits to origin/main."
    ));
}

#[test]
fn loading_announcement_is_phantom() {
    // "load" family was missing from every verb list (#463).
    assert!(has_phantom_tool_intent_no_tools(
        "Loading the brain files now."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Let me load both properly now."
    ));
}

#[test]
fn ordinary_sentences_with_commas_stay_clean() {
    // The clause tolerance must not flag prose that merely contains a
    // comma and a gerund with no imminence marker.
    assert!(!matches_work_announcement(
        "Thanks, reading your report was helpful and no action is needed."
    ));
    assert!(!matches_work_announcement(
        "For the record, pushing to main requires a review."
    ));
}

#[test]
fn reported_on_it_filing_narration_is_phantom() {
    // #752: the exact zero-tool narration that reached the user via the
    // empty-reasoning fallback path. The turn-end verdict relies on this
    // detector catching it. "Let me check ..." is a covered intent phrase.
    assert!(has_phantom_tool_intent_no_tools(
        "On it. Filing the issue and starting the work now. Let me check the repo first."
    ));
}
