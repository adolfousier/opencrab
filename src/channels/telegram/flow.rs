//! Telegram processing-log flow: the collapsed, edited-in-place block that
//! folds tool calls and intermediate narration into one growing message.
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! only visibility widened to pub(crate) so the handler glob re-export
//! keeps every existing call site and test import stable). Covers the
//! streaming state types, the three renderers (classic HTML, rich details,
//! rich markdown), and the flow operations (open/refresh/append/restick/
//! freeze/fold).

use crate::config::Config;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode};

use super::handler::{escape_html, format_inline, send_html_or_plain};

/// Individual tool call — each gets its own Telegram message.
pub(crate) struct ToolMsg {
    pub(crate) msg_id: Option<MessageId>,
    pub(crate) name: String,
    pub(crate) context: String,
    /// None = running, Some(true) = success, Some(false) = failed
    pub(crate) completed: Option<bool>,
    pub(crate) dirty: bool,
}

/// Per-message streaming state shared between the progress callback and the edit loop.
/// Each tool call gets its own message above; response streams in a separate message below.
/// Ordered display event — preserves chronological ordering of tools and intermediate texts.
#[derive(Clone)]
pub(crate) enum DisplayItem {
    /// New tool at this index in tool_msgs (needs send_message)
    NewTool(usize),
    /// Intermediate text between tool rounds
    Intermediate(String),
}

/// One entry in the in-place processing log (the growing `<blockquote
/// expandable>` message). Tool entries reference `tool_msgs` by index so a
/// status flip (⚙️ → ✅/❌) re-renders live; text entries hold the already
/// sanitized intermediate text (escaped at render time). Interleaving both in
/// one ordered flow lets tool calls and intermediate text share a single
/// collapsed block instead of each landing as a separate message.
#[derive(Clone)]
pub(crate) enum FlowEntry {
    /// A tool call at this index in `tool_msgs`.
    Tool(usize),
    /// Sanitized intermediate text (plain; escaped when rendered).
    Text(String),
}

pub(crate) struct StreamingState {
    /// Response/thinking message (always at bottom)
    pub(crate) msg_id: Option<MessageId>,
    /// Reasoning/thinking text — streamed live, cleared before tool calls or response
    pub(crate) thinking: String,
    /// Each tool call = its own individual message
    pub(crate) tool_msgs: Vec<ToolMsg>,
    /// Ordered queue of new display items (tools + intermediates in chronological order)
    pub(crate) display_queue: Vec<DisplayItem>,
    /// Message ID of the open processing-log message. Tool calls and
    /// intermediate text are appended to this one message (edited in place) so
    /// the whole procedural trace of a turn collapses into a single growing
    /// `<blockquote expandable>` block instead of one message per event. Set
    /// when the first entry lands; stays open for the rest of the turn so the
    /// final response is the only clean message at the bottom.
    pub(crate) open_group_msg_id: Option<MessageId>,
    /// Ordered entries in the open processing-log message (tool calls +
    /// intermediate text, in chronological order). Rendered together into the
    /// `open_group_msg_id` message on every append/status change.
    pub(crate) flow_entries: Vec<FlowEntry>,
    /// Live status shown in the open block's header while the turn runs
    /// ("read_file · 45s"). The block is the SINGLE progress surface (#360):
    /// no standalone status ticker exists while a block is open. HTML-safe
    /// (escaped at build time). None once the final response lands.
    pub(crate) flow_status: Option<String>,
    /// True when the open flow block lives on the rich API (#420 path A):
    /// edits must ride edit_rich_html; false = classic HTML blockquote.
    pub(crate) flow_rich: bool,
    /// Response text from streaming chunks — own message at bottom
    pub(crate) response: String,
    pub(crate) dirty: bool,
    /// When true, the edit loop deletes the response message and creates a fresh one
    /// at the bottom of the chat (so it appears below tool/approval messages).
    pub(crate) recreate: bool,
    /// Pre-block dynamic status message (thinking excerpt / user preview),
    /// standalone ONLY while no grouping block exists yet. Once the block
    /// opens (or the final response lands) it is deleted; the id is cleared
    /// ONLY after a successful delete so a failed delete retries next tick
    /// instead of orphaning the bubble.
    pub(crate) status_msg_id: Option<MessageId>,
    /// Last text rendered into the status message (skip no-op edits).
    pub(crate) status_last_text: Option<String>,
    /// Number of tool rounds completed (for display)
    pub(crate) tool_round_count: usize,
    /// When tool execution started (for elapsed time)
    pub(crate) tools_started_at: Option<std::time::Instant>,
    /// Instant the turn started (first user message), set once at construction
    /// and never reset — the wall-clock anchor for the header duration (#480),
    /// both live and settled. Distinct from `tools_started_at`, which is
    /// cleared on settle and re-armed per tool phase.
    pub(crate) turn_started_at: std::time::Instant,
    /// Terminal outcome once the turn ends, driving the settled block header
    /// (`✅ Finished (N tool calls, 45s)` / `❌ Failed` / `⏱ Timed out`, #480).
    /// `None` while the turn is live.
    pub(crate) flow_outcome: Option<FlowOutcome>,
    /// Intermediate texts already sent — used to dedup final response
    pub(crate) sent_intermediates: Vec<String>,
    /// Message IDs of every intermediate chunk delivered to Telegram, so a
    /// cancelled in-flight call can clean up after itself. Without this, a
    /// cancelled old call leaves its intermediate visible and the new call
    /// re-sends the same text — the exact-match duplicate the user reported.
    pub(crate) intermediate_msg_ids: Vec<MessageId>,
    /// Message IDs of every voice note delivered to Telegram via `send_voice`
    /// (TTS responses to voice-input turns). This field exists purely as a
    /// load-bearing invariant: voice-reply IDs live here and MUST NEVER be
    /// iterated for deletion by any cleanup/cancellation/rebuild path. If a
    /// future contributor adds a bulk cleanup over message IDs they have to
    /// consciously skip this field. The user's TTS voice note is the most
    /// expensive artefact to reproduce — it's a real synthesis call, not a
    /// cheap text render — so losing it to a sweep that "looked reasonable
    /// at the time" is a regression we've deliberately made hard to introduce.
    pub(crate) voice_msg_ids: Vec<MessageId>,
    /// True from start until first response text arrives — enables rolling messages for CLI providers
    /// where tools complete instantly (ToolStarted+ToolCompleted back-to-back)
    pub(crate) processing: bool,
    /// Short preview of the user's incoming message, captured once at
    /// handler start. Drives the pre-tool rolling status line: when
    /// the model hasn't streamed any reasoning yet AND no tool is
    /// running, we still want to surface SOMETHING context-aware to
    /// the user (e.g. "Working on: how do I add a topic? (5s)")
    /// rather than going silent. Silence here was the regression —
    /// the original status pipeline never went quiet, it just had
    /// nothing real to show; build_status_message uses this preview
    /// as honest content derived from the real user input rather
    /// than reintroducing hardcoded filler quips.
    pub(crate) user_message_preview: Option<String>,
}

