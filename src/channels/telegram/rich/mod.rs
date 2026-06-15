//! Telegram rich-message support (Bot API "rich messages", 2026-06).
//!
//! Pipeline: markdown text → [`ast::Block`] AST ([`parse`]) → either Telegram's
//! `InputRichMessage` JSON (rich-first path, finalized against the Bot API
//! field schema) or Telegram HTML ([`render_html`], the fallback path).
//!
//! The AST and parser are deliberately independent of the wire schema, so the
//! markdown front-end and its tests don't churn when the serializer lands, and
//! the same AST drives both the rich and fallback renderers.
//!
//! Files are kept small and single-purpose: [`ast`] (types), [`inline`] /
//! [`table`] / [`list`] / [`parse`] (front-end), [`render_html`] (fallback).

pub(crate) mod api;
pub(crate) mod ast;
mod inline;
mod list;
mod parse;
mod render_html;
mod table;

pub(crate) use parse::parse_markdown;

/// Whether `text` is better served by the AST renderer than the legacy
/// line-based converter: it contains a GitHub-flavored table or a task-list
/// checkbox, both of which the legacy path renders poorly (raw `| pipes |` and
/// literal `- [ ]` respectively).
pub(crate) fn prefers_rich_render(text: &str) -> bool {
    contains_table(text) || contains_task_list(text)
}

/// Whether `text` contains a GitHub-flavored pipe table.
pub(crate) fn contains_table(text: &str) -> bool {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    (0..lines.len()).any(|i| table::try_parse(&lines, i).is_some())
}

/// Whether `text` contains a `- [ ]` / `- [x]` task-list item.
pub(crate) fn contains_task_list(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim_start();
        let after = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("+ "));
        matches!(after, Some(rest)
            if rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]"))
    })
}

/// Parse `text` and render it as Telegram HTML in one call (the fallback path).
pub(crate) fn markdown_to_html(text: &str) -> String {
    render_html::render_html(&parse_markdown(text))
}
