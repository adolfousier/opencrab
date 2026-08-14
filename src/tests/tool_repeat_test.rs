//! Consecutive identical tool-round detection (#1030).
//!
//! The detector adds one signal to the existing nudge/retry/fallback ladder.
//! It must never end a turn, never suppress a call, and never let a replayed
//! turn inflate its count.

use crate::brain::agent::service::tool_repeat::{
    REPEAT_NUDGE_AT, RepeatVerdict, ToolRepeatTracker, observe_round, signature,
};
use serde_json::json;

fn sig(name: &str, input: serde_json::Value) -> String {
    signature(name, &input)
}

#[test]
fn a_first_call_is_fresh() {
    let mut t = ToolRepeatTracker::new();
    assert_eq!(t.observe("bash|{}"), RepeatVerdict::Fresh);
    assert_eq!(t.consecutive(), 1);
}

#[test]
fn a_different_call_resets_the_run() {
    let mut t = ToolRepeatTracker::new();
    t.observe("bash|{\"a\":1}");
    t.observe("bash|{\"a\":1}");
    assert_eq!(t.observe("bash|{\"a\":2}"), RepeatVerdict::Fresh);
    assert_eq!(t.consecutive(), 1);
}

#[test]
fn the_nudge_fires_exactly_at_the_threshold() {
    let mut t = ToolRepeatTracker::new();
    let s = "bash|{}";
    for i in 1..REPEAT_NUDGE_AT {
        let verdict = t.observe(s);
        assert!(
            !matches!(verdict, RepeatVerdict::NudgeNow(_)),
            "must not fire at {i}, below the threshold"
        );
    }
    assert_eq!(
        t.observe(s),
        RepeatVerdict::NudgeNow(REPEAT_NUDGE_AT),
        "fires on the call that reaches the threshold"
    );
}

#[test]
fn it_nudges_once_per_run_not_every_round() {
    // A model that keeps going must not be nudged on every subsequent round;
    // that would bloat the context with repeats of the same correction.
    let mut t = ToolRepeatTracker::new();
    let s = "bash|{}";
    for _ in 0..REPEAT_NUDGE_AT {
        t.observe(s);
    }
    for _ in 0..5 {
        assert!(!matches!(t.observe(s), RepeatVerdict::NudgeNow(_)));
    }
}

#[test]
fn a_new_run_can_be_nudged_again() {
    let mut t = ToolRepeatTracker::new();
    for _ in 0..REPEAT_NUDGE_AT {
        t.observe("bash|{\"a\":1}");
    }
    // Different call resets the run, then repeat it up to one short of the
    // threshold so the assertion below is the call that reaches it.
    t.observe("bash|{\"a\":2}");
    for _ in 2..REPEAT_NUDGE_AT {
        t.observe("bash|{\"a\":2}");
    }
    assert!(matches!(
        t.observe("bash|{\"a\":2}"),
        RepeatVerdict::NudgeNow(_)
    ));
}

#[test]
fn a_reset_clears_the_run_so_a_replay_cannot_inflate_it() {
    // A retry or fallback replays the failed attempt's calls. Without the
    // reset those copies stack onto the count and trip the threshold on calls
    // the model only made once.
    let mut t = ToolRepeatTracker::new();
    let s = "bash|{}";
    for _ in 1..REPEAT_NUDGE_AT {
        t.observe(s);
    }
    t.reset();
    assert_eq!(t.consecutive(), 0);
    assert_eq!(t.observe(s), RepeatVerdict::Fresh);
}

#[test]
fn argument_key_order_cannot_change_identity() {
    // serde_json here has no `preserve_order`, so its Map is a BTreeMap and
    // object keys always serialize sorted. This pins that: enabling
    // `preserve_order` later would let a model evade the guard by reordering
    // fields, and this test is what would catch it.
    let a = sig("bash", json!({"command": "ls", "cwd": "/tmp"}));
    let b = sig("bash", json!({"cwd": "/tmp", "command": "ls"}));
    assert_eq!(a, b);
}

#[test]
fn nested_object_key_order_also_cannot_change_identity() {
    let a = sig("run", json!({"opts": {"x": 1, "y": 2}}));
    let b = sig("run", json!({"opts": {"y": 2, "x": 1}}));
    assert_eq!(a, b);
}

#[test]
fn array_order_still_distinguishes_calls() {
    // Order matters in a list of arguments; canonicalizing it away would
    // merge genuinely different calls.
    let a = sig("run", json!({"args": ["a", "b"]}));
    let b = sig("run", json!({"args": ["b", "a"]}));
    assert_ne!(a, b);
}

#[test]
fn a_different_tool_with_identical_arguments_is_a_different_call() {
    assert_ne!(sig("bash", json!({"x": 1})), sig("read", json!({"x": 1})));
}

#[test]
fn an_empty_round_is_ignored() {
    // A round with no tool calls cannot be a repeat, and must not stall the
    // run counter on the empty string.
    let mut t = ToolRepeatTracker::new();
    assert!(observe_round(&mut t, "", None).is_none());
    assert_eq!(t.consecutive(), 0);
}

#[test]
fn the_correction_names_the_tool_and_offers_a_way_out() {
    let mut t = ToolRepeatTracker::new();
    let s = sig("bash", json!({"command": "ls"}));
    let mut nudge = None;
    for _ in 0..REPEAT_NUDGE_AT {
        nudge = observe_round(&mut t, &s, Some("bash".to_string()));
    }
    let nudge = nudge.expect("the threshold round yields a correction");
    assert!(nudge.contains("bash"), "names the offending tool");
    assert!(
        nudge.contains("cannot return anything different"),
        "states the mechanism rather than scolding"
    );
    assert!(
        nudge.contains("explain what you need"),
        "offers an exit that is not another tool call"
    );
}

#[test]
fn a_parallel_round_compares_as_a_unit() {
    // Rounds, not individual calls: a batch that repeats wholesale is the
    // repeat, and reordering within the batch is a different round.
    let round = [
        sig("bash", json!({"command": "ls"})),
        sig("read", json!({"path": "/tmp/x"})),
    ]
    .join("\n");
    let mut t = ToolRepeatTracker::new();
    assert_eq!(t.observe(&round), RepeatVerdict::Fresh);
    assert!(matches!(t.observe(&round), RepeatVerdict::Repeating(2)));
}
