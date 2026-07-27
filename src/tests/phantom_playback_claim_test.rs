//! Claiming to have played or launched something, with no tool call.
//!
//! Observed: a zero-tool turn replied *"Played it. Your notification.mp3 is
//! alive and well, sounds like a proper alert."* It ran nothing. Challenged
//! with "you did not execute any tool call", it then ran `afplay` and said
//! *"There, actually played it this time."*
//!
//! The turn WAS eligible for phantom detection — zero tool calls — but no
//! detector recognised the claim, because `action_verbs` had grown by
//! patching each fabrication that found a gap (deploys, then issue tracking,
//! then self-update, then investigation) and never covered running something
//! on the user's own machine.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::has_phantom_tool_intent_no_tools;

#[test]
fn the_observed_playback_claim_is_caught() {
    assert!(has_phantom_tool_intent_no_tools(
        "Played it. Your notification.mp3 is alive and well, sounds like a proper alert."
    ));
}

#[test]
fn launching_an_artifact_is_caught() {
    assert!(has_phantom_tool_intent_no_tools(
        "Launched the binary and it came up clean on port 8080."
    ));
}

#[test]
fn opening_a_file_is_caught() {
    assert!(has_phantom_tool_intent_no_tools(
        "Opened the file and the config is exactly as you described."
    ));
}

#[test]
fn an_offer_to_play_is_not_a_claim() {
    // Proposing must never be flagged; that is the behaviour to encourage.
    for proposal in [
        "I can play it for you if you want to hear it.",
        "Want me to play the notification sound?",
        "The next step would be to launch it and check the port.",
    ] {
        assert!(
            !has_phantom_tool_intent_no_tools(proposal),
            "a proposal must not be flagged: {proposal}"
        );
    }
}

#[test]
fn ordinary_prose_is_untouched() {
    assert!(!has_phantom_tool_intent_no_tools(
        "The notification file lives under public/sounds and is referenced by the alias."
    ));
}

// ── Multilingual, per the standing rule ─────────────────────────────────────
// Phrases live in every phantom_lang TOML and are scanned across all of them,
// never via detect_language, which misreads accented Latin.

#[test]
fn a_russian_playback_claim_is_caught() {
    // Cyrillic is detected reliably, so the Russian verbs are consulted.
    assert!(has_phantom_tool_intent_no_tools(
        "Воспроизвёл файл, звук в порядке."
    ));
}

#[test]
fn accented_latin_playback_claims_are_a_known_gap() {
    // NOT a passing behaviour — a documented limitation, asserted so it is
    // visible rather than discovered again by a user.
    //
    // The verbs ARE present in pt.toml and es.toml. But `action_verbs` are
    // deliberately gated to the DETECTED language (phantom.rs:79): they are
    // short single words with real cross-language collision risk, unlike the
    // multi-word intent phrases which are scanned across all languages.
    // `detect_language` misreads accented Latin, so these never reach the
    // Portuguese or Spanish lists.
    //
    // Widening the gating to fix this would import the collision risk the
    // comment warns about, so it needs its own change with its own evidence.
    // Until then a Portuguese or Spanish playback fabrication goes uncaught.
    assert!(
        !has_phantom_tool_intent_no_tools("Reproduzi o ficheiro e o som está perfeito."),
        "if this now passes, detect_language improved or the gating changed — \
         make these real assertions and delete this test"
    );
    assert!(!has_phantom_tool_intent_no_tools(
        "Reproduje el archivo y suena correctamente."
    ));
}
