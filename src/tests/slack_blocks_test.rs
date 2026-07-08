//! Tests for the Slack Block Kit builders (#455, `channels/slack/blocks.rs`):
//! mrkdwn text splits into section blocks with dividers for horizontal rules,
//! fence-aware, every section under Slack's 3000-char limit.

use crate::channels::slack::blocks::blocks_from_mrkdwn;
use slack_morphism::prelude::*;

fn section_text(b: &SlackBlock) -> Option<String> {
    match b {
        SlackBlock::Section(s) => match &s.text {
            Some(SlackBlockText::MarkDown(md)) => Some(md.text.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn plain_paragraph_is_one_mrkdwn_section() {
    let blocks = blocks_from_mrkdwn("hello *world*");
    assert_eq!(blocks.len(), 1);
    assert_eq!(section_text(&blocks[0]).as_deref(), Some("hello *world*"));
}

#[test]
fn empty_input_yields_no_blocks() {
    assert!(blocks_from_mrkdwn("").is_empty());
    assert!(blocks_from_mrkdwn("   \n  \n").is_empty());
}

#[test]
fn rule_becomes_divider_between_sections() {
    let blocks = blocks_from_mrkdwn("first part\n\n---\n\nsecond part");
    assert_eq!(blocks.len(), 3);
    assert_eq!(section_text(&blocks[0]).as_deref(), Some("first part"));
    assert!(matches!(blocks[1], SlackBlock::Divider(_)));
    assert_eq!(section_text(&blocks[2]).as_deref(), Some("second part"));
}

#[test]
fn rule_inside_code_fence_stays_literal() {
    let text = "before\n```\n---\ncode line\n```\nafter";
    let blocks = blocks_from_mrkdwn(text);
    // No divider: the --- lives inside the fence and stays in the section.
    assert!(
        blocks.iter().all(|b| !matches!(b, SlackBlock::Divider(_))),
        "fence-guarded rule must not split: {blocks:?}"
    );
    let joined: String = blocks.iter().filter_map(section_text).collect();
    assert!(joined.contains("---"));
    assert!(joined.contains("code line"));
}

#[test]
fn long_text_splits_on_paragraph_boundaries_under_limit() {
    // Two paragraphs of ~1800 chars each: together over 3000, so they must
    // land in separate sections, split at the paragraph boundary.
    let para_a = "a".repeat(1800);
    let para_b = "b".repeat(1800);
    let blocks = blocks_from_mrkdwn(&format!("{para_a}\n\n{para_b}"));
    assert_eq!(blocks.len(), 2);
    for b in &blocks {
        let text = section_text(b).expect("section");
        assert!(text.chars().count() <= 3000, "section over limit");
    }
    assert!(section_text(&blocks[0]).unwrap().starts_with('a'));
    assert!(section_text(&blocks[1]).unwrap().starts_with('b'));
}

#[test]
fn single_oversized_paragraph_hard_splits() {
    let huge = "x".repeat(6500);
    let blocks = blocks_from_mrkdwn(&huge);
    assert_eq!(blocks.len(), 3);
    for b in &blocks {
        assert!(section_text(b).expect("section").chars().count() <= 3000);
    }
}

// ── ctx footer context block (#457) ─────────────────────────────────

#[test]
fn context_footer_is_small_grey_context_block_with_italics() {
    let block = crate::channels::slack::blocks::context_footer("ctx: 48K/200K 24% | 52 tok/s");
    let SlackBlock::Context(ctx) = block else {
        panic!("footer must be a context block");
    };
    assert_eq!(ctx.elements.len(), 1);
    let SlackContextBlockElement::MarkDown(md) = &ctx.elements[0] else {
        panic!("footer element must be mrkdwn");
    };
    assert_eq!(md.text, "_ctx: 48K/200K 24% | 52 tok/s_");
}
