//! #757: a turn has no marker on DisplayMessage, so it is inferred from the
//! message list — it opens at a user message and runs to the next one.

use crate::tui::app::{DisplayMessage, ToolCallEntry, ToolCallGroup};
use crate::tui::render::chat::{turn_of, turn_ranges};
use uuid::Uuid;

fn msg(role: &str, content: &str) -> DisplayMessage {
    DisplayMessage {
        id: Uuid::new_v4(),
        role: role.to_string(),
        content: content.to_string(),
        timestamp: chrono::Utc::now(),
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: None,
        expanded: false,
        expanded_full: false,
        tool_group: None,
    }
}

fn tool_group_msg() -> DisplayMessage {
    let mut m = msg("tool_group", "1 tool call");
    m.tool_group = Some(ToolCallGroup {
        calls: vec![ToolCallEntry {
            description: "read a file".to_string(),
            success: true,
            details: None,
            completed: true,
            tool_input: serde_json::Value::Null,
        }],
        expanded: false,
    });
    m
}

#[test]
fn empty_list_has_no_turns() {
    assert!(turn_ranges(&[]).is_empty());
}

#[test]
fn a_full_turn_spans_user_through_its_whole_reply() {
    // user, thinking, tool_group, assistant  → one turn covering all four.
    let msgs = vec![
        msg("user", "do the thing"),
        msg("assistant", ""),
        tool_group_msg(),
        msg("assistant", "done"),
    ];
    let turns = turn_ranges(&msgs);
    assert_eq!(turns.len(), 1);
    assert_eq!((turns[0].start, turns[0].end), (0, 4));
    assert_eq!(turns[0].len(), 4);
}

#[test]
fn each_user_message_opens_a_new_turn() {
    let msgs = vec![
        msg("user", "first"),
        msg("assistant", "a"),
        msg("user", "second"),
        msg("assistant", "b"),
    ];
    let turns = turn_ranges(&msgs);
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].start, turns[0].end), (0, 2));
    assert_eq!((turns[1].start, turns[1].end), (2, 4));
}

#[test]
fn back_to_back_user_messages_are_separate_turns() {
    let msgs = vec![msg("user", "one"), msg("user", "two")];
    let turns = turn_ranges(&msgs);
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].start, turns[0].end), (0, 1));
    assert_eq!((turns[1].start, turns[1].end), (1, 2));
}

#[test]
fn rows_before_the_first_user_message_form_a_leading_turn() {
    // History markers / restored rows must not be dropped.
    let msgs = vec![
        msg("history_marker", "— older messages —"),
        msg("assistant", "restored"),
        msg("user", "hello"),
        msg("assistant", "hi"),
    ];
    let turns = turn_ranges(&msgs);
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].start, turns[0].end), (0, 2), "leading range kept");
    assert_eq!((turns[1].start, turns[1].end), (2, 4));
}

#[test]
fn turns_cover_every_message_without_gaps_or_overlap() {
    let msgs = vec![
        msg("system", "banner"),
        msg("user", "a"),
        msg("assistant", ""),
        tool_group_msg(),
        msg("error", "oops"),
        msg("user", "b"),
        msg("assistant", "done"),
    ];
    let turns = turn_ranges(&msgs);
    // Contiguous and complete: each turn starts where the previous ended.
    assert_eq!(turns.first().unwrap().start, 0);
    assert_eq!(turns.last().unwrap().end, msgs.len());
    for pair in turns.windows(2) {
        assert_eq!(pair[0].end, pair[1].start, "gap/overlap between turns");
    }
}

#[test]
fn turn_of_resolves_any_row_back_to_its_turn() {
    let msgs = vec![
        msg("user", "a"),
        msg("assistant", ""),
        tool_group_msg(),
        msg("user", "b"),
        msg("assistant", "done"),
    ];
    // A tool row in the middle of turn 1 resolves to turn 1.
    assert_eq!(turn_of(&msgs, 2).map(|t| (t.start, t.end)), Some((0, 3)));
    // The reply in turn 2 resolves to turn 2.
    assert_eq!(turn_of(&msgs, 4).map(|t| (t.start, t.end)), Some((3, 5)));
    // Out of range resolves to nothing.
    assert!(turn_of(&msgs, 99).is_none());
}
