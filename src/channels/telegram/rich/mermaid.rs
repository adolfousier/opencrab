//! Mermaid diagram rendering for Telegram rich messages (#1044).
//!
//! Model output frequently carries ```mermaid fences. They are embedded via
//! `sendRichMessage`: the PRIMARY path is the markdown input mode plus the
//! Bot API 10.2 `media` field (`tg://photo?id=` references), which keeps any
//! tables in the message native; the HTML input mode (`<img>`) is the
//! fallback for servers without the `media` field. A broken image URL makes
//! the whole send fail with `RICH_MESSAGE_PHOTO_NO_MEDIA_FOUND`, so before
//! delivery each fence is pre-validated against the renderer (mermaid.ink):
//! a 200 + `image/*` response embeds the image; anything else falls back to
//! the in-process renderer when the `local-mermaid` feature is compiled in
//! (PNG bytes uploaded via multipart), else degrades to a legible failure
//! block (the renderer's error note plus the original source) instead of
//! killing the message. Pre-validation never panics or hangs; failure paths
//! yield [`MermaidResult::Failed`] when no fallback render succeeds either.

use super::ast::{Block, MermaidResult};
use futures::FutureExt;
use futures::future::BoxFuture;

/// Base URL of the mermaid.ink image renderer. The diagram source is
/// base64url-appended. NOTE: this sends the diagram text to a third party.
const MERMAID_INK_BASE: &str = "https://mermaid.ink/img/";

/// Query parameters appended to every mermaid.ink render request.
///
/// The service's defaults produce small blurry JPEGs; these request PNG at
/// double resolution instead (empirically verified 2026-08-26:
/// `type=png&width=1600&scale=2` yields a 3200px-wide PNG where bare `/img/`
/// yields a ~1900px-wide JPEG of the same source). `scale` is ignored
/// without `width`, so the two always travel together.
const MERMAID_INK_PARAMS: &str = "?type=png&width=1600&scale=2";

/// Upper bound on a single pre-validation request. A slow renderer must not
/// stall message delivery; on timeout we fall back (to the local renderer
/// when compiled in, else to a failure block).
const PREVALIDATE_TIMEOUT_SECS: u64 = 10;

/// Cap on how much of the renderer's error body we surface, so a huge HTML
/// error page can't blow up the message.
const ERROR_NOTE_MAX_CHARS: usize = 400;

/// One media reference embedded via the markdown `media` field (#1044).
/// `id` matches the `tg://photo?id=<id>` reference in the markdown text.
///
/// Exactly one of the payload sources is set:
/// - `url`: the mermaid.ink renderer image URL Telegram fetches server-side
///   (the legacy, network-dependent path).
/// - `bytes`: locally-rendered PNG bytes (feature `local-mermaid`) uploaded
///   to Telegram via multipart as `attach://<id>` — Telegram never touches a
///   third-party URL.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MediaEntry {
    pub(crate) id: String,
    pub(crate) url: Option<String>,
    pub(crate) bytes: Option<Vec<u8>>,
}

/// A located ```mermaid fence in the source markdown. `start` is the byte
/// offset of the opening fence line's first byte; `end` is the byte offset
/// just past the closing fence line (including its terminator). Replacing
/// `text[start..end]` swaps the fence without touching the rest.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MermaidFence {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) source: String,
}

/// Encode `input` as base64url (RFC 4648 §5, no padding), the alphabet
/// mermaid.ink requires. Standard base64 (`+`, `/`) returns 404 there.
pub(crate) fn base64url(input: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input.as_bytes())
}

/// Whether a fence body (the text between opening and closing ``` lines)
/// starts like a mermaid diagram. Used to classify *untagged* fences, whose
/// info string is empty: models frequently emit diagrams as ```graph TD ...
/// without the `mermaid` tag. The first non-blank, non-comment (`%%`) line
/// must start with a known diagram opener; `graph` additionally requires a
/// direction word (`TD`/`TB`/`BT`/`LR`/`RL`) so DOT-style `graph G {` and
/// similar foreign notations are not misclassified.
pub(crate) fn looks_like_mermaid_source(source: &str) -> bool {
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let mut words = line.split_whitespace();
        let head = words.next().unwrap_or("").to_ascii_lowercase();
        return match head.as_str() {
            "graph" => words.next().is_some_and(|d| {
                matches!(
                    d.to_ascii_lowercase().as_str(),
                    "td" | "tb" | "bt" | "lr" | "rl"
                )
            }),
            "flowchart" | "sequencediagram" | "classdiagram" | "classdiagram-v2"
            | "statediagram" | "statediagram-v2" | "erdiagram" | "journey" | "gantt" | "pie"
            | "quadrantchart" | "requirementdiagram" | "gitgraph" | "mindmap" | "timeline"
            | "zenuml" | "sankey-beta" | "xychart-beta" | "block-beta" | "packet-beta"
            | "architecture-beta" => true,
            _ => false,
        };
    }
    false
}