/// A resolved line in the processing-log flow, ready to render. Tool lines
/// carry the status label (icon + name) and context; text lines carry the
/// sanitized intermediate text. Both are HTML-escaped at render time.
pub(crate) enum FlowLine {
    Tool { label: String, context: String },
    Text(String),
}

/// Render resolved flow lines into final Telegram HTML. A lone tool line with
/// no other content stays a plain one-liner (mirrors #296); anything else
/// collapses into a single `<blockquote expandable>` block (Bot API 7.3+) that
/// renders with a tap-to-expand arrow in groups, DMs, and all official clients
/// with no rich-API dependency. Tool calls and intermediate text share the same
/// block so only the final response stays clean at the bottom (#300). Output is
/// final HTML — send via `send_html_or_plain`, never through
/// `markdown_to_telegram_html` (it would double-process the HTML).
/// Plain-text preview of the latest activity for the collapsed block header
/// (#405). Telegram's collapsed expandable blockquote shows the header plus the
/// first content line, and entries render chronologically — so without this a
/// long turn pins its FIRST narration line on screen forever while only the
/// header counters tick.
///
/// Priority chain: (1, #481 amended) the most recent HUMAN-READABLE
/// intermediary text, returned WHOLE — all paragraphs, newlines preserved, NOT
/// truncated — after skipping entries that are JSON, code blocks, or raw
/// output; (2, #482) line-start `#` comments from the most recent bash command
/// when there is no narration; (3) the most recent tool label + context. Each
/// renderer escapes/styles the returned text.
pub(crate) fn latest_activity_preview(lines: &[FlowLine]) -> Option<String> {
    // Priority 1: the whole most-recent human-readable intermediary text.
    if let Some(text) = lines.iter().rev().find_map(|l| match l {
        FlowLine::Text(t) => human_readable_preview(t),
        FlowLine::Tool { .. } => None,
    }) {
        return Some(text);
    }
    // Priority 2: line-start `#` comments from the most recent bash command
    // (the agent narrates its steps in the command itself, no separate text).
    if let Some(comments) = lines.iter().rev().find_map(|l| match l {
        FlowLine::Tool { label, context } if is_bash_tool(label) => {
            extract_status_from_text(context)
        }
        _ => None,
    }) {
        return Some(comments);
    }
    // Fallback: the most recent tool label + context.
    lines.iter().rev().find_map(|l| match l {
        FlowLine::Tool { label, context } => Some(if context.is_empty() {
            label.clone()
        } else {
            format!("{label} {context}")
        }),
        FlowLine::Text(_) => None,
    })
}

/// A flow tool line is a bash call when its name (the last word of the
/// `{icon} {name}` label) is `bash`.
fn is_bash_tool(label: &str) -> bool {
    label.split_whitespace().last() == Some("bash")
}

