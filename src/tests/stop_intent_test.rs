//! Natural-language stop detection (#965).
//!
//! The old gate was an exact match on the bare word `stop`, so `STOP!!!` and
//! `hold on` sailed straight through to the agent while tools kept running.
//! The dangerous direction is the opposite one though: cancelling an
//! instruction like "stop the docker container" silently drops requested work,
//! so the negative cases below matter more than the positive ones.

use crate::utils::stop_intent::{is_stop_command_or_intent, is_stop_intent, normalize};

#[test]
fn normalize_strips_punctuation_and_case() {
    assert_eq!(normalize("STOP!!!"), "stop");
    assert_eq!(normalize("  Stop.  "), "stop");
    assert_eq!(normalize("HOLD ON, BRO!"), "hold on bro");
}

#[test]
fn punctuated_and_shouted_forms_cancel() {
    // The exact-match gate failed every one of these.
    for text in [
        "stop",
        "STOP!!!",
        "Stop.",
        "  stop  ",
        "STOP!!!!!!!!",
        "stop!",
    ] {
        assert!(is_stop_intent(text), "should cancel: {text:?}");
    }
}

#[test]
fn pause_phrases_cancel() {
    for text in [
        "hold on",
        "hold up",
        "hang on",
        "wait a sec",
        "wait a second",
        "one moment",
    ] {
        assert!(is_stop_intent(text), "should cancel: {text:?}");
    }
}

#[test]
fn a_pause_phrase_opening_a_longer_message_cancels() {
    // The reported shape: a shouted pause followed by explanation.
    assert!(is_stop_intent(
        "HOLD ON BRO! we need to check something first"
    ));
    assert!(is_stop_intent("wait a sec, I need to look at that"));
}

#[test]
fn addressing_the_bot_still_cancels() {
    for text in [
        "stop crab",
        "stop crabs",
        "hold on crab",
        "hold on crabs",
        "stop please",
        "stop bro",
        "stop now",
    ] {
        assert!(is_stop_intent(text), "should cancel: {text:?}");
    }
}

#[test]
fn every_supported_language_can_stop_it() {
    // Scanned across all six, never via a language guess: a wrong guess would
    // silently disarm the kill switch for whoever it guessed wrong about.
    for text in [
        "stop",     // en
        "стоп",     // ru
        "подожди",  // ru
        "espera",   // es
        "pare",     // pt
        "arrete",   // fr
        "attends",  // fr
        "berhenti", // id
        "tunggu",   // id
    ] {
        assert!(is_stop_intent(text), "should cancel: {text:?}");
    }
}

#[test]
fn instructions_about_stopping_are_not_interrupts() {
    // Cancelling any of these would drop work the user explicitly asked for,
    // which is a worse bug than the one this module fixes.
    for text in [
        "stop the docker container",
        "wait for the build to finish",
        "cancel the subscription",
        "can you stop the server please",
        "I had to stop working on it yesterday",
        "halt the deployment when tests fail",
    ] {
        assert!(!is_stop_intent(text), "must NOT cancel: {text:?}");
    }
}

#[test]
fn an_address_term_alone_is_not_an_interrupt() {
    // Stripping the whole message down to nothing must not count as a match.
    for text in ["crab", "crabs", "bot", "please", "bro"] {
        assert!(!is_stop_intent(text), "must NOT cancel: {text:?}");
    }
}

#[test]
fn empty_and_whitespace_are_not_interrupts() {
    for text in ["", "   ", "!!!", "..."] {
        assert!(!is_stop_intent(text), "must NOT cancel: {text:?}");
    }
}

#[test]
fn the_slash_command_still_works_including_the_group_spelling() {
    assert!(is_stop_command_or_intent("/stop"));
    assert!(is_stop_command_or_intent("/stop@opencrabsbot"));
    assert!(is_stop_command_or_intent("/STOP"));
    assert!(is_stop_command_or_intent("stop"));
}

#[test]
fn unrelated_slash_commands_never_cancel() {
    // Switching models applies to the next run; it must not drop current work.
    for text in ["/models", "/help", "/usage", "/new", "/status"] {
        assert!(
            !is_stop_command_or_intent(text),
            "must NOT cancel: {text:?}"
        );
    }
}
