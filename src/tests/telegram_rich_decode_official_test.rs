//! Tests for the typed `RichBlock`/`RichText` rendering of received rich
//! messages (#1058).
//!
//! Telegram normalizes a rich message's source (markdown/html) into an
//! official `RichBlock[]` tree server-side; the receiving bot only ever sees
//! `blocks`. These fixtures mirror the official Bot API 10.1/10.2 shapes
//! (RichText nesting via `text`, bare string leaves, 2D `cells` tables,
//! `summary`+`blocks` details, `PhotoSize` arrays). The leaf-walk decoder
//! built in #686 dropped or glued most of these; the typed renderer must
//! surface every one, and unknown block types must never disappear silently.

use crate::channels::telegram::rich_decode::decode_rich_content;
use serde_json::json;

#[test]
fn official_paragraph_with_nested_richtext_renders_inline() {
    // Official RichText: array of runs; `bold` wraps its text in `text`.
    let raw = json!({
        "message_id": 1,
        "rich_message": { "blocks": [
            { "type": "paragraph", "text": [
                "Deploy finished: ",
                { "type": "bold", "text": "all green" },
                { "type": "italic", "text": " (finally)" }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert_eq!(out.trim(), "Deploy finished: all green (finally)");
}

#[test]
fn official_bare_string_paragraph_renders() {
    // RichText also allows a bare string leaf.
    let raw = json!({
        "message_id": 2,
        "rich_message": { "blocks": [
            { "type": "paragraph", "text": "Cek dulu, jangan ngomong doang." }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("Cek dulu, jangan ngomong doang."));
}

#[test]
fn official_table_renders_pipe_grid() {
    // Official `cells`: 2D array; each cell wraps content in `content`.
    let raw = json!({
        "message_id": 3,
        "rich_message": { "blocks": [
            { "type": "table", "is_bordered": true, "cells": [
                [
                    { "content": { "type": "paragraph", "text": "Stage" } },
                    { "content": { "type": "paragraph", "text": "Status" } }
                ],
                [
                    { "content": { "type": "paragraph", "text": "build" } },
                    { "content": { "type": "paragraph", "text": "ok" } }
                ],
                [
                    { "content": { "type": "paragraph", "text": "test" } },
                    { "content": { "type": "paragraph", "text": "ok" } }
                ]
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "Stage | Status");
    assert_eq!(lines[1], "build | ok");
    assert_eq!(lines[2], "test | ok");
}

#[test]
fn legacy_rows_table_also_gets_separators() {
    // Our pre-#1058 own-AST shape: rows[].cells[] with text leaves. The old
    // walker glued these into one blob; separators now apply to both shapes.
    let raw = json!({
        "message_id": 4,
        "rich_message": { "blocks": [
            { "type": "table", "rows": [
                { "cells": [ {"text": "A"}, {"text": "B"} ] },
                { "cells": [ {"text": "1"}, {"text": "2"} ] }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("A | B"));
    assert!(out.contains("1 | 2"));
}

#[test]
fn details_summary_and_content_both_render() {
    // The #1058 live repro: a peer-crab report whose collapsed details block
    // arrived as summary + nested blocks. The old walker kept only text that
    // happened to sit in recognized keys; the summary's sibling content was
    // dropped entirely.
    let raw = json!({
        "message_id": 5,
        "rich_message": { "blocks": [
            { "type": "paragraph", "text": "Cek dulu — last turn gue janji save lesson." },
            { "type": "details", "is_open": false, "summary": "13 tool calls", "blocks": [
                { "type": "paragraph", "text": "memory write deferred" },
                { "type": "paragraph", "text": "retry queued" }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("Cek dulu"), "outer paragraph survives: {out}");
    assert!(
        out.contains("[details: 13 tool calls]"),
        "summary renders: {out}"
    );
    assert!(
        out.contains("memory write deferred"),
        "collapsed content renders: {out}"
    );
    assert!(
        out.contains("retry queued"),
        "second nested block renders: {out}"
    );
}

#[test]
fn list_items_with_labels_render_as_lines() {
    let raw = json!({
        "message_id": 6,
        "rich_message": { "blocks": [
            { "type": "list", "items": [
                { "label": "build", "content": [ { "type": "text", "text": "ok" } ] },
                { "label": "test", "content": [ { "type": "text", "text": "2 failed" } ] }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("- build: ok"), "labeled item: {out}");
    assert!(out.contains("- test: 2 failed"), "labeled item: {out}");
}

#[test]
fn ordered_list_numbers_items() {
    let raw = json!({
        "message_id": 7,
        "rich_message": { "blocks": [
            { "type": "list", "is_ordered": true, "items": [
                { "content": [ { "type": "text", "text": "probe" } ] },
                { "content": [ { "type": "text", "text": "rebuild" } ] }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("1. probe"), "{out}");
    assert!(out.contains("2. rebuild"), "{out}");
}

#[test]
fn math_block_renders_expression() {
    let raw = json!({
        "message_id": 8,
        "rich_message": { "blocks": [
            { "type": "math", "expression": "E = mc^2" }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("[math: E = mc^2]"));
}

#[test]
fn photo_block_surfaces_file_id_and_caption() {
    let raw = json!({
        "message_id": 9,
        "rich_message": { "blocks": [
            { "type": "photo", "has_spoiler": false,
              "photo": [
                { "file_id": "small", "width": 160, "height": 160 },
                { "file_id": "AgACfullres", "width": 1280, "height": 720 }
              ],
              "caption": [ "the render " , { "type": "bold", "text": "after fix" } ]
            }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("photo attached"), "{out}");
    assert!(
        out.contains("AgACfullres"),
        "largest-size file_id surfaced: {out}"
    );
    assert!(
        out.contains("the render after fix"),
        "caption text renders: {out}"
    );
}

#[test]
fn unknown_block_type_never_disappears_silently() {
    let raw = json!({
        "message_id": 10,
        "rich_message": { "blocks": [
            { "type": "hologram", "data": "beep" }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("[unsupported rich block: hologram]"), "{out}");
}

#[test]
fn text_link_appends_url() {
    let raw = json!({
        "message_id": 11,
        "rich_message": { "blocks": [
            { "type": "paragraph", "text": [
                "see ",
                { "type": "text_link", "text": "the docs", "url": "https://docs.opencrabs.com" }
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(
        out.contains("the docs (https://docs.opencrabs.com)"),
        "{out}"
    );
}

#[test]
fn heading_with_level_gets_markers() {
    let raw = json!({
        "message_id": 12,
        "rich_message": { "blocks": [
            { "type": "heading", "level": 3, "text": "Status" }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("### Status"), "{out}");
}

#[test]
fn embed_url_without_text_renders_marker() {
    let raw = json!({
        "message_id": 13,
        "rich_message": { "blocks": [
            { "type": "embed", "url": "https://example.com/x" }
        ]}
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("[embed: https://example.com/x]"), "{out}");
}