/// Extract line-start `#` comments from a bash command as status text (#482).
/// A comment is a line whose first non-whitespace char is `#` — no shell-aware
/// parsing of inline `#` (amendment). The `#` and any `---`/`===` decoration
/// are stripped, so `# --- Setup environment ---` yields `Setup environment`.
/// Multiple comments join by newlines, untruncated (amendment). Shebang lines
/// (`#!`) are ignored. `None` when the command has no line-start comments.
pub(crate) fn extract_status_from_text(command: &str) -> Option<String> {
    let comments: Vec<String> = command
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('#') && !l.starts_with("#!"))
        .map(|l| {
            l.trim_start_matches('#')
                .trim()
                .trim_matches(|c: char| c == '-' || c == '=' || c.is_whitespace())
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    (!comments.is_empty()).then(|| comments.join("\n"))
}

/// Return an intermediary text entry as a preview when it is human-readable
/// narration, else `None` so the caller skips backward to an earlier entry
/// (#481). Skips JSON (starts with `{`/`[`), code blocks (starts with a triple
/// backtick), and raw output (a single token with no internal whitespace — a
/// bare number or path). Human-readable text is returned WHOLE: trimmed, inline
/// markdown markers (`*`/`` ` ``/`_`) stripped so the preview never shows raw
/// source, newlines preserved, no truncation (amendment: whole intermediary
/// text as the status source).
fn human_readable_preview(text: &str) -> Option<String> {
    let trimmed = text.trim();
    // Raw output: one bare token, no internal whitespace, that reads as a path
    // or number (has a `/` or no letters at all) — e.g. `src/foo.rs` or `12345`.
    // A one-word sentence like `Done.` keeps its letters and is NOT raw.
    let looks_raw = !trimmed.chars().any(char::is_whitespace)
        && (trimmed.contains('/') || !trimmed.chars().any(char::is_alphabetic));
    if trimmed.is_empty()
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with("```")
        || looks_raw
    {
        return None;
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '_'))
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

/// Longest folded narration entry kept in the collapsed flow block (#489).
/// The block is a PROGRESS view, not the full transcript: capping each
/// folded `Text` entry keeps the block compact so far more tool rounds fit
/// before the 30K rich size freeze. Matters most for Claude CLI, whose
/// answer streams as intermediate text folded into the block (API keeps it
/// in response.content). Display-only: the renderers read `flow_entries`
/// without mutating them, so `take_folded_final` still reclaims the FULL
/// final answer.
const FOLDED_NARRATION_CAP: usize = 300;

/// Truncate a folded narration entry to [`FOLDED_NARRATION_CAP`] chars on a
/// char boundary, appending an ellipsis when cut. Short entries pass through.
fn cap_narration(text: &str) -> String {
    if text.chars().count() <= FOLDED_NARRATION_CAP {
        return text.to_string();
    }
    let capped: String = text.chars().take(FOLDED_NARRATION_CAP).collect();
    format!("{capped}…")
}

pub(crate) fn render_flow_html(lines: &[FlowLine], live_status: Option<&str>) -> String {
    render_flow_html_with(lines, &FlowHeader::Live(live_status))
}

pub(crate) fn render_flow_html_with(lines: &[FlowLine], header: &FlowHeader) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut tool_count = 0usize;
    for line in lines {
        match line {
            FlowLine::Tool { label, context } => {
                tool_count += 1;
                if context.is_empty() {
                    out.push(format!("<b>{}</b>", escape_html(label)));
                } else {
                    // Context (path / command / query) as monospace so it reads
                    // as code, not prose, inside the expanded block (#306).
                    out.push(format!(
                        "<b>{}</b> <code>{}</code>",
                        escape_html(label),
                        escape_html(context)
                    ));
                }
            }
            FlowLine::Text(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    // Render intermediate narration with the same inline markdown
                    // (bold, italics, `code`, links) as the final completion, so
                    // the expanded block is formatted, not raw markdown source
                    // (#306). format_inline emits only inline tags, which are
                    // valid inside <blockquote>; no block-level <pre> to break it.
                    // Capped (#489) so verbose folded narration doesn't blow the
                    // block size budget; display-only, reclaim reads flow_entries.
                    out.push(format_inline(&escape_html(&cap_narration(text))));
                }
            }
        }
    }
    if out.is_empty() {
        return String::new();
    }
    // Lone tool line stays plain (#296) while the turn is live; the live status
    // rides on it (#360). A settled outcome always renders the block header so
    // the ✅/❌/⏱ badge and duration show (#480).
    if out.len() == 1
        && tool_count == 1
        && let FlowHeader::Live(status) = header
    {
        return match status {
            Some(st) => format!("{} · {}", out.remove(0), st),
            None => out.remove(0),
        };
    }
    // Latest activity rides directly under the header so the COLLAPSED
    // preview always shows what is happening now, not the first entry
    // from many minutes ago (#405).
    let latest = latest_activity_preview(lines)
        .map(|l| format!("↳ <i>{}</i>\n\n", escape_html(&l)))
        .unwrap_or_default();
    format!(
        "<blockquote expandable><b>{}</b>\n{}{}</blockquote>",
        flow_header_text(tool_count, header),
        latest,
        out.join("\n\n")
    )
}