/// Whether `text` contains a fence that should render as a mermaid diagram:
/// either tagged ```mermaid, or untagged with mermaid-shaped content
/// ([`looks_like_mermaid_source`]). A fast line-scan used to gate the richer
/// (async) render path.
pub(crate) fn has_mermaid_fence(text: &str) -> bool {
    let mut in_fence = false;
    let mut tagged_mermaid = false;
    // Whether the OPENING fence carried no info string. Content
    // classification applies to bare fences only, so the opening tag is what
    // decides it — the closing line is bare almost every time and says
    // nothing about the block.
    let mut untagged = false;
    let mut body_start = 0usize;
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        let line_end = pos + line.len();
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_fence {
                let body = text[body_start..pos].trim_end_matches('\n');
                if tagged_mermaid || (untagged && looks_like_mermaid_source(body)) {
                    return true;
                }
                in_fence = false;
            } else {
                in_fence = true;
                let info = rest.trim();
                tagged_mermaid = info.eq_ignore_ascii_case("mermaid");
                untagged = info.is_empty();
                body_start = line_end;
            }
        }
        pos = line_end;
    }
    false
}

/// Locate every mermaid fence in `text`, returning byte ranges and the
/// diagram source between the fences. Consistent with [`has_mermaid_fence`]:
/// a fence qualifies when its info string trims to `mermaid`
/// (case-insensitive), or is empty and the body starts like a diagram
/// ([`looks_like_mermaid_source`]); either way it is closed by the next
/// bare ``` line.
pub(crate) fn find_mermaid_fences(text: &str) -> Vec<MermaidFence> {
    let mut fences = Vec::new();
    let mut in_fence = false;
    let mut is_mermaid = false;
    // See `has_mermaid_fence`: classification keys off the OPENING info
    // string, never the closing line's.
    let mut untagged = false;
    let mut block_start = 0usize;
    let mut source_start = 0usize;
    let mut pos = 0usize;

    for line in text.split_inclusive('\n') {
        let line_end = pos + line.len();
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_fence {
                let source = &text[source_start..pos];
                if is_mermaid || (untagged && looks_like_mermaid_source(source)) {
                    fences.push(MermaidFence {
                        start: block_start,
                        end: line_end,
                        source: source.to_string(),
                    });
                }
                in_fence = false;
                is_mermaid = false;
            } else {
                block_start = pos;
                source_start = line_end;
                let info = rest.trim();
                is_mermaid = info.eq_ignore_ascii_case("mermaid");
                untagged = info.is_empty();
                in_fence = true;
            }
        }
        pos = line_end;
    }
    fences
}

/// Whether `text` should be routed through the mermaid render path:
/// rich messages are enabled, the `mermaid_render` flag is on, and the text
/// actually contains a mermaid fence. Requires `rich_messages` because the
/// image can only be embedded via `sendRichMessage`.
pub(crate) fn should_render_mermaid(text: &str) -> bool {
    let tg = &crate::config::Config::current().channels.telegram;
    tg.rich_messages && tg.mermaid_render && has_mermaid_fence(text)
}

/// Full mermaid.ink embed/prevalidate URL for a diagram source: b64url
/// payload plus the hi-res PNG parameters ([`MERMAID_INK_PARAMS`]).
pub(crate) fn ink_url(source: &str) -> String {
    format!(
        "{}{}{}",
        MERMAID_INK_BASE,
        base64url(source),
        MERMAID_INK_PARAMS
    )
}

/// Pre-validate a single mermaid diagram against the renderer. On HTTP 200
/// + an `image/*` content type it DOWNLOADS the rendered PNG and returns
/// [`MermaidResult::ImageBytes`] — Telegram never fetches a URL from us
/// (its own URL fetcher proved the unreliable link: `400 failed to get
/// HTTP URL content` on live probes while the same URL fetched fine from
/// this host). Every other outcome (non-200, non-image, timeout, transport
/// error, client build failure, dropped body) yields
/// [`MermaidResult::Failed`] with a legible note. Never panics, never hangs
/// past the timeout.
///
/// Primary path for every build: on success [`resolve_fence`] delivers the
/// bytes via multipart; on [`MermaidResult::Failed`] it hands off to the
/// in-process renderer ([`local_fallback`]) when the `local-mermaid`
/// feature is compiled in.
pub(crate) async fn prevalidate(source: &str) -> MermaidResult {
    let url = ink_url(source);

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
        // Bytes delivery (hybrid round 2): download the PNG here and hand
        // Telegram the bytes via multipart (`attach://`). Telegram's own
        // server-side URL fetcher is the unreliable hop — it 400s with
        // "failed to get HTTP URL content" on URLs this host fetches fine —
        // so Telegram never sees a URL at all.
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(_) => {
                return local_fallback(source, "diagram renderer dropped the image".to_string());
            }
        };
        // Oversize renders fall to the local ladder, which downscales
        // instead of letting Telegram reject the photo outright (#1238).
        if let Some((w, h)) = png_dims(&body) {
            if !crate::channels::telegram::rich::render::photo_fits_box(w, h) {
                return local_fallback(
                    source,
                    format!("rendered diagram {w}x{h} exceeds the photo box"),
                );
            }
        }
        tracing::info!(bytes = body.len(), "mermaid.ink render ok; delivering bytes");
        return MermaidResult::ImageBytes(body.to_vec());
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

