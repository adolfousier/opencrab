//! Whether a message submitted during a running turn is a duplicate (#798).
//!
//! Recalling the previous message with Arrow Up and pressing Enter while a turn
//! is still running used to queue it unconditionally, so an accidental
//! double-Enter became a second real turn: the same question answered twice,
//! both copies persisted, and both reloaded into context on every later turn of
//! the session. The duplicate is pure waste and biases the model toward
//! re-answering.
//!
//! The comparison is deliberately short-sighted. It looks only at the turn
//! currently in flight and the messages already queued behind it, never further
//! back, so deliberately repeating yourself later in a session is untouched.
//! Repetition is only meaningless while the first copy has not been answered
//! yet.
//!
//! Pure so the decision is testable without a TUI or a running turn.

/// What to do with a message submitted while the session is busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Submission {
    /// Not a duplicate: add it to the queue.
    Queue,
    /// Identical to the turn in flight; its answer is already being produced.
    DuplicateOfInFlight,
    /// Identical to something already waiting in the queue.
    DuplicateOfQueued,
}

impl Submission {
    /// Should the caller drop this submission?
    pub(crate) fn is_duplicate(self) -> bool {
        !matches!(self, Submission::Queue)
    }
}

/// Classify `incoming` against the turn in flight and the pending queue.
///
/// `in_flight` is the user message the running turn is answering, if known.
/// `queued` is what is already waiting behind it.
///
/// Matching is exact after trimming. Anything looser would eventually drop a
/// message the user meant to send, and a wrongly dropped message is far worse
/// than a duplicate: one wastes context, the other silently loses work.
pub(crate) fn classify(incoming: &str, in_flight: Option<&str>, queued: &[String]) -> Submission {
    let incoming = incoming.trim();

    // An empty submission is not a duplicate of anything; the caller's own
    // empty-input guard owns that case.
    if incoming.is_empty() {
        return Submission::Queue;
    }

    if in_flight.is_some_and(|m| m.trim() == incoming) {
        return Submission::DuplicateOfInFlight;
    }
    if queued.iter().any(|m| m.trim() == incoming) {
        return Submission::DuplicateOfQueued;
    }
    Submission::Queue
}