/// Render resolved flow lines as a `<details><summary>` collapsible for the
/// rich API's HTML input mode (#420 path A): the server parses it into a
/// native RichBlockDetails (summary/blocks/is_open), giving collapse parity
/// with the classic `<blockquote expandable>` PLUS the 32K rich limit, so
/// long tool chains stop splitting into multiple blocks. The summary carries
/// the live header (#360) and the latest-activity preview (#405); the
/// chronological log is the collapsed body. A lone tool line stays a plain
/// one-liner (mirrors #296, same as the HTML renderer).
pub(crate) fn render_flow_details(lines: &[FlowLine], live_status: Option<&str>) -> String {
    render_flow_details_with(lines, &FlowHeader::Live(live_status))
}

fn render_flow_details_with(lines: &[FlowLine], header: &FlowHeader) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut tool_count = 0usize;
    for line in lines {
        match line {
            FlowLine::Tool { label, context } => {
                tool_count += 1;
                if context.is_empty() {
                    out.push(format!("<b>{}</b>", escape_html(label)));
                } else {
                    out.push(format!(
                        "<b>{}</b> <code>{}</code>",
                        escape_html(label),
                        escape_html(context)
                    ));
                }
            }
            FlowLine::Text(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    // Capped (#489): keeps the block compact so more rounds
                    // fit before the 30K freeze. Display-only.
                    out.push(format_inline(&escape_html(&cap_narration(text))));
                }
            }
        }
    }
    if out.is_empty() {
        return String::new();
    }
    if out.len() == 1
        && tool_count == 1
        && let FlowHeader::Live(status) = header
    {
        return match status {
            Some(st) => format!("{} · {}", out.remove(0), st),
            None => out.remove(0),
        };
    }
    // The rich HTML input mode is a real HTML parser: raw newlines are
    // ignored (unlike the classic Bot API HTML path), so each entry must be
    // its own block-level element or the whole log runs together as one
    // inline wall. One <p> per entry gives the same visual separation the
    // classic blockquote gets from blank lines.
    let body: String = out.iter().map(|e| format!("<p>{e}</p>")).collect();
    // Latest activity rides in the summary so the COLLAPSED rich block shows
    // what is happening now (#405). The classic HTML path (render_flow_html_with)
    // puts the `↳` preview under the header inside the expandable blockquote,
    // which stays visible collapsed; the rich `<details>` collapses to the
    // summary ALONE, so without this the rich surface loses the live progress
    // preview entirely and shows only "N tool calls · 45s". Rich HTML input
    // ignores raw newlines, so it rides inline after the header.
    let latest = latest_activity_preview(lines)
        .map(|l| format!(" ↳ <i>{}</i>", escape_html(&l)))
        .unwrap_or_default();
    format!(
        "<details><summary><sub><b>{}</b>{}</sub></summary>{}</details>",
        flow_header_text(tool_count, header),
        latest,
        body
    )
}

/// Render resolved flow lines into markdown for the rich API
/// (`sendRichMessage`). The rich API supports 32K chars (vs 4096 for HTML),
/// so long tool chains fit in a single message without splitting (#393).
/// Output is markdown — send via `send_rich_markdown` / `edit_rich_markdown`.
/// Falls back to `render_flow_html` on rich API failure.
// Channel-unused since the #421 revert (the rich flow path shipped with no
// collapse); kept because #420 reuses this renderer once RichBlockDetails
// serialization lands, and its tests pin the markdown contract meanwhile.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_flow_rich(lines: &[FlowLine], live_status: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut tool_count = 0usize;
    for line in lines {
        match line {
            FlowLine::Tool { label, context } => {
                tool_count += 1;
                if context.is_empty() {
                    out.push(format!("**{label}**"));
                } else {
                    out.push(format!("**{label}** `{context}`"));
                }
            }
            FlowLine::Text(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        return String::new();
    }
    if out.len() == 1 && tool_count == 1 {
        // Lone tool line stays plain (#296); the live status rides on it so
        // the single surface still shows progress from the first call (#360).
        return match live_status {
            Some(st) => format!("{} · {}", out.remove(0), st),
            None => out.remove(0),
        };
    }
    let mut header = if tool_count > 0 {
        format!("{} tool calls", tool_count)
    } else {
        "Processing log".to_string()
    };
    if let Some(st) = live_status {
        header = format!("⚙️ {} · {}", header, st);
    }
    // Same latest-activity preview as the HTML renderer (#405).
    let latest = latest_activity_preview(lines)
        .map(|l| format!("↳ {l}\n\n"))
        .unwrap_or_default();
    format!("**{header}**\n{latest}{}", out.join("\n\n"))
}

/// Compact elapsed time for the block header ("45s", "1m 20s"). Sub-minute
/// values snap to 5s steps so the header edit fires at most every ~5s
/// instead of every tick (#360 — header edits share the per-chat budget).
pub(crate) fn humanize_elapsed(secs: u64) -> String {
    let secs = (secs / 5) * 5;
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

/// Wall-clock duration for the flow-block header (#480): precise seconds under
/// a minute (`45s`), then `X min Ys` (`1 min 30s`, `5 min 0s`). Used for both
/// the live header and the settled outcome header, anchored at turn start.
/// Replaces `humanize_elapsed_coarse`: the block is the design, so a manually
/// expanded block collapsing on a header edit is a client-side (Desktop)
/// behavior we can't detect, and precise progress time is worth more.
pub(crate) fn humanize_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{} min {}s", secs / 60, secs % 60)
    }
}

