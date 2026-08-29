//! #1261: a sequenced plan announcement in a zero-tool turn is a phantom.
//!
//! A turn handed a multi-step implementation task answered with one sentence
//! of intent — "Setting up the plan, then mapping every call site before
//! touching anything." — emitted zero tool calls, and that sentence was
//! delivered as the turn's answer. None of the work happened.
//!
//! Every pattern in all six language files missed it, for two independent
//! reasons. `work_announcement_re` enumerates EXECUTION verbs (running,
//! checking, pushing) and never carried the PREPARATION ones (mapping,
//! planning, drafting, listing); and both that pattern and `gerund_re` key on
//! an imminence marker — a trailing `now` / `…` / `:`, or a leading "Now" —
//! which a clause sequenced with `then` / `before` does not carry.
//!
//! `plan_announcement_re` closes that shape in every language. It is read by
//! the zero-tool gate only: once a tool has run, "Updating the config fixed
//! it, then the build passed" is the same shape as a legitimate recap.

use crate::brain::agent::service::phantom::{
    has_forward_intent_post_success, has_phantom_tool_intent_no_tools, matches_plan_announcement,
};

/// The shape that was delivered as an answer, with the project's nouns
/// replaced by neutral ones.
const REPORTED: &str =
    "Setting up the plan, then mapping every call site before touching anything.";

#[test]
fn the_reported_announcement_is_detected() {
    assert!(
        matches_plan_announcement(REPORTED),
        "the sequenced plan announcement must match"
    );
    assert!(
        has_phantom_tool_intent_no_tools(REPORTED),
        "a turn that emitted zero tool calls has already proved nothing ran"
    );
}

/// Scanned across `all_langs()`, not against a detected language: the same
/// promise in any of the six must be caught.
#[test]
fn the_shape_is_caught_in_every_supported_language() {
    for (lang, text) in [
        ("en", "Mapping the consumers before making any edit."),
        (
            "pt",
            "Configurando o plano, depois mapeando cada consumidor antes de mexer em nada.",
        ),
        (
            "es",
            "Configurando el plan, luego mapeando cada consumidor antes de tocar nada.",
        ),
        (
            "fr",
            "Mise en place du plan, puis cartographie de chaque appelant avant de toucher \
             quoi que ce soit.",
        ),
        ("ru", "Составляю план, затем размечаю каждого потребителя."),
        (
            "id",
            "Menyiapkan rencana, lalu memetakan setiap pemanggil sebelum menyentuh apa pun.",
        ),
    ] {
        assert!(
            matches_plan_announcement(text),
            "{lang}: sequenced plan announcement not detected"
        );
        assert!(
            has_phantom_tool_intent_no_tools(text),
            "{lang}: zero-tool gate did not fire"
        );
    }
}

/// The sequencing marker is what separates a promise of steps from a
/// statement about them. Without it there is no announced ordering, and the
/// detector must not guess.
#[test]
fn a_gerund_without_a_sequencing_marker_is_not_an_announcement() {
    assert!(!matches_plan_announcement(
        "Mapping the consumers is straightforward once the registry is loaded."
    ));
    assert!(!matches_plan_announcement(
        "Reading the file was enough to find the bug."
    ));
}

/// A completed report is not a promise, whichever words it happens to use.
#[test]
fn finished_work_is_not_flagged() {
    for text in [
        "Done. Pushed 3 commits and closed the issue.",
        "The retry ladder runs before the fallback walk, which is why the log shows both.",
        "Nothing changed here, then the build passed.",
    ] {
        assert!(
            !matches_plan_announcement(text),
            "false positive on a report: {text}"
        );
    }
}

/// Deliberately not wired into the post-success path: after a tool has run,
/// this shape is ambiguous with a recap, and the zero-tool count that
/// resolves it is gone.
#[test]
fn the_post_success_gate_is_left_alone() {
    assert!(
        !has_forward_intent_post_success(
            "Updating the config fixed it, then the build passed clean."
        ),
        "a recap after successful tool calls must not be treated as a promise"
    );
}

/// A structured answer that merely contains the words is not an
/// announcement — the existing prose windows stop at the first structural
/// line, and that must keep holding for the new pattern.
#[test]
fn a_structured_answer_is_not_an_announcement() {
    let answer = "Here is the ordering.\n\n\
         | step | owner |\n\
         |---|---|\n\
         | Setting up the plan, then mapping every call site | me |\n";
    assert!(
        !has_phantom_tool_intent_no_tools(answer),
        "a table row is content, not a promise"
    );
}
