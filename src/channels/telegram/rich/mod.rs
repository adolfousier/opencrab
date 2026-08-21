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
pub(crate) mod mermaid;
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

/// Parse `text` and render it as Telegram HTML in one call (the fallback path).
pub(crate) fn markdown_to_html(text: &str) -> String {
    render_html::render_html(&parse_markdown(text))
}

/// Like [`markdown_to_html`] but wraps paragraphs in `<p>` tags. Used by the
/// chrome_rich path where Telegram expects native paragraph elements.
pub(crate) fn markdown_to_html_p(text: &str) -> String {
    render_html::render_html_p(&parse_markdown(text))
}

/// Parse `text`, resolve any mermaid fences to rendered images (or legible
/// failure blocks) via [`mermaid::resolve_blocks`], and render the result as
/// Telegram HTML. Async because mermaid pre-validation makes an HTTP call to
/// the renderer. This is the FALLBACK path, used when the primary
/// markdown+media send fails (e.g. a Bot API server < 10.2 without the
/// `media` field); the primary path keeps tables native via the markdown
/// dialect's `media` array (#1044).
///
/// Gated on [`mermaid::should_render_mermaid`]: when the feature is off or
/// the text has no mermaid fence, no HTTP is made and the output equals
/// [`markdown_to_html`]. Every chrome surface that can embed images renders
/// prose through this pair (or the `_p` variant) so a mermaid fence in plan
/// prose degrades exactly like one in a final reply (#1142).
pub(crate) async fn markdown_to_html_mermaid(text: &str) -> String {
    let blocks = parse_markdown(text);
    let resolved = if mermaid::should_render_mermaid(text) {
        mermaid::resolve_blocks(blocks).await
    } else {
        blocks
    };
    render_html::render_html(&resolved)
}

/// Like [`markdown_to_html_mermaid`] but wraps paragraphs in `<p>` tags (and
/// renders soft line breaks as `<br>`): the rich `sendRichMessage` HTML
/// dialect used by the plan card's prose bodies (#1142).
pub(crate) async fn markdown_to_html_mermaid_p(text: &str) -> String {
    let blocks = parse_markdown(text);
    let resolved = if mermaid::should_render_mermaid(text) {
        mermaid::resolve_blocks(blocks).await
    } else {
        blocks
    };
    render_html::render_html_p(&resolved)
}

/// Send `markdown` as a native rich message, rendering any mermaid fences as
/// embedded images first (#1044). When a mermaid fence is present and the
/// feature is enabled, fences are resolved once and the message goes out via
/// the markdown dialect with a `media` array (so pipe tables stay native);
/// if that send fails (e.g. a Bot API server < 10.2 without the `media`
/// field) it falls back to the HTML dialect. With no mermaid fence it stays
/// byte-identical to the plain rich-markdown path. Returns `Err` on
/// transport failure so the caller can fall back.
pub(crate) async fn send_rich_with_mermaid(
    api_url: &str,
    token: &str,
    chat_id: i64,
    thread_id: Option<teloxide::types::ThreadId>,
    markdown: &str,
    origin: &str,
    origin_detail: &str,
) -> anyhow::Result<()> {
    send_rich_with_mermaid_id(
        api_url,
        token,
        chat_id,
        thread_id,
        markdown,
        origin,
        origin_detail,
    )
    .await
    .map(|_| ())
}

/// Same as [`send_rich_with_mermaid`] but returns the new message id.
pub(crate) async fn send_rich_with_mermaid_id(
    api_url: &str,
    token: &str,
    chat_id: i64,
    thread_id: Option<teloxide::types::ThreadId>,
    markdown: &str,
    origin: &str,
    origin_detail: &str,
) -> anyhow::Result<i32> {
    if !mermaid::should_render_mermaid(markdown) {
        return api::send_rich_markdown_id(
            api_url,
            token,
            chat_id,
            thread_id,
            markdown,
            origin,
            origin_detail,
        )
        .await;
    }

    // Resolve every fence once: valid diagrams become markdown media
    // references, broken ones become legible failure blocks. Non-fence text
    // is left byte-identical.
    let (resolved, media) = mermaid::resolve_markdown_media(markdown).await;

    // All fences failed → `resolved` carries only failure blocks, no media to
    // embed; send it as plain rich markdown (no `media` field).
    if media.is_empty() {
        return api::send_rich_markdown_id(
            api_url,
            token,
            chat_id,
            thread_id,
            &resolved,
            origin,
            origin_detail,
        )
        .await;
    }

    // Primary: markdown dialect + media array keeps pipe tables native.
    match api::send_rich_markdown_media_id(
        api_url,
        token,
        chat_id,
        thread_id,
        &resolved,
        &media,
        origin,
        origin_detail,
    )
    .await
    {
        Ok(id) => Ok(id),
        Err(e) => {
            // Fallback: HTML dialect (Bot API < 10.2 or a media-field
            // rejection). Tables degrade there, but the message still lands.
            tracing::warn!("rich markdown+media send failed ({e}); falling back to html dialect");
            let html = markdown_to_html_mermaid(markdown).await;
            api::send_rich_html_id(
                api_url,
                token,
                chat_id,
                thread_id,
                &html,
                None,
                origin,
                origin_detail,
            )
            .await
        }
    }
}
