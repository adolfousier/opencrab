//! Block Kit builders for Slack completion delivery (#455).
//!
//! A plain `with_text` post renders a structured answer as one flat wall.
//! Splitting the (already mrkdwn-converted) text into Block Kit blocks —
//! sections for paragraphs, dividers for horizontal rules, a context block
//! for the ctx footer (#457) — gives real visual structure with the same
//! content. The caller keeps the plain text as the notification fallback
//! and retries text-only when a blocks post is rejected, so delivery never
//! regresses on a Block Kit error.

use slack_morphism::prelude::*;

/// Slack rejects section text over 3000 chars.
const SECTION_LIMIT: usize = 3000;

/// Split mrkdwn `text` into Block Kit blocks: `---` lines outside code
/// fences become dividers, everything between them becomes mrkdwn section
/// blocks capped at [`SECTION_LIMIT`] (split on paragraph boundaries when
/// possible). Returns an empty vec for blank input.
pub(crate) fn blocks_from_mrkdwn(text: &str) -> Vec<SlackBlock> {
    let mut blocks: Vec<SlackBlock> = Vec::new();
    let mut segment = String::new();
    let mut in_fence = false;

    let flush = |segment: &mut String, blocks: &mut Vec<SlackBlock>| {
        let trimmed = segment.trim();
        if !trimmed.is_empty() {
            for piece in split_to_limit(trimmed) {
                blocks.push(section(&piece));
            }
        }
        segment.clear();
    };

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && is_rule(line) {
            flush(&mut segment, &mut blocks);
            blocks.push(SlackBlock::Divider(SlackDividerBlock::new()));
            continue;
        }
        segment.push_str(line);
        segment.push('\n');
    }
    flush(&mut segment, &mut blocks);
    blocks
}

/// The ctx budget footer as a context block (#457): Slack renders context
/// elements in small grey type, visually separated from the body — the
/// native equivalent of Telegram's italic footer. Italic mrkdwn inside for
/// full parity.
pub(crate) fn context_footer(footer: &str) -> SlackBlock {
    SlackBlock::Context(SlackContextBlock::new(vec![
        SlackContextBlockElement::MarkDown(SlackBlockMarkDownText::new(format!("_{footer}_"))),
    ]))
}

/// A markdown horizontal rule the mrkdwn converter leaves as-is.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*'))
}

fn section(text: &str) -> SlackBlock {
    SlackBlock::Section(SlackSectionBlock::new().with_text(SlackBlockText::MarkDown(
        SlackBlockMarkDownText::new(text.to_string()),
    )))
}

/// Split a segment into `SECTION_LIMIT`-sized pieces, preferring paragraph
/// boundaries (`\n\n`) and falling back to a hard char-boundary split for a
/// single oversized paragraph.
fn split_to_limit(text: &str) -> Vec<String> {
    if text.chars().count() <= SECTION_LIMIT {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        let candidate_len = if current.is_empty() {
            para.chars().count()
        } else {
            current.chars().count() + 2 + para.chars().count()
        };
        if candidate_len > SECTION_LIMIT && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        if para.chars().count() > SECTION_LIMIT {
            // Single paragraph over the limit: hard-split at char boundaries.
            let chars: Vec<char> = para.chars().collect();
            for chunk in chars.chunks(SECTION_LIMIT) {
                out.push(chunk.iter().collect());
            }
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
