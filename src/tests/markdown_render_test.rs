//! Tests for the TUI markdown renderer's inline emphasis, lists, and polish.
//!
//! These pin the elements that previously rendered flat: bold/italic were
//! no-ops, bullet/ordered lists had no markers, and links dropped their URL.
//! Each test parses a snippet and asserts the resulting spans carry the right
//! style or marker text.

use crate::tui::markdown::parse_markdown;
use ratatui::style::Modifier;
use ratatui::text::Line;

/// Flatten a parsed line into its plain text.
fn line_text(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Find the first span whose content equals `needle`, returning its modifier.
fn modifier_of(lines: &[Line<'static>], needle: &str) -> Option<Modifier> {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.as_ref() == needle)
        .map(|s| s.style.add_modifier)
}

#[test]
fn bold_text_carries_bold_modifier() {
    let lines = parse_markdown("Some **bold** word", 80);
    let m = modifier_of(&lines, "bold").expect("a 'bold' span must exist");
    assert!(
        m.contains(Modifier::BOLD),
        "**bold** must render with the BOLD modifier, got {m:?}"
    );
}

#[test]
fn italic_text_carries_italic_modifier() {
    let lines = parse_markdown("an *emphasised* bit", 80);
    let m = modifier_of(&lines, "emphasised").expect("an 'emphasised' span must exist");
    assert!(
        m.contains(Modifier::ITALIC),
        "*italic* must render with the ITALIC modifier, got {m:?}"
    );
}

#[test]
fn nested_bold_italic_composes_both_modifiers() {
    // Bold containing italic — the inner text must carry BOTH weights.
    let lines = parse_markdown("**bold _and italic_**", 80);
    let m = modifier_of(&lines, "and italic").expect("inner span must exist");
    assert!(
        m.contains(Modifier::BOLD) && m.contains(Modifier::ITALIC),
        "nested emphasis must compose BOLD|ITALIC, got {m:?}"
    );
}

#[test]
fn strikethrough_carries_crossed_out() {
    let lines = parse_markdown("this is ~~gone~~ now", 80);
    let m = modifier_of(&lines, "gone").expect("a 'gone' span must exist");
    assert!(
        m.contains(Modifier::CROSSED_OUT),
        "~~strike~~ must render CROSSED_OUT, got {m:?}"
    );
}

#[test]
fn unordered_list_items_get_bullet_markers() {
    let lines = parse_markdown("- first\n- second", 80);
    let bullets = lines
        .iter()
        .filter(|l| line_text(l).starts_with("• "))
        .count();
    assert!(
        bullets >= 2,
        "both bullet items must start with '• ', lines: {:?}",
        lines.iter().map(line_text).collect::<Vec<_>>()
    );
}

#[test]
fn ordered_list_items_get_numbered_markers() {
    let lines = parse_markdown("1. one\n2. two\n3. three", 80);
    let texts: Vec<String> = lines.iter().map(line_text).collect();
    assert!(
        texts.iter().any(|t| t.starts_with("1. one")),
        "first item must be '1. one', got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("2. two")),
        "second item must be '2. two', got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("3. three")),
        "third item must be '3. three', got {texts:?}"
    );
}

#[test]
fn nested_list_is_indented() {
    let lines = parse_markdown("- top\n  - nested", 80);
    let texts: Vec<String> = lines.iter().map(line_text).collect();
    assert!(
        texts.iter().any(|t| t.starts_with("  • nested")),
        "nested item must be indented under its parent, got {texts:?}"
    );
}

#[test]
fn link_text_is_underlined_and_url_surfaced() {
    let lines = parse_markdown("see [the docs](https://example.com/x) now", 80);
    let m = modifier_of(&lines, "the docs").expect("link text span must exist");
    assert!(
        m.contains(Modifier::UNDERLINED),
        "link text must be underlined, got {m:?}"
    );
    let all: String = lines.iter().map(line_text).collect();
    assert!(
        all.contains("https://example.com/x"),
        "the link destination must be surfaced, got: {all}"
    );
}

#[test]
fn task_list_renders_checkboxes() {
    let lines = parse_markdown("- [x] done\n- [ ] todo", 80);
    let all: String = lines.iter().map(line_text).collect();
    assert!(
        all.contains("[x]"),
        "checked task must show [x], got: {all}"
    );
    assert!(
        all.contains("[ ]"),
        "unchecked task must show [ ], got: {all}"
    );
}

#[test]
fn blockquote_gets_a_gutter() {
    let lines = parse_markdown("> quoted wisdom", 80);
    let texts: Vec<String> = lines.iter().map(line_text).collect();
    assert!(
        texts.iter().any(|t| t.starts_with("▌ ")),
        "blockquote must render a gutter, got {texts:?}"
    );
}
