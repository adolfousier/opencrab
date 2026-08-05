//! Tests for the collapsible Slack tool group (#373-style expand toggle).
//!
//! Covers the render modes (collapsed summary, expanded list, single-tool
//! plain line), the toggle/preservation semantics of the SlackState
//! registry, and retention pruning.

use crate::channels::slack::SlackState;
use crate::channels::slack::tool_group::{GroupEntry, GroupState, render};
use slack_morphism::prelude::{SlackChannelId, SlackTs};

fn entries(n: usize, done: bool) -> Vec<GroupEntry> {
    (0..n)
        .map(|i| GroupEntry::Tool {
            name: format!("tool{i}"),
            context: format!(" (arg{i})"),
            status: if done { Some(true) } else { None },
        })
        .collect()
}

fn group(n: usize, done: bool, expanded: bool) -> GroupState {
    GroupState {
        channel: SlackChannelId::new("C1".into()),
        entries: entries(n, done),
        expanded,
    }
}

fn text_of(content: &slack_morphism::prelude::SlackMessageContent) -> String {
    content.text.clone().unwrap_or_default()
}

#[test]
fn collapsed_group_shows_summary_only() {
    let content = render(&group(3, false, false), &SlackTs::new("1.0".into()));
    let text = text_of(&content);
    assert!(text.contains("3 tool calls"), "text: {text}");
    assert!(text.contains("running"));
    assert!(!text.contains("tool0"), "collapsed must hide tool rows");
    // Toggle button present for multi-tool groups.
    assert!(content.blocks.as_ref().is_some_and(|b| b.len() == 2));
}

#[test]
fn expanded_group_lists_every_tool() {
    let content = render(&group(3, true, true), &SlackTs::new("1.0".into()));
    let text = text_of(&content);
    assert!(text.contains("tool0") && text.contains("tool2"), "{text}");
    assert!(text.contains("✅"));
}

#[test]
fn single_tool_renders_plain_without_button() {
    let content = render(&group(1, false, false), &SlackTs::new("1.0".into()));
    let text = text_of(&content);
    assert!(text.contains("tool0"));
    assert!(!text.contains("tool call"), "no group header for one tool");
    assert!(content.blocks.as_ref().is_some_and(|b| b.len() == 1));
}

#[tokio::test]
async fn toggle_flips_and_updates_preserve_expansion() {
    let state = SlackState::new();
    state
        .upsert_tool_group("111.0".into(), group(2, false, false))
        .await;
    // User expands.
    let toggled = state.toggle_tool_group("111.0").await.expect("exists");
    assert!(toggled.expanded);
    // A later progress update (built with expanded=false) must NOT snap
    // the group shut: upsert preserves the stored expansion choice.
    let stored = state
        .upsert_tool_group("111.0".into(), group(2, true, false))
        .await;
    assert!(
        stored.expanded,
        "update must preserve user's expanded state"
    );
    assert_eq!(stored.entries.len(), 2);
    // Unknown ts: toggle is a no-op.
    assert!(state.toggle_tool_group("999.9").await.is_none());
}

#[tokio::test]
async fn retention_prunes_oldest_groups() {
    let state = SlackState::new();
    for i in 0..25 {
        state
            .upsert_tool_group(format!("{i}.0"), group(2, true, false))
            .await;
    }
    // Cap is 20: the first five aged out, the newest survive.
    assert!(state.toggle_tool_group("0.0").await.is_none());
    assert!(state.toggle_tool_group("4.0").await.is_none());
    assert!(state.toggle_tool_group("24.0").await.is_some());
}