/// Terminal state of a turn, shown in the settled flow-block header (#480).
#[derive(Clone, Copy)]
pub(crate) enum FlowOutcome {
    Finished,
    Failed,
    TimedOut,
}

impl FlowOutcome {
    /// Icon and verb for the settled header, e.g. `("✅", "Finished")`.
    pub(crate) fn icon_verb(self) -> (&'static str, &'static str) {
        match self {
            FlowOutcome::Finished => ("✅", "Finished"),
            FlowOutcome::Failed => ("❌", "Failed"),
            FlowOutcome::TimedOut => ("⏱", "Timed out"),
        }
    }
}

/// How the block header renders: live during a turn, or settled to a terminal
/// outcome at the end (#480). The shared [`flow_header_text`] turns this plus
/// the tool count into the header string every renderer wraps.
pub(crate) enum FlowHeader<'a> {
    /// Turn in progress. `Some(status)` → `⚙️ N tool calls · {status}`; `None`
    /// → the plain `N tool calls` / `Processing log`.
    Live(Option<&'a str>),
    /// Turn settled: `{icon} {verb} (N tool calls, {duration})`, dropping the
    /// `N tool calls` clause when no tools ran.
    Settled {
        icon: &'a str,
        verb: &'a str,
        duration: &'a str,
    },
}

/// Build the header text (no styling wrapper) shared by all three renderers so
/// the classic HTML, rich-details, and rich-markdown headers can never drift
/// (#480).
pub(crate) fn flow_header_text(tool_count: usize, header: &FlowHeader) -> String {
    let base = if tool_count > 0 {
        format!("{tool_count} tool calls")
    } else {
        "Processing log".to_string()
    };
    match header {
        FlowHeader::Live(None) => base,
        FlowHeader::Live(Some(status)) => format!("⚙️ {base} · {status}"),
        FlowHeader::Settled {
            icon,
            verb,
            duration,
        } => {
            if tool_count > 0 {
                format!("{icon} {verb} ({tool_count} tool calls, {duration})")
            } else {
                format!("{icon} {verb} ({duration})")
            }
        }
    }
}

/// Status glyph for a tool call: running, succeeded, or failed.
pub(crate) fn tool_status_icon(completed: Option<bool>) -> &'static str {
    match completed {
        None => "⚙️",
        Some(true) => "✅",
        Some(false) => "❌",
    }
}

/// Resolve the open processing-log flow (tool calls + intermediate text, in
/// order) into renderable lines.
pub(crate) fn flow_lines(s: &StreamingState) -> Vec<FlowLine> {
    s.flow_entries
        .iter()
        .filter_map(|entry| match entry {
            FlowEntry::Tool(idx) => s.tool_msgs.get(*idx).map(|t| FlowLine::Tool {
                label: format!("{} {}", tool_status_icon(t.completed), t.name),
                context: t.context.clone(),
            }),
            FlowEntry::Text(text) => Some(FlowLine::Text(text.clone())),
        })
        .collect()
}

/// Resolve the flow into final Telegram HTML. Live turn → the plain live
/// header; a settled turn → the terminal outcome header with wall-clock
/// duration from turn start (#480).
pub(crate) fn render_flow(s: &StreamingState) -> String {
    match s.flow_outcome {
        Some(outcome) => {
            let (icon, verb) = outcome.icon_verb();
            let duration = humanize_duration(s.turn_started_at.elapsed().as_secs());
            render_flow_html_with(
                &flow_lines(s),
                &FlowHeader::Settled {
                    icon,
                    verb,
                    duration: &duration,
                },
            )
        }
        None => render_flow_html(&flow_lines(s), s.flow_status.as_deref()),
    }
}

/// Resolve the flow into the rich-API details HTML (#420 path A), with the same
/// live/settled header split as [`render_flow`].
pub(crate) fn render_flow_details_state(s: &StreamingState) -> String {
    match s.flow_outcome {
        Some(outcome) => {
            let (icon, verb) = outcome.icon_verb();
            let duration = humanize_duration(s.turn_started_at.elapsed().as_secs());
            render_flow_details_with(
                &flow_lines(s),
                &FlowHeader::Settled {
                    icon,
                    verb,
                    duration: &duration,
                },
            )
        }
        None => render_flow_details(&flow_lines(s), s.flow_status.as_deref()),
    }
}

/// Re-render the open processing-log flow and edit its message in place. Used
/// after appending an entry and after a tool status flip (⚙️ → ✅/❌). A no-op
/// edit ("message is not modified") and transient errors are ignored: the
/// message already shows the correct content and the next tick retries.
/// Edits ride the surface the block was opened on: rich details (#420 path
/// A) with classic-HTML fallback, or the classic HTML path directly.
pub(crate) async fn refresh_flow(
    bot: &Bot,
    chat: ChatId,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) {
    let (mid, rich) = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        match s.open_group_msg_id {
            Some(mid) => (mid, s.flow_rich),
            None => return,
        }
    };
    if rich {
        refresh_flow_rich_details(bot, chat, mid, streaming).await;
    } else {
        refresh_flow_html(bot, chat, mid, streaming).await;
    }
}

