//! Mermaid diagram rendering for Telegram rich messages (#1044).
//!
//! Model output frequently carries ```mermaid fences. Telegram's
//! `sendRichMessage` can embed an image via `<img>` but ONLY in the HTML
//! input mode, and a broken image URL makes the whole send fail with
//! `RICH_MESSAGE_PHOTO_NO_MEDIA_FOUND`. So before delivery each fence is
//! pre-validated against the renderer (mermaid.ink): a 200 + `image/*`
//! response embeds the image, anything else degrades to a legible failure
//! block (the renderer's error note plus the original source) instead of
//! killing the message. Pre-validation never panics or hangs; every failure
//! path yields [`MermaidResult::Failed`].

use super::ast::{Block, MermaidResult};
use futures::FutureExt;
use futures::future::BoxFuture;

/// Base URL of the mermaid.ink image renderer. The diagram source is
/// base64url-appended. NOTE: this sends the diagram text to a third party.
const MERMAID_INK_BASE: &str = "https://mermaid.ink/img/";

/// Upper bound on a single pre-validation request. A slow renderer must not
/// stall message delivery; on timeout we degrade to a failure block.
const PREVALIDATE_TIMEOUT_SECS: u64 = 10;

/// Cap on how much of the renderer's error body we surface, so a huge HTML
/// error page can't blow up the message.
const ERROR_NOTE_MAX_CHARS: usize = 400;

/// Encode `input` as base64url (RFC 4648 §5, no padding), the alphabet
/// mermaid.ink requires. Standard base64 (`+`, `/`) returns 404 there.
pub(crate) fn base64url(input: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input.as_bytes())
}

/// Whether `text` contains an opening ``` fence tagged `mermaid`.
/// A fast line-scan used to gate the richer (async) render path.
pub(crate) fn has_mermaid_fence(text: &str) -> bool {
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("```") {
            if in_fence {
                in_fence = false; // a closing fence
            } else {
                if rest.trim().eq_ignore_ascii_case("mermaid") {
                    return true;
                }
                in_fence = true;
            }
        }
    }
    false
}

/// Whether `text` should be routed through the mermaid HTML render path:
/// rich messages are enabled, the `mermaid_render` flag is on, and the text
/// actually contains a mermaid fence. Requires `rich_messages` because the
/// image can only be embedded via `sendRichMessage`.
pub(crate) fn should_render_mermaid(text: &str) -> bool {
    let tg = &crate::config::Config::current().channels.telegram;
    tg.rich_messages && tg.mermaid_render && has_mermaid_fence(text)
}

/// Pre-validate a single mermaid diagram against the renderer. Returns
/// [`MermaidResult::Image`] with the embed URL only on HTTP 200 + an
/// `image/*` content type; every other outcome (non-200, non-image, timeout,
/// transport error, client build failure) yields [`MermaidResult::Failed`]
/// with a legible note. Never panics, never hangs past the timeout.
pub(crate) async fn prevalidate(source: &str) -> MermaidResult {
    let url = format!("{}{}", MERMAID_INK_BASE, base64url(source));

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(PREVALIDATE_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(_) => return MermaidResult::Failed("diagram renderer unavailable".into()),
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            let note = if e.is_timeout() {
                "diagram renderer timed out".to_string()
            } else {
                "diagram renderer unreachable".to_string()
            };
            return MermaidResult::Failed(note);
        }
    };

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if is_image_response(status, &content_type) {
        return MermaidResult::Image(url);
    }

    // Not a usable image: surface the renderer's own error text (mermaid.ink
    // returns a plain-text parse error) so the failure block is legible.
    let body = resp.text().await.unwrap_or_default();
    MermaidResult::Failed(error_note(status, &body))
}

/// Whether an HTTP response represents a usable rendered image. Split out so
/// the accept/reject branching is unit-testable without a network call.
pub(crate) fn is_image_response(status: u16, content_type: &str) -> bool {
    (200..300).contains(&status) && content_type.to_lowercase().starts_with("image/")
}

/// Build a short, legible failure note from the renderer's response body.
pub(crate) fn error_note(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("diagram renderer returned HTTP {status}");
    }
    trimmed.chars().take(ERROR_NOTE_MAX_CHARS).collect()
}

/// Recursively replace every `Code{lang:"mermaid"}` block with a
/// `Mermaid{source, result}` block by pre-validating each fence. Handles
/// top-level fences and fences nested inside quotes, list items, and details.
/// Boxed because the walk is recursive and an async fn cannot recurse
/// without indirection (E0733).
pub(crate) fn resolve_blocks(blocks: Vec<Block>) -> BoxFuture<'static, Vec<Block>> {
    async move {
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            out.push(resolve_block(block).await);
        }
        out
    }
    .boxed()
}

fn resolve_block(block: Block) -> BoxFuture<'static, Block> {
    async move {
        match block {
            Block::Code {
                lang: Some(lang),
                text,
            } if is_mermaid_lang(&lang) => {
                let result = prevalidate(&text).await;
                Block::Mermaid {
                    source: text,
                    result,
                }
            }
            Block::Quote(inner) => Block::Quote(resolve_blocks(inner).await),
            Block::List(mut list) => {
                for item in &mut list.items {
                    item.children = resolve_blocks(std::mem::take(&mut item.children)).await;
                }
                Block::List(list)
            }
            Block::Details {
                summary,
                blocks,
                open,
            } => Block::Details {
                summary,
                blocks: resolve_blocks(blocks).await,
                open,
            },
            other => other,
        }
    }
    .boxed()
}

fn is_mermaid_lang(lang: &str) -> bool {
    lang.trim().eq_ignore_ascii_case("mermaid")
}

/// HTML for a successfully rendered diagram: a bare `<img>` in a `<figure>`,
/// which the Telegram rich-HTML parser turns into a native photo block.
pub(crate) fn image_html(url: &str) -> String {
    format!("<figure><img src=\"{}\"/></figure>", escape(url))
}

/// HTML for a diagram that could not be rendered: a bold warning line, the
/// renderer's error note in a blockquote, and the original source in a code
/// block so the reader can see (and fix) what failed.
pub(crate) fn failure_html(err: &str, source: &str) -> String {
    format!(
        "<b>⚠️ Mermaid diagram could not be rendered</b>\n<blockquote>{}</blockquote>\n<pre><code>{}</code></pre>",
        escape(err),
        escape(source)
    )
}

/// Minimal HTML entity escaping (matches render_html's escaping).
fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
