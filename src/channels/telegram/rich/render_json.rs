//! AST → `InputRichMessage` JSON serializer (#420 path B).
//!
//! Serializes the schema-independent [`super::ast`] into the Bot API's
//! rich-block JSON so a rich send can carry native blocks — including
//! `RichBlockDetails` collapse (`summary` / `blocks` / `is_open`) — instead
//! of re-encoded markdown or HTML text. Every block is a type-tagged object
//! (`{"type": "details", ...}`) with the Bot API's snake_case field names.
//!
//! Not yet wired into the send path: the HTML input mode (#420 path A) ships
//! first because the server parses it into the same native blocks with zero
//! schema risk. This serializer takes over once its block schema is
//! validated live against `sendRichMessage`.
#![cfg_attr(not(test), allow(dead_code))]

use serde_json::{Value, json};

use super::ast::{Align, Block, Inline, List, Table};

/// Serialize parsed blocks into the `rich_message` value for
/// `sendRichMessage` / `editMessageText`: `{"blocks": [...]}`.
pub(crate) fn input_rich_message(blocks: &[Block]) -> Value {
    json!({ "blocks": render_blocks(blocks) })
}

fn render_blocks(blocks: &[Block]) -> Vec<Value> {
    blocks.iter().map(render_block).collect()
}

/// One block-level element as a type-tagged JSON object.
pub(crate) fn render_block(b: &Block) -> Value {
    match b {
        Block::Heading { level, content } => json!({
            "type": "heading",
            "level": level,
            "content": render_inlines(content),
        }),
        Block::Paragraph(content) => json!({
            "type": "paragraph",
            "content": render_inlines(content),
        }),
        Block::List(list) => render_list(list),
        Block::Table(table) => render_table(table),
        // `language` is null (not omitted) for untagged fences: Telegram's
        // API treats absent and null optionals identically.
        Block::Code { lang, text } => json!({
            "type": "code",
            "language": lang,
            "text": text,
        }),
        // Mermaid fences resolve to images on the HTML path; the blocks path
        // (unused for mermaid) falls back to the raw source as a code block.
        Block::Mermaid { source, .. } => json!({
            "type": "code",
            "language": "mermaid",
            "text": source,
        }),
        Block::Quote(blocks) => json!({
            "type": "quote",
            "blocks": render_blocks(blocks),
        }),
        Block::Math(text) => json!({
            "type": "math",
            "text": text,
        }),
        Block::Divider => json!({ "type": "divider" }),
        Block::Details {
            summary,
            blocks,
            open,
        } => json!({
            "type": "details",
            "summary": render_inlines(summary),
            "blocks": render_blocks(blocks),
            "is_open": open,
        }),
    }
}

fn render_list(list: &List) -> Value {
    let items: Vec<Value> = list
        .items
        .iter()
        .map(|item| {
            json!({
                // null = plain item, true/false = checked/unchecked task box.
                "task": item.task,
                "content": render_inlines(&item.content),
                "blocks": render_blocks(&item.children),
            })
        })
        .collect();
    json!({
        "type": "list",
        "ordered": list.ordered,
        "items": items,
    })
}

fn render_table(table: &Table) -> Value {
    let cells = |row: &[Vec<Inline>]| -> Vec<Value> {
        row.iter().map(|cell| json!(render_inlines(cell))).collect()
    };
    json!({
        "type": "table",
        "align": table.align.iter().map(|a| render_align(*a)).collect::<Vec<_>>(),
        "header": cells(&table.header),
        "rows": table.rows.iter().map(|r| json!(cells(r))).collect::<Vec<_>>(),
    })
}

fn render_align(align: Align) -> &'static str {
    match align {
        Align::None => "none",
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn render_inlines(inlines: &[Inline]) -> Vec<Value> {
    inlines.iter().map(render_inline).collect()
}

/// One inline-level element as a type-tagged JSON object.
pub(crate) fn render_inline(inline: &Inline) -> Value {
    match inline {
        Inline::Text(text) => json!({ "type": "text", "text": text }),
        Inline::Bold(content) => json!({ "type": "bold", "content": render_inlines(content) }),
        Inline::Italic(content) => json!({ "type": "italic", "content": render_inlines(content) }),
        Inline::Strike(content) => json!({
            "type": "strikethrough",
            "content": render_inlines(content),
        }),
        Inline::Code(text) => json!({ "type": "code", "text": text }),
        Inline::Math(text) => json!({ "type": "math", "text": text }),
        Inline::Link { content, url } => json!({
            "type": "link",
            "content": render_inlines(content),
            "url": url,
        }),
    }
}
