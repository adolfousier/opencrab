//! The correction injected when a turn claims work it never ran (#796).
//!
//! The old wording stated what was missing ("your last response produced ZERO
//! tool_use blocks"), which argues against a position the model does not hold.
//! A model that believes it already ran `gh issue list` reads that as a
//! formatting complaint and keeps the belief. These pin the wording that
//! replaces it: the mechanism, and a check the model can actually apply.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::nudge::no_tool_calls_nudge;

#[test]
fn every_variant_states_that_reasoning_cannot_execute() {
    // The mechanism is the load-bearing sentence. Without it the correction is
    // just a complaint about formatting, which is what failed before.
    for nudge in [no_tool_calls_nudge(true), no_tool_calls_nudge(false)] {
        assert!(
            nudge.contains("nothing runs inside your reasoning"),
            "must state the mechanism: {nudge}"
        );
        assert!(
            nudge.contains("imagined it"),
            "must name the false belief: {nudge}"
        );
    }
}

#[test]
fn every_variant_keeps_the_finished_escape() {
    // Without an exit that is not a tool call, a model that genuinely finished
    // gets nudged, calls something pointless to comply, and is nudged again.
    for nudge in [no_tool_calls_nudge(true), no_tool_calls_nudge(false)] {
        assert!(
            nudge.contains("genuinely done"),
            "real completion needs a non-tool exit: {nudge}"
        );
    }
}

#[test]
fn the_local_variant_avoids_the_word_stop() {
    // Qwen/Kimi/DeepSeek read "STOP" as "wait for further instruction" and
    // reply with an acknowledgement instead of calling anything.
    let nudge = no_tool_calls_nudge(true);
    assert!(
        !nudge.contains("STOP"),
        "local models treat STOP as an instruction to wait: {nudge}"
    );
}

#[test]
fn the_local_variant_names_the_structured_api() {
    // These models write `{"tool_call": {...}}` as message text believing that
    // IS the invocation, so the channel has to be named.
    let nudge = no_tool_calls_nudge(true);
    assert!(nudge.contains("structured tool-call API"), "{nudge}");
    assert!(
        nudge.contains("does not execute"),
        "must say that text-shaped calls do nothing: {nudge}"
    );
}

#[test]
fn a_nudge_is_framed_as_a_system_message() {
    // The loop injects these as user-role messages; the bracketed [System: ...]
    // framing is what keeps them from reading as the user's own words.
    for nudge in [no_tool_calls_nudge(true), no_tool_calls_nudge(false)] {
        assert!(nudge.starts_with("[System:"), "{nudge}");
        assert!(nudge.ends_with(']'), "{nudge}");
    }
}
