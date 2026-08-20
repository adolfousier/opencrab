//! Whether a finished turn may rewrite the session's model.
//!
//! A turn reports the model it actually ran on, which is how an alias becomes
//! concrete: ask a CLI for `opus` and it answers as the version it resolved.
//! Refreshing the session's model from that keeps the footer honest without a
//! restart.
//!
//! It is only honest while the turn stayed on the session's own provider. A
//! turn that ran somewhere else reports that provider's model, and writing it
//! back replaces the user's pick with one they never chose. `/models` then
//! looks ignored: the selection is accepted, the next turn falls elsewhere,
//! and the answer's model overwrites the choice on the way out.

/// May a turn's reported model replace the session's current one?
///
/// `started_on_session_provider` carries the same contract the persisted pair
/// obeys (#705): false means the turn ran on a provider that was not the
/// session's, so nothing it reports describes the session's own choice.
pub fn should_refresh_session_model(
    started_on_session_provider: bool,
    current_model: &str,
    reported_model: &str,
) -> bool {
    if !started_on_session_provider {
        return false;
    }
    // A provider that reports nothing must not blank the pick.
    if reported_model.trim().is_empty() {
        return false;
    }
    current_model != reported_model
}