/// Rich-details edit path (#420 path A). 32K char limit, 30K freeze
/// threshold. A not-modified response is a no-op; any other failure falls
/// back to the classic HTML edit so the block never silently stops updating.
pub(crate) async fn refresh_flow_rich_details(
    bot: &Bot,
    chat: ChatId,
    mid: MessageId,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) {
    let details = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        render_flow_details_state(&s)
    };
    if details.is_empty() {
        return;
    }
    if details.chars().count() > 30000 {
        freeze_flow_block(streaming, mid, "rich size limit reached");
        return;
    }
    match super::rich::api::edit_rich_html(bot.token(), chat.0, mid.0, &details).await {
        Ok(_) => {}
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("message is not modified") {
                return;
            }
            tracing::warn!(
                "Telegram: rich details edit failed for mid={:?}: {msg} — falling back to HTML",
                mid
            );
            refresh_flow_html(bot, chat, mid, streaming).await;
        }
    }
}

/// HTML edit path for the processing-log flow. 4096-char limit.
pub(crate) async fn refresh_flow_html(
    bot: &Bot,
    chat: ChatId,
    mid: MessageId,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) {
    let html = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        render_flow(&s)
    };
    if html.is_empty() {
        return;
    }
    // Proactive freeze: past Telegram's 4096-char edit limit the edit can
    // only fail. Keep the message as last rendered and start a new block.
    if html.chars().count() > 4000 {
        freeze_flow_block(streaming, mid, "size limit reached");
        return;
    }
    match bot
        .edit_message_text(chat, mid, html)
        .parse_mode(ParseMode::Html)
        .await
    {
        Ok(_) => {}
        // Transient rate limit: wait it out and retry once with fresh
        // content. Deleting here used to wipe a fully rendered report off
        // the screen over a 9-second throttle (#356).
        Err(teloxide::RequestError::RetryAfter(secs)) => {
            tracing::warn!(
                "Telegram: refresh_flow rate-limited for mid={:?} — waiting {}s, then retrying",
                mid,
                secs.seconds()
            );
            tokio::time::sleep(secs.duration()).await;
            let retry_html = {
                let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                if s.open_group_msg_id != Some(mid) {
                    return; // block closed/replaced while waiting
                }
                render_flow(&s)
            };
            if let Err(e) = bot
                .edit_message_text(chat, mid, retry_html)
                .parse_mode(ParseMode::Html)
                .await
            {
                tracing::warn!(
                    "Telegram: refresh_flow retry failed for mid={:?}: {} — keeping message, next tick retries",
                    mid,
                    e
                );
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("message is not modified") {
                // Content already correct — nothing to do.
            } else if msg.contains("MESSAGE_TOO_LONG") {
                freeze_flow_block(streaming, mid, "MESSAGE_TOO_LONG");
            } else if msg.contains("message to edit not found") {
                // Genuinely gone (deleted externally) — forget the id.
                tracing::warn!(
                    "Telegram: refresh_flow target mid={:?} no longer exists — starting a new block",
                    mid
                );
                let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                if s.open_group_msg_id == Some(mid) {
                    s.open_group_msg_id = None;
                    s.flow_entries.clear();
                }
            } else {
                // Parse error or anything else: NEVER delete displayed
                // content over a failed update — the message still shows the
                // last successful render and the next tick retries (#356).
                tracing::warn!(
                    "Telegram: refresh_flow edit failed for mid={:?}: {} — keeping message",
                    mid,
                    e
                );
            }
        }
    }
}

/// Freeze the current processing-log block: keep the rendered message on
/// screen exactly as it is, close it, and let subsequent entries start a
/// fresh block. Used when the block can no longer be edited (size limit).
/// The entries rendered into the frozen message are dropped from state so
/// the next block starts small instead of instantly overflowing again.
/// A mid-turn user follow-up landed (queued-message injection): freeze the
/// open processing-log block IN PLACE — its content stays visible above the
/// follow-up — and mark the response placeholder for re-post, so the next
/// tool round opens a fresh block BELOW the user's message and the chat
/// keeps flowing bottom-down (#404).
pub(crate) fn detach_flow_for_followup(streaming: &Arc<std::sync::Mutex<StreamingState>>) {
    // The block is NOT closed here (#475). The original #404 freeze existed
    // to make the next round appear below the user's follow-up, but in a
    // busy group every incoming message froze the block and shredded one
    // turn into a dozen fragments. #451's restick achieves the ordering the
    // freeze was for — the SAME block relocates below the newest message on
    // the next round — so grouping survives: one block per turn, always at
    // the bottom. Only the response placeholder re-posts.
    let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
    if s.open_group_msg_id.is_some() {
        tracing::info!(
            "Telegram: mid-turn follow-up — flow block stays open; restick moves it \
             below on the next round (#475)"
        );
    }
    if s.msg_id.is_some() {
        s.recreate = true;
    }
}

