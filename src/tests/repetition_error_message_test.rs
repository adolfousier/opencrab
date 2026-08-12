//! A turn ended by the loop detector must not be reported as a provider fault
//! (#1023).
//!
//! The within-turn announcement-loop detector stops a turn when the model keeps
//! announcing an action without emitting the call. The user saw "Provider got
//! stuck repeating itself", which blames the provider for a decision this code
//! made, and sends them looking for provider-side causes.
//!
//! The `/models` suggestion stays, because it genuinely helps — a different
//! model usually does emit the call. What changed is that it now comes with the
//! real reason instead of standing in for a fault that did not happen.

use crate::brain::agent::error::{AgentError, format_user_error};

fn repetition_error() -> AgentError {
    AgentError::Internal(
        "Repetition detected: near-identical announcements repeated within the turn".to_string(),
    )
}

/// The message must not attribute the stop to the provider.
#[test]
fn the_message_does_not_blame_the_provider() {
    let msg = format_user_error(&repetition_error());
    assert!(
        !msg.to_lowercase().contains("provider"),
        "the loop detector ended this turn, not the provider: {msg}"
    );
}

/// It must say what actually happened, so the user can recognise it.
#[test]
fn the_message_names_the_real_cause() {
    let msg = format_user_error(&repetition_error()).to_lowercase();
    assert!(
        msg.contains("announcing") || msg.contains("loop"),
        "must describe the announce-without-calling loop: {msg}"
    );
}

/// Switching models still helps here, so keep offering it.
#[test]
fn the_message_still_offers_the_remedy_that_works() {
    let msg = format_user_error(&repetition_error());
    assert!(
        msg.contains("/models"),
        "a different model usually emits the call, so the suggestion is not \
         removed — only its stated reason is corrected: {msg}"
    );
}

/// The cross-turn variant maps to the same explanation.
#[test]
fn the_cross_turn_variant_maps_the_same_way() {
    let across = AgentError::Internal(
        "Repetition detected: near-identical announcements repeated across turns".to_string(),
    );
    assert_eq!(
        format_user_error(&across),
        format_user_error(&repetition_error())
    );
}

/// An unrelated provider failure keeps naming the provider.
#[test]
fn genuine_provider_failures_still_say_provider() {
    let stream = AgentError::Internal("error decoding response body".to_string());
    assert!(
        format_user_error(&stream)
            .to_lowercase()
            .contains("provider"),
        "a real stream break is the provider's, and should still say so"
    );
}
