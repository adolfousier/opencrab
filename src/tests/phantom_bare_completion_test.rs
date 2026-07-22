//! Regression for the #680 follow-up: a bare "Done." with ZERO tool calls,
//! answering a request to BUILD/CREATE a deliverable, evaded every phantom
//! detector.
//!
//! Context: a model was asked to produce a full artifact (initiate an A2A
//! collaboration, build a project, provide complete copy-pasteable code) and
//! replied with a lone "Done." — no code, no tool calls. No self-heal fired
//! because every zero-tool detector bails on length (`trimmed.len() < 20` /
//! `< 40`) and "done" is in no verb list, so a 5-byte completion word is
//! invisible.
//!
//! The fix flags the trio: zero tools + a content-free bare completion word +
//! a delivery-intent request. The delivery gate keeps a legitimate cross-turn
//! ack ("did you commit? — Done.") untouched, since interrogatives never count
//! as delivery intent.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::{is_bare_completion_only, is_delivery_intent};

#[test]
fn bare_completion_words_are_content_free() {
    for phrase in [
        "Done.",
        "done",
        "All done!",
        "Ready.",
        "Finished.",
        "Complete.",
        "All set.",
        "Done ✅",
        "  Done.  ",
        "<<react:🔥>> Done.",
    ] {
        assert!(
            is_bare_completion_only(phrase),
            "expected bare completion: {phrase:?}"
        );
    }
}

#[test]
fn a_real_confirmation_with_specifics_is_not_bare() {
    // A genuine ack names WHAT happened — longer than any bare token.
    assert!(!is_bare_completion_only(
        "Committed as 7256f6 — 11 files changed."
    ));
    // A deliverable inline (code fence) is backed by content.
    assert!(!is_bare_completion_only(
        "Done.\n```rust\nfn main() {}\n```"
    ));
    // Empty text is not a completion claim.
    assert!(!is_bare_completion_only(""));
    // A completion word followed by real substance is not "bare".
    assert!(!is_bare_completion_only(
        "Done. The Remotion project is under src/video/ and runs at 30fps."
    ));
}

#[test]
fn delivery_requests_are_recognized() {
    for req in [
        "create a 30-second motion graphics masterpiece",
        "Bro, act as yourself. I want you to create a video and provide the full code.",
        "build the Remotion project and provide the absolute full copy-pasteable code",
        "write me a script that renders the animation",
        "generate the component",
        "implement the parser",
    ] {
        assert!(is_delivery_intent(req), "expected delivery intent: {req:?}");
    }
}

#[test]
fn questions_about_work_are_not_delivery_intent() {
    // Interrogatives ask ABOUT work, they don't request an artifact — a bare
    // "Done." answering them is a legitimate cross-turn ack, not a phantom.
    for q in [
        "did you build it?",
        "is the code ready?",
        "what does this component do?",
        "how does the build work?",
        "where is the remotion project?",
    ] {
        assert!(
            !is_delivery_intent(q),
            "should not be delivery intent: {q:?}"
        );
    }
}

#[test]
fn channel_prefix_is_stripped_before_intent_check() {
    // A leading channel/context bracket must not hide the request verb.
    assert!(is_delivery_intent(
        "[Channel: Telegram | group]\ncreate the animation and provide the code"
    ));
}
