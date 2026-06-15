//! Tests for the Telegram rich-message markdown front-end: the schema-
//! independent AST parser ([`parse_markdown`]) and the HTML fallback renderer
//! ([`markdown_to_html`]). The rich-first `InputRichMessage` serializer is
//! finalized separately against the Bot API field schema and tested there.

use crate::channels::telegram::rich::ast::{Align, Block, Inline};
use crate::channels::telegram::rich::{
    contains_table, markdown_to_html, parse_markdown, prefers_rich_render,
};

fn text(s: &str) -> Inline {
    Inline::Text(s.to_string())
}

// ── inline parsing ──────────────────────────────────────────────────

#[test]
fn inline_bold_italic_code_link() {
    let blocks = parse_markdown("a **b** _c_ `d` [e](http://x)");
    let Block::Paragraph(inl) = &blocks[0] else {
        panic!("expected paragraph, got {:?}", blocks[0]);
    };
    assert_eq!(
        inl,
        &vec![
            text("a "),
            Inline::Bold(vec![text("b")]),
            text(" "),
            Inline::Italic(vec![text("c")]),
            text(" "),
            Inline::Code("d".to_string()),
            text(" "),
            Inline::Link {
                content: vec![text("e")],
                url: "http://x".to_string(),
            },
        ]
    );
}

#[test]
fn unbalanced_delimiters_stay_literal() {
    let blocks = parse_markdown("a **b c");
    let Block::Paragraph(inl) = &blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(inl, &vec![text("a **b c")]);
}

#[test]
fn inline_code_is_not_reparsed() {
    let blocks = parse_markdown("`**not bold**`");
    let Block::Paragraph(inl) = &blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(inl, &vec![Inline::Code("**not bold**".to_string())]);
}

// ── headings ────────────────────────────────────────────────────────

#[test]
fn atx_headings_carry_level() {
    let blocks = parse_markdown("# Title\n\n### Sub");
    assert_eq!(
        blocks,
        vec![
            Block::Heading {
                level: 1,
                content: vec![text("Title")],
            },
            Block::Heading {
                level: 3,
                content: vec![text("Sub")],
            },
        ]
    );
}

#[test]
fn hash_without_space_is_not_a_heading() {
    let blocks = parse_markdown("#hashtag");
    assert_eq!(blocks, vec![Block::Paragraph(vec![text("#hashtag")])]);
}

// ── lists (nesting + ordered + tasks) ───────────────────────────────

#[test]
fn nested_bullet_list() {
    let blocks = parse_markdown("- a\n- b\n  - b1\n  - b2\n- c");
    let Block::List(list) = &blocks[0] else {
        panic!("expected list, got {:?}", blocks[0]);
    };
    assert!(!list.ordered);
    assert_eq!(list.items.len(), 3);
    // Second item carries a nested list of two children.
    let Block::List(child) = &list.items[1].children[0] else {
        panic!("expected nested list under item b");
    };
    assert_eq!(child.items.len(), 2);
    assert_eq!(child.items[0].content, vec![text("b1")]);
}

#[test]
fn ordered_list_is_ordered() {
    let blocks = parse_markdown("1. one\n2. two");
    let Block::List(list) = &blocks[0] else {
        panic!("expected list");
    };
    assert!(list.ordered);
    assert_eq!(list.items.len(), 2);
}

#[test]
fn task_list_checkboxes() {
    let blocks = parse_markdown("- [ ] todo\n- [x] done");
    let Block::List(list) = &blocks[0] else {
        panic!("expected list");
    };
    assert_eq!(list.items[0].task, Some(false));
    assert_eq!(list.items[1].task, Some(true));
    assert_eq!(list.items[1].content, vec![text("done")]);
}

// ── tables ──────────────────────────────────────────────────────────

#[test]
fn pipe_table_with_alignment() {
    let md = "| Name | Qty |\n| :--- | ---: |\n| Apple | 3 |\n| Pear | 12 |";
    let blocks = parse_markdown(md);
    let Block::Table(t) = &blocks[0] else {
        panic!("expected table, got {:?}", blocks[0]);
    };
    assert_eq!(t.header.len(), 2);
    assert_eq!(t.align, vec![Align::Left, Align::Right]);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0][0], vec![text("Apple")]);
}

#[test]
fn contains_table_detects_only_real_tables() {
    assert!(contains_table("| a | b |\n| - | - |\n| 1 | 2 |"));
    // A lone pipe line without a separator is not a table.
    assert!(!contains_table("a | b is just prose"));
    assert!(!contains_table("# heading\n\nsome text"));
}

#[test]
fn prefers_rich_render_for_tables_and_task_lists() {
    assert!(prefers_rich_render("| a | b |\n| - | - |\n| 1 | 2 |"));
    assert!(prefers_rich_render("- [ ] todo\n- [x] done"));
    assert!(prefers_rich_render("  * [x] indented task"));
    // Plain prose and ordinary bullet lists stay on the legacy path.
    assert!(!prefers_rich_render(
        "# heading\n\n- a normal bullet\n- another"
    ));
    assert!(!prefers_rich_render(
        "just a sentence with [brackets] in it"
    ));
}

// ── HTML fallback rendering ─────────────────────────────────────────

#[test]
fn table_renders_as_aligned_pre_grid() {
    let md = "| A | B |\n| - | - |\n| 1 | 22 |";
    let html = markdown_to_html(md);
    assert!(
        html.starts_with("<pre>"),
        "table must be a <pre> block: {html}"
    );
    // Header's second column is padded to the width of the widest cell ("22").
    assert!(html.contains("A | B "), "header row not padded: {html}");
    assert!(html.contains("1 | 22"), "data row missing: {html}");
}

#[test]
fn heading_renders_bold() {
    assert_eq!(markdown_to_html("# Hi"), "<b>Hi</b>");
    assert_eq!(markdown_to_html("### Deep"), "<b><i>Deep</i></b>");
}

#[test]
fn html_special_chars_are_escaped() {
    let html = markdown_to_html("a < b & c > d");
    assert_eq!(html, "a &lt; b &amp; c &gt; d");
}

#[test]
fn task_list_renders_checkboxes() {
    let html = markdown_to_html("- [ ] todo\n- [x] done");
    assert!(html.contains("☐ todo"), "{html}");
    assert!(html.contains("☑ done"), "{html}");
}
