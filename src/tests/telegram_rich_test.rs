//! Additional coverage for the rich module gate functions and the
//! last-intermediate footer path added in PR #202.
//!
//! The existing `telegram_rich_parse_test` already covers the pure gate
//! functions (`has_rich_structure`, `contains_table`, `prefers_rich_render`,
//! `contains_task_list`) and `telegram_last_intermediate_footer_test` covers
//! `build_last_intermediate_with_footer`. This file adds edge-case coverage
//! for the new `edit_rich_markdown` and last-intermediate footer
//! integration points.

use crate::channels::telegram::rich::api::{build_body, build_body_blocks, build_body_html};
use crate::channels::telegram::rich::{contains_task_list, has_rich_structure};
use teloxide::types::{MessageId, ThreadId};

// ── additional gate-function edge cases ─────────────────────────────

#[test]
fn task_list_asterisk_prefix_detected() {
    assert!(contains_task_list("* [ ] star task\n* [x] done"));
}

#[test]
fn task_list_plus_prefix_detected() {
    assert!(contains_task_list("+ [ ] plus task"));
}

#[test]
fn task_list_mixed_prefixes() {
    assert!(contains_task_list("- [ ] dash\n* [x] star\n+ [ ] plus"));
}

#[test]
fn task_list_indented_detected() {
    // Indented task items still start with `- ` after trimming.
    assert!(contains_task_list("  - [ ] indented"));
}

#[test]
fn rich_structure_ordered_list_detected() {
    assert!(has_rich_structure("1. first\n2. second\n3. third"));
}

#[test]
fn rich_structure_empty_text_false() {
    assert!(!has_rich_structure(""));
}

#[test]
fn rich_structure_whitespace_only_false() {
    assert!(!has_rich_structure("   \n  \n  "));
}

// ── edit_rich_markdown request body ─────────────────────────────────

#[test]
fn rich_request_body_with_thread_id() {
    let body = build_body(99, Some(ThreadId(MessageId(42))), "| A |\n| - |\n| 1 |");
    assert_eq!(body["chat_id"], 99);
    assert_eq!(body["message_thread_id"], 42);
    assert_eq!(body["rich_message"]["markdown"], "| A |\n| - |\n| 1 |");
}

#[test]
fn rich_request_body_without_thread_id() {
    let body = build_body(100, None, "hello");
    assert_eq!(body["chat_id"], 100);
    assert!(body.get("message_thread_id").is_none());
    assert_eq!(body["rich_message"]["markdown"], "hello");
}

// ── HTML input mode (#420 path A) ────────────────────────────────────

#[test]
fn rich_html_body_uses_html_key() {
    // The html input key is what lets <details><summary> become a native
    // RichBlockDetails collapsible; the markdown key cannot express it.
    let body = build_body_html(100, None, "<details><summary>x</summary>y</details>");
    assert_eq!(body["chat_id"], 100);
    assert!(body.get("message_thread_id").is_none());
    assert!(body["rich_message"].get("markdown").is_none());
    assert_eq!(
        body["rich_message"]["html"],
        "<details><summary>x</summary>y</details>"
    );
}

#[test]
fn rich_html_body_with_thread_id() {
    let body = build_body_html(99, Some(ThreadId(MessageId(42))), "<b>hi</b>");
    assert_eq!(body["message_thread_id"], 42);
    assert_eq!(body["rich_message"]["html"], "<b>hi</b>");
}

// ── native block input (#476 path B) ─────────────────────────────────

#[test]
fn rich_block_body_passes_rich_message_through() {
    // The block value is the InputRichMessage `{"blocks": [...]}`, sent
    // as-is so the server does no markdown re-parsing (tables and fences
    // both render natively, no fence-artifact mangling).
    let rich = serde_json::json!({
        "blocks": [
            {"type": "table", "align": ["left"], "header": [[{"type": "text", "text": "A"}]], "rows": []},
            {"type": "code", "language": "rust", "text": "fn main() {}"}
        ]
    });
    let body = build_body_blocks(100, None, &rich);
    assert_eq!(body["chat_id"], 100);
    assert!(body.get("message_thread_id").is_none());
    // Neither markdown nor html keys: the blocks ARE the rich_message.
    assert!(body["rich_message"].get("markdown").is_none());
    assert!(body["rich_message"].get("html").is_none());
    assert_eq!(body["rich_message"]["blocks"][1]["type"], "code");
    assert_eq!(body["rich_message"]["blocks"][1]["text"], "fn main() {}");
}

#[test]
fn rich_block_body_with_thread_id() {
    let rich = serde_json::json!({ "blocks": [] });
    let body = build_body_blocks(99, Some(ThreadId(MessageId(42))), &rich);
    assert_eq!(body["message_thread_id"], 42);
}

#[test]
fn markdown_to_rich_blocks_serializes_table_and_fence() {
    // The end-to-end helper: a message with BOTH a table and a code fence
    // (the #476 case that mangled under markdown input) serializes to
    // native blocks for each.
    let v = crate::channels::telegram::rich::markdown_to_rich_blocks(
        "| A | B |\n| - | - |\n| 1 | 2 |\n\n```rust\nfn x() {}\n```",
    );
    let blocks = v["blocks"].as_array().expect("blocks");
    assert!(blocks.iter().any(|b| b["type"] == "table"));
    assert!(
        blocks
            .iter()
            .any(|b| b["type"] == "code" && b["text"] == "fn x() {}")
    );
}
