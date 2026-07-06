//! Tests for the AST → `InputRichMessage` JSON serializer (#420 path B,
//! `rich/render_json.rs`). Pins the wire shape: every block and inline is a
//! type-tagged object with the Bot API's snake_case field names, and
//! `Block::Details` maps to RichBlockDetails (`summary` / `blocks` /
//! `is_open`) — the native collapse the markdown input mode cannot express.

use crate::channels::telegram::rich::parse_markdown;
use crate::channels::telegram::rich::render_json::{input_rich_message, render_block};

// ── top-level envelope ───────────────────────────────────────────────

#[test]
fn envelope_wraps_blocks_array() {
    let blocks = parse_markdown("hello **world**");
    let v = input_rich_message(&blocks);
    let arr = v["blocks"].as_array().expect("blocks array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["type"], "paragraph");
}

// ── details → RichBlockDetails ───────────────────────────────────────

#[test]
fn details_block_serializes_to_rich_block_details() {
    let blocks =
        parse_markdown("<details>\n<summary>3 tool calls</summary>\n\nlog line\n\n</details>");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "details");
    assert_eq!(v["is_open"], false);
    assert_eq!(v["summary"][0]["type"], "text");
    assert_eq!(v["summary"][0]["text"], "3 tool calls");
    assert_eq!(v["blocks"][0]["type"], "paragraph");
    assert_eq!(v["blocks"][0]["content"][0]["text"], "log line");
}

#[test]
fn details_open_attribute_maps_to_is_open_true() {
    let blocks = parse_markdown("<details open>\n<summary>s</summary>\n\nbody\n\n</details>");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "details");
    assert_eq!(v["is_open"], true);
}

// ── block coverage ───────────────────────────────────────────────────

#[test]
fn heading_carries_level_and_content() {
    let blocks = parse_markdown("## Title");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "heading");
    assert_eq!(v["level"], 2);
    assert_eq!(v["content"][0]["text"], "Title");
}

#[test]
fn code_block_carries_language_and_literal_text() {
    let blocks = parse_markdown("```rust\nfn main() {}\n```");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "code");
    assert_eq!(v["language"], "rust");
    assert_eq!(v["text"], "fn main() {}");
}

#[test]
fn untagged_code_block_has_null_language() {
    let blocks = parse_markdown("```\nplain\n```");
    let v = render_block(&blocks[0]);
    assert!(v["language"].is_null());
}

#[test]
fn task_list_items_carry_task_state_and_nesting() {
    let blocks = parse_markdown("- [x] done\n- [ ] todo\n- plain");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "list");
    assert_eq!(v["ordered"], false);
    let items = v["items"].as_array().expect("items");
    assert_eq!(items[0]["task"], true);
    assert_eq!(items[1]["task"], false);
    assert!(items[2]["task"].is_null());
    assert_eq!(items[0]["content"][0]["text"], "done");
}

#[test]
fn table_serializes_align_header_rows() {
    let blocks = parse_markdown("| A | B |\n| :- | -: |\n| 1 | 2 |");
    let v = render_block(&blocks[0]);
    assert_eq!(v["type"], "table");
    assert_eq!(v["align"][0], "left");
    assert_eq!(v["align"][1], "right");
    assert_eq!(v["header"][0][0]["text"], "A");
    assert_eq!(v["rows"][0][1][0]["text"], "2");
}

#[test]
fn quote_divider_math_serialize() {
    let blocks = parse_markdown("> quoted\n\n---\n\n$$\nE = mc^2\n$$");
    assert_eq!(render_block(&blocks[0])["type"], "quote");
    assert_eq!(render_block(&blocks[1])["type"], "divider");
    let math = render_block(&blocks[2]);
    assert_eq!(math["type"], "math");
    assert_eq!(math["text"], "E = mc^2");
}

// ── inline coverage ──────────────────────────────────────────────────

#[test]
fn inline_styles_nest_as_typed_content() {
    let blocks = parse_markdown("**bold _italic_** ~~gone~~ `lit` [link](https://x.example)");
    let v = render_block(&blocks[0]);
    let content = v["content"].as_array().expect("inline content");
    let bold = &content[0];
    assert_eq!(bold["type"], "bold");
    assert!(
        bold["content"]
            .as_array()
            .expect("bold content")
            .iter()
            .any(|i| i["type"] == "italic")
    );
    assert!(content.iter().any(|i| i["type"] == "strikethrough"));
    assert!(
        content
            .iter()
            .any(|i| i["type"] == "code" && i["text"] == "lit")
    );
    assert!(
        content
            .iter()
            .any(|i| i["type"] == "link" && i["url"] == "https://x.example")
    );
}