/// Pure: given a pre-validation outcome, the fence's position, and its
/// source, produce the markdown replacement and (for a valid image) the
/// media entry. Split out so the replacement shape is unit-testable without
/// a network call. `index` is the fence's ordinal in the message.
pub(crate) fn replacement_for(
    outcome: &MermaidResult,
    index: usize,
    source: &str,
) -> (String, Option<MediaEntry>) {
    match outcome {
        MermaidResult::Image(url) => {
            let id = format!("diag{index}");
            (
                format!("![diagram](tg://photo?id={id})"),
                Some(MediaEntry {
                    id,
                    url: Some(url.clone()),
                    bytes: None,
                }),
            )
        }
        MermaidResult::ImageBytes(bytes) => {
            let id = format!("diag{index}");
            (
                format!("![diagram](tg://photo?id={id})"),
                Some(MediaEntry {
                    id,
                    url: None,
                    bytes: Some(bytes.clone()),
                }),
            )
        }
        MermaidResult::Failed(err) => (markdown_failure_block(err, source), None),
    }
}

/// Resolve a single fence to a render outcome. Hybrid delivery (#1044,
/// hybrid round): mermaid.ink is the PRIMARY render path (ELK layout,
/// Chromium text rendering, full mermaid 11 diagram coverage) and the
/// in-process renderer is the AUTOMATIC fallback for when the service is
/// down, erroring, or slow.
///
/// - [`prevalidate`] succeeds → the PNG bytes ride to Telegram via
///   multipart (`attach://`); Telegram never refetches a URL.
/// - [`prevalidate`] fails ([`MermaidResult::Failed`]) → the fence is
///   re-resolved LOCALLY when the `local-mermaid` feature is compiled in:
///   PNG bytes rendered in-process and uploaded via multipart
///   (`attach://<id>`) — Telegram never fetches a URL from us in that case.
/// - Without the feature, the prevalidation note degrades to the legible
///   failure block exactly as the pre-hybrid URL path did.
async fn resolve_fence(source: &str) -> MermaidResult {
    match prevalidate(source).await {
        ok @ MermaidResult::Image(_) => ok,
        MermaidResult::Failed(note) => local_fallback(source, note),
        other => other,
    }
}

/// Local-render fallback for a failed mermaid.ink prevalidation. Sync on
/// purpose: tests drive it directly without touching the network (same seam
/// pattern as [`is_image_response`]).
pub(crate) fn local_fallback(source: &str, note: String) -> MermaidResult {
    #[cfg(feature = "local-mermaid")]
    {
        tracing::warn!(note = %note, "mermaid.ink primary path failed; falling back to local render");
        crate::channels::telegram::rich::render::local_render_result(source)
    }
    #[cfg(not(feature = "local-mermaid"))]
    {
        tracing::debug!(note = %note, "mermaid.ink down; no local renderer compiled in");
        MermaidResult::Failed(note)
    }
}

/// Resolve every mermaid fence in `text` for the markdown+media path: valid
/// diagrams become `![diagram](tg://photo?id=diagN)` references with a
/// matching [`MediaEntry`], broken ones become legible markdown failure
/// blocks. Non-fence text is untouched (byte-identical). Boxed because the
/// resolver is async and because it spans an await boundary.
pub(crate) fn resolve_markdown_media(text: &str) -> BoxFuture<'static, (String, Vec<MediaEntry>)> {
    let text = text.to_string();
    async move {
        let fences = find_mermaid_fences(&text);
        if fences.is_empty() {
            return (text, Vec::new());
        }
        let mut result = text.clone();
        let mut media = Vec::new();
        // Replace from last to first so earlier byte offsets stay valid.
        for (i, fence) in fences.iter().enumerate().rev() {
            let outcome = resolve_fence(&fence.source).await;
            let (replacement, entry) = replacement_for(&outcome, i, &fence.source);
            if let Some(e) = entry {
                media.push(e);
            }
            result.replace_range(fence.start..fence.end, &replacement);
        }
        // Media was pushed in reverse fence order; restore fence order.
        media.reverse();
        (result, media)
    }
    .boxed()
}

/// Recursively replace every `Code{lang:"mermaid"}` block with a
/// `Mermaid{source, result}` block by pre-validating each fence. Handles
/// top-level fences and fences nested inside quotes, list items, and details.
/// Boxed because the walk is recursive and an async fn cannot recurse
/// without indirection (E0733). Used by the HTML fallback path.
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
            Block::Code { lang, text }
                if lang.as_deref().is_some_and(is_mermaid_lang)
                    || (lang.is_none() && looks_like_mermaid_source(&text)) =>
            {
                let result = resolve_fence(&text).await;
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

/// Markdown for a diagram that could not be rendered: a bold warning line,
/// then a code fence holding the renderer's error note and the original
/// source so the reader can see (and fix) what failed.
pub(crate) fn markdown_failure_block(err: &str, source: &str) -> String {
    format!(
        "> ⚠️ **Mermaid diagram could not be rendered**\n\n```\n{err}\n\nSource:\n{source}\n```"
    )
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