pub(crate) fn freeze_flow_block(
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    mid: MessageId,
    reason: &str,
) {
    tracing::info!(
        "Telegram: freezing processing-log block mid={:?} ({reason}) — content stays visible, next entries start a new block",
        mid
    );
    let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
    if s.open_group_msg_id == Some(mid) {
        s.open_group_msg_id = None;
        s.flow_entries.clear();
    }
}

/// Send the open processing-log message for the first time and record its id.
/// A newly landed message re-posts the streaming placeholder next tick so the
/// response stays at the bottom (the only flow-driven recreate; subsequent
/// entries merely edit this message in place — #299).
/// When `rich_messages` is enabled, sends via the rich API (32K limit) with
/// HTML fallback (#393).
pub(crate) async fn open_flow(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) {
    // Rich-first WITH collapse parity (#420 path A): the flow renders as a
    // <details><summary> collapsible through the rich API's HTML input mode
    // (native RichBlockDetails, 32K limit — no block splitting). Any rich
    // failure falls back to the classic HTML <blockquote expandable> path,
    // which stays the proven baseline (#421: the markdown-input rich path
    // shipped flat, with no collapse at all, and was reverted).
    if Config::current().channels.telegram.rich_messages {
        let details = {
            let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            render_flow_details_state(&s)
        };
        if !details.is_empty() {
            match super::rich::api::send_rich_html_id(bot.token(), chat.0, thread_id, &details)
                .await
            {
                Ok(mid) => {
                    let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                    s.open_group_msg_id = Some(MessageId(mid));
                    s.flow_rich = true;
                    if s.msg_id.is_some() {
                        s.recreate = true;
                    }
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        "Telegram: rich details flow open failed: {e} — falling back to HTML"
                    );
                }
            }
        }
    }
    let html = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        render_flow(&s)
    };
    if html.is_empty() {
        return;
    }
    if let Ok(mid) = send_html_or_plain(bot, chat, thread_id, &html).await {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.open_group_msg_id = Some(mid);
        s.flow_rich = false;
        if s.msg_id.is_some() {
            s.recreate = true;
        }
    }
}

/// Append buffered tool calls to the open processing-log flow, editing that one
/// message in place (or opening it if none is live yet) so consecutive tool
/// calls collapse into a single growing block.
pub(crate) async fn append_tool_group(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    buffer: &[usize],
) {
    if buffer.is_empty() {
        return;
    }
    let open = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        for &idx in buffer {
            s.flow_entries.push(FlowEntry::Tool(idx));
        }
        s.open_group_msg_id
    };
    if open.is_some() {
        // Tag the tools with the open message so status flips find them.
        {
            let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            let mid = s.open_group_msg_id;
            for &idx in buffer {
                if let Some(tool) = s.tool_msgs.get_mut(idx) {
                    tool.msg_id = mid;
                }
            }
        }
        refresh_flow(bot, chat, streaming).await;
    } else {
        open_flow(bot, chat, thread_id, streaming).await;
        let mid = streaming
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_group_msg_id;
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        for &idx in buffer {
            if let Some(tool) = s.tool_msgs.get_mut(idx) {
                tool.msg_id = mid;
            }
        }
    }
}

/// Append sanitized intermediate text to the open processing-log flow, editing
/// that one message in place (or opening it if none is live yet). The text is
/// folded into the collapsed block instead of landing as its own message, so
/// only the final response stays clean at the bottom. Empty text (e.g. a
/// react-only intermediate) is ignored.
pub(crate) async fn append_intermediate_to_flow(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    text: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    let open = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.flow_entries.push(FlowEntry::Text(text.to_string()));
        s.open_group_msg_id
    };
    if open.is_some() {
        refresh_flow(bot, chat, streaming).await;
    } else {
        open_flow(bot, chat, thread_id, streaming).await;
    }
}

