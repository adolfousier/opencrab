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
//! [`table`] / [`list`] / [`parse`] (front-end), [`render_html`] (fallback),
//! [`render_json`] (rich-first serializer, #420 path B).

pub(crate) mod api;
pub(crate) mod ast;
mod inline;
mod list;
mod parse;
mod render_html;
pub(crate) mod render_json;
mod table;

pub(crate) use parse::parse_markdown;
pub(crate) use table::reflow_collapsed_tables;

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

/// Whether a structured reply should be delivered as a native rich message:
/// the `channels.telegram.rich_messages` config flag is on AND the text has
/// block structure ([`has_rich_structure`]). On by default (#425); older
/// clients and Telegram Web show a "not supported" placeholder, so outdated
/// deployments opt out (onboard dialog or `richtext off`) and get the
/// universal HTML rendering. Read via the zero-disk config mirror.
pub(crate) fn should_send_native_rich(text: &str) -> bool {
    let flag = crate::config::Config::current()
        .channels
        .telegram
        .rich_messages;
    let structured = has_rich_structure(text);
    // Logged because the verdict was previously invisible: only FAILURES down
    // the rich path were recorded, so "rich was never attempted" and "rich was
    // sent and the client rendered it badly" produced identical logs. A table
    // that arrived as unformatted HTML could not be diagnosed from the log at
    // all (#860). Both inputs are recorded, not just the answer, so a false
    // verdict says which half caused it.
    tracing::info!(
        "Telegram rich verdict: {} (rich_messages={}, structured={}, table={}, len={})",
        flag && structured,
        flag,
        structured,
        contains_table(text),
        text.len()
    );
    flag && structured
}

/// Whether `text` contains block-level markdown structure that native rich
/// rendering handles meaningfully better than plain/HTML: a table, ATX
/// heading, list item, fenced code block, or block math. Plain prose (even
/// with inline emphasis) returns false, so it stays on the existing path and
/// is never reinterpreted by Telegram's markdown parser. Gates the native
/// `sendRichMessage` path (together with the config flag).
///
/// A message is NEVER disqualified from rich (the #476 fence-disqualify was
/// reverted: it dragged tables in mixed table+fence messages onto the HTML
/// path, where tables unwrap to raw pipes). Fence mangling under the rich
/// markdown parser is a separate cosmetic issue whose real fix is the
/// native-block serializer (#420 path B), not exclusion.
pub(crate) fn has_rich_structure(text: &str) -> bool {
    contains_table(text)
        || text.lines().any(|line| {
            let t = line.trim_start();
            is_atx_heading(t)
                || list::is_item(t)
                || t.starts_with("```")
                || t == "$$"
                || t == "<details>"
                || t == "<details open>"
                || t.starts_with("<details ")
        })
}

/// A `# `..`###### ` ATX heading line (1-6 hashes followed by a space).
fn is_atx_heading(t: &str) -> bool {
    let hashes = t.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && t[hashes..].starts_with(' ')
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

/// Parse `text` into the native `InputRichMessage` block value (#476 path
/// B): `{"blocks": [...]}`, sent via `api::send_rich_blocks_id`. No
/// server-side markdown re-parsing, so tables AND code fences render
/// natively with no parser to mangle them (the fence-artifact bug the
/// markdown input mode cannot avoid). Falls back to the markdown/HTML send
/// paths on any API rejection.
pub(crate) fn markdown_to_rich_blocks(text: &str) -> serde_json::Value {
    render_json::input_rich_message(&parse_markdown(text))
}

/// Parse `text` and render it as Telegram HTML in one call (the fallback path).
pub(crate) fn markdown_to_html(text: &str) -> String {
    render_html::render_html(&parse_markdown(text))
}
