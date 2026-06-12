//! Regression: `<<IMG:path>>` markers must become PLAIN-TEXT hints, never inline
//! `image_url` (multimodal) content blocks.
//!
//! Inlining the base64 image attached an `image_url` content block to the user
//! message that rode along to EVERY provider. Vision-capable models accept it,
//! but text-only fallbacks (zhipu/glm) reject non-text content with
//! `400 messages.content.type is invalid, allowed values: ['text']`, which broke
//! the fallback chain whenever an image message reached a text-only provider
//! (Telegram, 2026-06-12). The agent views images via `analyze_image` instead,
//! so the user message must stay text-only.

use crate::brain::agent::AgentService;
use crate::brain::provider::{ContentBlock, Message};

fn joined_text(msg: &Message) -> String {
    msg.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn image_marker_becomes_text_hint_not_image_block() {
    let msg = AgentService::build_user_message("look at this <<IMG:/tmp/x/photo.jpg>> what is it?");

    // EVERY content block must be text — no image_url / ContentBlock::Image.
    assert!(
        msg.content
            .iter()
            .all(|b| matches!(b, ContentBlock::Text { .. })),
        "image markers must not produce image content blocks (would 400 text-only providers)"
    );

    let text = joined_text(&msg);
    assert!(
        text.contains("[image attached: /tmp/x/photo.jpg]"),
        "the path hint must survive so the agent can call analyze_image; got: {text}"
    );
    assert!(
        !text.contains("<<IMG:"),
        "the raw marker must be replaced; got: {text}"
    );
}

#[test]
fn multiple_image_markers_all_become_hints() {
    let msg =
        AgentService::build_user_message("<<IMG:/a/one.png>> and <<IMG:https://x/two.jpg>> done");
    assert!(
        msg.content
            .iter()
            .all(|b| matches!(b, ContentBlock::Text { .. })),
        "no image blocks for any marker"
    );
    let text = joined_text(&msg);
    assert!(text.contains("[image attached: /a/one.png]"));
    assert!(text.contains("[image attached: https://x/two.jpg]"));
    assert!(!text.contains("<<IMG:"));
}

#[test]
fn plain_text_passes_through_unchanged() {
    let msg = AgentService::build_user_message("just text, no image");
    assert_eq!(joined_text(&msg), "just text, no image");
}