/// Re-stick the open processing-log block to the bottom of the chat when newer
/// chatter has buried it (#451). Called only on a new round (tools/intermediate
/// appended this tick), never on plain status ticks, so an idle chat sees no
/// churn. If a message with a higher id than the block landed, re-post the
/// block's current full content as a fresh message at the bottom on the SAME
/// surface (rich details or classic HTML), retag its tool entries to the new
/// message, then delete the old copy. On any re-send failure the old block is
/// kept untouched: relocation must never lose the block. `newest_incoming` is
/// the highest incoming message id the handler has recorded for this chat.
pub(crate) async fn restick_flow_if_buried(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    newest_incoming: Option<i32>,
) {
    let (old_mid, rich) = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        match s.open_group_msg_id {
            Some(mid) => (mid, s.flow_rich),
            None => return,
        }
    };
    // Buried only if a chat message with a higher id than the block landed.
    match newest_incoming {
        Some(newest) if newest > old_mid.0 => {}
        _ => return,
    }

    // Re-post the current full flow at the bottom on the same surface.
    let new_mid: Option<MessageId> = if rich {
        let details = {
            let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            render_flow_details_state(&s)
        };
        if details.is_empty() {
            return;
        }
        match super::rich::api::send_rich_html_id(bot.token(), chat.0, thread_id, &details).await {
            Ok(mid) => Some(MessageId(mid)),
            Err(e) => {
                tracing::warn!(
                    "Telegram: restick rich re-post failed: {e} — keeping buried block in place"
                );
                None
            }
        }
    } else {
        let html = {
            let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            render_flow(&s)
        };
        if html.is_empty() {
            return;
        }
        match send_html_or_plain(bot, chat, thread_id, &html).await {
            Ok(mid) => Some(mid),
            Err(e) => {
                tracing::warn!(
                    "Telegram: restick HTML re-post failed: {e} — keeping buried block in place"
                );
                None
            }
        }
    };
    let Some(new_mid) = new_mid else {
        return;
    };

    // Swap the block id to the relocated message BEFORE deleting the old copy,
    // so a concurrent refresh edits the new message. Decide under the lock and
    // release it before any await (the guard is not Send). If something else
    // moved or closed the block while we were sending, our just-sent copy is a
    // stray duplicate: delete it instead of the old block.
    let relocated = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        if s.open_group_msg_id == Some(old_mid) {
            s.open_group_msg_id = Some(new_mid);
            for t in s.tool_msgs.iter_mut() {
                if t.msg_id == Some(old_mid) {
                    t.msg_id = Some(new_mid);
                }
            }
            true
        } else {
            false
        }
    };
    if relocated {
        if let Err(e) = bot.delete_message(chat, old_mid).await {
            tracing::warn!("Telegram: restick could not delete old block mid={old_mid:?}: {e}");
        }
    } else if let Err(e) = bot.delete_message(chat, new_mid).await {
        tracing::warn!("Telegram: restick could not delete stray duplicate: {e}");
    }
}

/// Pull the trailing folded intermediate out of the collapsed processing-log
/// block so it can be delivered as its own message below.
///
/// For CLI providers the final assistant answer is emitted mid-stream as an
/// `IntermediateText` event (and cleared from the returned `response.content`),
/// so #300's fold buries it inside the expandable block and the completion
/// never lands as a separate bubble. Mid-turn narration is always followed by
/// more tool calls, so a `Text` entry sitting LAST in the flow is always the
/// final answer, never interstitial text. This pops it, re-renders the block
/// without it (or deletes the block if it becomes empty), and returns the text.
/// Returns `None` when the flow ended on a tool call — then the answer is in
/// `response.content` and the normal delivery path handles it.
pub(crate) async fn take_folded_final(
    bot: &Bot,
    chat: ChatId,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) -> Option<String> {
    let text = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        pop_trailing_folded_texts(&mut s.flow_entries)
    };
    text.as_ref()?;
    // Re-render the block without the promoted answer, or remove it entirely if
    // that answer was its only remaining entry.
    let now_empty = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.flow_entries.is_empty()
    };
    if now_empty {
        let mid = {
            let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            s.open_group_msg_id.take()
        };
        if let Some(mid) = mid {
            let _ = bot.delete_message(chat, mid).await;
        }
    } else {
        refresh_flow(bot, chat, streaming).await;
    }
    text
}

/// Pop the whole trailing run of folded `Text` entries and join them
/// (#478). Mid-turn narration is always followed by more tool calls, so
/// the trailing text run after the last tool IS the final answer — and
/// since #475 keeps ONE block across queued follow-ups, that answer can
/// be multi-part. Popping only the last entry left earlier parts
/// imprisoned in the block.
pub(crate) fn pop_trailing_folded_texts(entries: &mut Vec<FlowEntry>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    while matches!(entries.last(), Some(FlowEntry::Text(_))) {
        match entries.pop() {
            Some(FlowEntry::Text(t)) => parts.push(t),
            other => {
                if let Some(e) = other {
                    entries.push(e);
                }
                break;
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join("\n\n"))
}

/// Whether a folded intermediate is a duplicate of the final answer.
///
/// Streaming can fold only a truncated head of the final response into the
/// block (a mid-sentence prefix), so an exact match misses it: the copy left in
/// the block is usually a PREFIX of the delivered completion, not equal to it.
/// That gap is why an answer returned in `response.content` (API providers)
/// rendered both inside the collapsed block and as the completion below, while
/// the CLI path (answer reclaimed from the block) did not. Treat a substantial
/// prefix overlap in either direction as a duplicate, with a length guard so a
/// short distinct narration line that merely shares an opening is not mistaken
/// for the answer.
pub(crate) fn folded_duplicates_final(folded: &str, final_text: &str) -> bool {
    let norm_folded: String = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    let norm_final: String = final_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if norm_folded.is_empty() || norm_final.is_empty() {
        return false;
    }
    // Exact equality is a duplicate at ANY length — identical strings carry
    // zero false-positive risk. A short final answer folded verbatim used to
    // slip under the prefix length guard below and render twice: once inside
    // the collapsed block and once as the completion (#316).
    if norm_folded == norm_final {
        return true;
    }
    let overlap = norm_folded.len().min(norm_final.len());
    overlap >= 20 && (norm_final.starts_with(&norm_folded) || norm_folded.starts_with(&norm_final))
}
