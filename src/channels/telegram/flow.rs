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
    /// Raw, untruncated status source (the redacted bash command) for the
    /// flow's `#`-comment status extractor (#488). `context` is the decorated,
    /// middle-truncated display hint — wrong input for extraction because the
    /// wrapper hides a first-line `#` and truncation drops comments. Empty for
    /// non-bash tools.
    pub(crate) raw_context: String,
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
    /// Whether this session's chat is a DM (owner-private). Drives scope-aware
    /// redaction: secrets show in DMs, scrub in group/channel chats (#677).
    pub(crate) is_dm: bool,
    /// Buffered `suggest_followups` options (#724). The tool fires its event
    /// mid-turn, but the buttons must be the LAST thing in the chat, so we stash
    /// the options here and render them once, after the final delivery. Only the
    /// latest set is kept if the tool fires more than once.
    pub(crate) pending_suggestions: Option<Vec<String>>,
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
    /// Live duration shown in the open block's header while the turn runs
    /// ("45s"), rendered as the trailing duration segment after the status
    /// message and count (#509). The block is the SINGLE progress surface
    /// (#360): no standalone status ticker exists while a block is open.
    /// HTML-safe (escaped at build time). None once the final response lands.
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
    /// Pre-activity header preview (thinking excerpt / Working-on line),
    /// shown in the flow header while no flow entry yields an activity
    /// preview of its own. The flow message is the ONLY status surface: the
    /// legacy pre-block status bubble is gone, so early-turn status rides
    /// here from the first activity tick.
    pub(crate) header_preview: Option<String>,
    /// Always-visible flow sections (plan title, checklist progress, active
    /// goal, ctx footer). Rolled by the edit loop from live data; ctx is set
    /// once at final delivery.
    pub(crate) sections: super::flow_chrome::FlowSections,
    /// Last active goal text sighted this turn (ADR 0005 Decision 10): the
    /// engine deletes the goal row when a plan task completes, so the chrome
    /// retains the text here and keeps the Goal section until settle. Per-turn
    /// state — a fresh StreamingState next turn drops any retained goal.
    pub(crate) retained_goal: Option<String>,
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
    /// Background-work indicator for the settled footer (#1054): computed at
    /// settle time from `agent.background_manager().running_tasks(session_id)`
    /// — `Some("<label> running")` for one task, `Some("N tasks running")`
    /// for several, `None` when nothing is detached. Rendered as the final
    /// footer segment after the clock.
    pub(crate) bg_indicator: Option<String>,
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
    /// Last plan keyboard actually applied to the open flow message via
    /// editMessageReplyMarkup (rich path only; the HTML edit path re-sends
    /// the markup inline on every edit).
    pub(crate) applied_plan_kb: super::flow_chrome::PlanKb,
    /// True from start until first response text arrives — enables rolling messages for CLI providers
    /// where tools complete instantly (ToolStarted+ToolCompleted back-to-back)
    pub(crate) processing: bool,
    /// True when the session's provider runs tools inside the CLI
    /// (`cli_handles_tools()`), i.e. claude-cli. CLI turns fold the whole model
    /// turn as intermediate narration into the block, so folded entries are
    /// capped to protect the 30K rich-block budget. API providers keep their
    /// answer in `response.content` and only fold brief interstitial narration,
    /// so they skip the per-entry cap and show full reasoning; the block-level
    /// 30K freeze still guards a pathological turn (#532 / upstream #531).
    pub(crate) is_cli: bool,
}

/// A resolved line in the processing-log flow, ready to render. Tool lines
/// carry the status label (icon + name) and context; text lines carry the
/// sanitized intermediate text. Both are HTML-escaped at render time.
pub(crate) enum FlowLine {
    Tool {
        label: String,
        /// Decorated display hint (`` (`cmd`) ``, middle-truncated).
        context: String,
        /// Raw command for `#`-comment status extraction (#488); empty for
        /// non-bash tools.
        raw_context: String,
    },
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
    // Reads raw_context, NOT the decorated/truncated display context (#488):
    // the wrapper prefix hides a first-line `#` and truncation drops comments.
    if let Some(comments) = lines.iter().rev().find_map(|l| match l {
        FlowLine::Tool {
            label, raw_context, ..
        } if is_bash_tool(label) => extract_status_from_text(raw_context),
        _ => None,
    }) {
        return Some(comments);
    }
    // Fallback: the most recent tool label + context.
    lines.iter().rev().find_map(|l| match l {
        FlowLine::Tool { label, context, .. } => Some(if context.is_empty() {
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

/// Per-entry cap for API providers. Their answer lives in `response.content`,
/// not folded text, so only brief interstitial narration folds into the block;
/// skipping the tight CLI cap keeps that reasoning readable. `usize::MAX` means
/// "no per-entry truncation" — the block-level 30K rich freeze remains the
/// guard against a pathological many-round turn (#532 / upstream #531).
const API_NARRATION_CAP: usize = usize::MAX;

/// The folded-narration cap for a turn: the tight [`FOLDED_NARRATION_CAP`] for
/// CLI providers (whose whole turn folds into the block and would fill the 30K
/// budget), or [`API_NARRATION_CAP`] (uncapped) for API providers.
fn narration_cap_for(is_cli: bool) -> usize {
    if is_cli {
        FOLDED_NARRATION_CAP
    } else {
        API_NARRATION_CAP
    }
}

/// Truncate a folded narration entry to `cap` chars on a char boundary,
/// appending an ellipsis when cut. Short entries (and any entry when
/// `cap == usize::MAX`) pass through unchanged.
fn cap_narration(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_string();
    }
    let capped: String = text.chars().take(cap).collect();
    format!("{capped}…")
}

// Channel code renders through the `_chrome` variants; these no-chrome
// wrappers stay as the test entry points pinning the renderer contract.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_flow_html(lines: &[FlowLine], live_status: Option<&str>) -> String {
    render_flow_html_with(lines, &FlowHeader::Live(live_status))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_flow_html_with(lines: &[FlowLine], header: &FlowHeader) -> String {
    render_flow_html_chrome(
        lines,
        header,
        None,
        &super::flow_chrome::FlowSections::default(),
        0,
    )
}

pub(crate) fn render_flow_html_chrome(
    lines: &[FlowLine],
    header: &FlowHeader,
    fallback_status: Option<&str>,
    sections: &super::flow_chrome::FlowSections,
    elapsed_secs: u64,
) -> String {
    render_flow_html_chrome_pref(
        lines,
        header,
        fallback_status,
        sections,
        FOLDED_NARRATION_CAP,
        elapsed_secs,
        None,
    )
}

/// Decompose the renderer's header / sections / activity into the merged-footer
/// inputs (ADR 0005 Decision 12), shared by the classic and rich paths so the
/// footer join can never drift between surfaces.
#[allow(clippy::too_many_arguments)] // one primitive per footer input; the
// decomposition IS the point (ADR 0005 Decision 12)
fn footer_parts<'a>(
    header: &'a FlowHeader,
    fallback_status: Option<&'a str>,
    sections: &'a super::flow_chrome::FlowSections,
    activity: Option<&'a str>,
    tool_count: usize,
    has_log: bool,
    elapsed_secs: u64,
    bg: Option<&'a str>,
) -> super::flow_chrome::FooterParts<'a> {
    let outcome = match header {
        FlowHeader::Settled { icon, verb, .. } => Some((*icon, *verb)),
        FlowHeader::Live(_) => None,
    };
    super::flow_chrome::FooterParts {
        outcome,
        plan_state: sections.plan_state.as_deref(),
        working_on: fallback_status,
        activity,
        tool_count,
        has_log,
        ctx: sections.ctx.as_deref(),
        elapsed_secs,
        bg,
    }
}

/// Build the flow-message body entries (tool + intermediate-text lines) shared
/// by the classic and rich renderers, plus the tool count. `Text` narration is
/// inline-formatted and capped per `narration_cap` (tight for CLI, uncapped for
/// API — #532).
fn flow_body_entries(lines: &[FlowLine], narration_cap: usize) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut tool_count = 0usize;
    for line in lines {
        match line {
            FlowLine::Tool { label, context, .. } => {
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
                    // Same inline markdown as the final completion so the
                    // expanded log is formatted, not raw source (#306); capped
                    // for CLI (#489), uncapped for API (#532). Display-only.
                    out.push(format_inline(&escape_html(&cap_narration(
                        text,
                        narration_cap,
                    ))));
                }
            }
        }
    }
    (out, tool_count)
}

/// Classic HTML flow message (ADR 0005): an uncollapsed shell, never one outer
/// expandable. The always-visible plan chrome (title / progress / goal) leads;
/// the processing log, when it has entries, sits in its own
/// `<blockquote expandable>` (full body, no summary line above it — Decision
/// 11/12); the merged footer is the plain final line (Decision 3).
/// `elapsed_secs` drives the footer clock (Decision 13).
pub(crate) fn render_flow_html_chrome_pref(
    lines: &[FlowLine],
    header: &FlowHeader,
    fallback_status: Option<&str>,
    sections: &super::flow_chrome::FlowSections,
    narration_cap: usize,
    elapsed_secs: u64,
    bg: Option<&str>,
) -> String {
    let (out, tool_count) = flow_body_entries(lines, narration_cap);
    let has_log = !out.is_empty();
    let activity = latest_activity_preview(lines);
    let footer = super::flow_chrome::merged_footer(
        &footer_parts(
            header,
            fallback_status,
            sections,
            activity.as_deref(),
            tool_count,
            has_log,
            elapsed_secs,
            bg,
        ),
        HeaderMarkup::Html,
    );
    // Always-visible plan chrome in the locked vertical order (title, prose
    // expandables, ☐/☑ checklist rows, goal — ADR 0005 Decision 3), assembled
    // for the classic Bot API HTML dialect (blank lines stand in for the rich
    // <hr> boundaries — Decision 13). Settled renders swap a completed goal's
    // icon to ✅ (Decision 10).
    let chrome = sections.chrome_classic(matches!(header, FlowHeader::Settled { .. }));

    let mut msg = String::new();
    if !chrome.is_empty() {
        msg.push_str(&chrome);
    }
    // A blank line separates any chrome above from the log/footer cluster
    // (Decision 13, classic uses blank lines). The log body is the full entry
    // list in one expandable with no summary line above it; the merged footer
    // is the plain final line under it.
    if has_log {
        if !msg.is_empty() {
            msg.push_str("\n\n");
        }
        msg.push_str(&format!(
            "<blockquote expandable>{}</blockquote>\n{footer}",
            out.join("\n\n")
        ));
    } else {
        if !msg.is_empty() {
            msg.push_str("\n\n");
        }
        msg.push_str(&footer);
    }
    msg
}

/// Render resolved flow lines as a `<details><summary>` collapsible for the
/// rich API's HTML input mode (#420 path A): the server parses it into a
/// native RichBlockDetails (summary/blocks/is_open), giving collapse parity
/// with the classic `<blockquote expandable>` PLUS the 32K rich limit, so
/// long tool chains stop splitting into multiple blocks. The summary carries
/// the live header (#360) and the latest-activity preview (#405); the
/// chronological log is the collapsed body. A lone tool line stays a plain
/// one-liner (mirrors #296, same as the HTML renderer).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_flow_details(lines: &[FlowLine], live_status: Option<&str>) -> String {
    render_flow_details_with(lines, &FlowHeader::Live(live_status))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_flow_details_with(lines: &[FlowLine], header: &FlowHeader) -> String {
    render_flow_details_chrome(
        lines,
        header,
        None,
        &super::flow_chrome::FlowSections::default(),
        0,
    )
}

pub(crate) fn render_flow_details_chrome(
    lines: &[FlowLine],
    header: &FlowHeader,
    fallback_status: Option<&str>,
    sections: &super::flow_chrome::FlowSections,
    elapsed_secs: u64,
) -> String {
    render_flow_details_chrome_pref(
        lines,
        header,
        fallback_status,
        sections,
        FOLDED_NARRATION_CAP,
        elapsed_secs,
        None,
    )
}

/// Rich `<details>` flow message (#420 path A) reshaped for ADR 0005: an
/// uncollapsed shell, never one outer expandable. Always-visible plan chrome
/// leads as a `<p>` block; a `<p>&nbsp;</p>` spacer precedes the footer when
/// chrome is present (Decision 13); the merged footer is a `<sub>` line, and
/// when the log has entries that `<sub>` becomes the processing-log `<summary>`
/// with the full entry list as the collapsed body (Decision 12). `elapsed_secs`
/// drives the footer clock.
pub(crate) fn render_flow_details_chrome_pref(
    lines: &[FlowLine],
    header: &FlowHeader,
    fallback_status: Option<&str>,
    sections: &super::flow_chrome::FlowSections,
    narration_cap: usize,
    elapsed_secs: u64,
    bg: Option<&str>,
) -> String {
    let (out, tool_count) = flow_body_entries(lines, narration_cap);
    let has_log = !out.is_empty();
    let activity = latest_activity_preview(lines);
    let footer = super::flow_chrome::merged_footer(
        &footer_parts(
            header,
            fallback_status,
            sections,
            activity.as_deref(),
            tool_count,
            has_log,
            elapsed_secs,
            bg,
        ),
        HeaderMarkup::Html,
    );

    let mut msg = String::new();
    let chrome = sections.chrome_rich(matches!(header, FlowHeader::Settled { .. }));
    if !chrome.is_empty() {
        // Always-visible plan chrome in the locked vertical order: title flush
        // against the per-heading prose <details>, <hr> boundaries, ☐/☑
        // checklist rows, goal (ADR 0005 Decision 3/12/13); a kept spacer
        // follows before the footer.
        msg.push_str(&chrome);
        msg.push_str("<p>&nbsp;</p>");
    }
    if has_log {
        // The merged footer is the processing-log summary; the body is the full
        // <p>-wrapped entry list (one <p> per entry so the rich parser keeps
        // them separated).
        let body: String = out.iter().map(|e| format!("<p>{e}</p>")).collect();
        msg.push_str(&format!(
            "<details><summary><sub>{footer}</sub></summary>{body}</details>"
        ));
    } else {
        // No log yet: a plain <sub> footer line.
        msg.push_str(&format!("<sub>{footer}</sub>"));
    }
    msg
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
            FlowLine::Tool { label, context, .. } => {
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
        // Header-only render: the markdown path allows empty entries too so
        // the three renderers agree on the header-only contract.
        return flow_header_text(
            tool_count,
            &FlowHeader::Live(live_status),
            None,
            HeaderMarkup::Markdown,
        );
    }
    if out.len() == 1 && tool_count == 1 {
        // Lone tool line stays plain (#296); the live status rides on it so
        // the single surface still shows progress from the first call (#360).
        return match live_status {
            Some(st) => format!("{} • {}", out.remove(0), st),
            None => out.remove(0),
        };
    }
    // Same latest-activity preview as the HTML renderer (#405), leading the
    // header status-first (#509); this markdown path is always live. Raw text,
    // no escaping — the markdown dialect keeps narration verbatim.
    let status_msg = latest_activity_preview(lines);
    let header = flow_header_text(
        tool_count,
        &FlowHeader::Live(live_status),
        status_msg.as_deref(),
        HeaderMarkup::Markdown,
    );
    format!("{header}\n\n{}", out.join("\n\n"))
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
    /// Turn in progress; the payload is the elapsed duration. With a status
    /// message the header reads `⚙️ {status} • N tool calls • {duration}` (#509);
    /// with neither status nor duration it is the plain `N tool calls` /
    /// `Processing log`.
    Live(Option<&'a str>),
    /// Turn settled: `{icon} {verb} (N tool calls, {duration})`, dropping the
    /// `N tool calls` clause when no tools ran.
    Settled {
        icon: &'a str,
        verb: &'a str,
        duration: &'a str,
    },
}

/// Inline-markup dialect for the header: the classic/rich-details HTML paths
/// want `<b>`/`<i>`, the rich-markdown path wants `**`/`_`. Kept here so the
/// bold/italic emphasis is applied inside the shared builder and the three
/// renderers can never disagree on where the emphasis falls.
#[derive(Clone, Copy)]
pub(crate) enum HeaderMarkup {
    Html,
    Markdown,
}

impl HeaderMarkup {
    pub(crate) fn bold(self, s: &str) -> String {
        match self {
            HeaderMarkup::Html => format!("<b>{s}</b>"),
            HeaderMarkup::Markdown => format!("**{s}**"),
        }
    }

    pub(crate) fn italic(self, s: &str) -> String {
        match self {
            HeaderMarkup::Html => format!("<i>{s}</i>"),
            HeaderMarkup::Markdown => format!("_{s}_"),
        }
    }
}

/// Strip a leading running-gear (`⚙️`, base `⚙` plus its optional variation
/// selector) and any following whitespace from a status message, so the live
/// header's own gear is never doubled when the status is the bare-tool fallback
/// (#509 follow-up). Text that does not start with a gear is returned unchanged.
fn strip_leading_gear(s: &str) -> &str {
    s.trim_start_matches(['⚙', '\u{fe0f}']).trim_start()
}

/// Build the fully-styled header shared by all three renderers so the classic
/// HTML, rich-details, and rich-markdown headers can never drift (#480, #509).
/// The live header leads with the status message (bold), then the tool-call
/// count, then the duration (both italic), `•`-separated: `⚙️ status • count •
/// duration`. `status_msg` is the activity preview the renderer already escapes
/// for its own dialect; the `Live` payload carries the duration. The
/// plain-count (nothing running yet) and settled headers stay fully bold.
pub(crate) fn flow_header_text(
    tool_count: usize,
    header: &FlowHeader,
    status_msg: Option<&str>,
    markup: HeaderMarkup,
) -> String {
    let base = if tool_count > 0 {
        format!("{tool_count} tool calls")
    } else {
        "Processing log".to_string()
    };
    match header {
        FlowHeader::Live(duration) => {
            // No status message and no elapsed duration → the just-started case:
            // the plain bold count / "Processing log", no gear (#509).
            if status_msg.is_none() && duration.is_none() {
                return markup.bold(&base);
            }
            // Ordered live header: status message FIRST (bold), then the count,
            // then the duration (both italic), all `•`-separated (#509).
            let mut segs: Vec<String> = Vec::new();
            if let Some(status) = status_msg {
                // The header already prints one live gear; strip a leading
                // running-gear from the status message so the bare-tool fallback
                // ("⚙️ bash …") does not render a double gear (#509 follow-up).
                // Settled ✅/❌ tool icons are left alone: they read as the tool's
                // own outcome, not a duplicate of the header gear.
                segs.push(markup.bold(strip_leading_gear(status)));
            }
            segs.push(markup.italic(&base));
            if let Some(dur) = duration {
                segs.push(markup.italic(dur));
            }
            format!("⚙️ {}", segs.join(" • "))
        }
        FlowHeader::Settled {
            icon,
            verb,
            duration,
        } => {
            let text = if tool_count > 0 {
                format!("{icon} {verb} ({tool_count} tool calls, {duration})")
            } else {
                format!("{icon} {verb} ({duration})")
            };
            markup.bold(&text)
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
                raw_context: t.raw_context.clone(),
            }),
            FlowEntry::Text(text) => Some(FlowLine::Text(text.clone())),
        })
        .collect()
}

/// Resolve the flow into final Telegram HTML. Live turn → the plain live
/// header; a settled turn → the terminal outcome header with wall-clock
/// duration from turn start (#480).
pub(crate) fn render_flow(s: &StreamingState) -> String {
    let narration_cap = narration_cap_for(s.is_cli);
    let elapsed = s.turn_started_at.elapsed().as_secs();
    match s.flow_outcome {
        Some(outcome) => {
            let (icon, verb) = outcome.icon_verb();
            let duration = humanize_duration(elapsed);
            render_flow_html_chrome_pref(
                &flow_lines(s),
                &FlowHeader::Settled {
                    icon,
                    verb,
                    duration: &duration,
                },
                None,
                &s.sections,
                narration_cap,
                elapsed,
                s.bg_indicator.as_deref(),
            )
        }
        None => render_flow_html_chrome_pref(
            &flow_lines(s),
            &FlowHeader::Live(s.flow_status.as_deref()),
            s.header_preview.as_deref(),
            &s.sections,
            narration_cap,
            elapsed,
            s.bg_indicator.as_deref(),
        ),
    }
}

/// Resolve the flow into the rich-API details HTML (#420 path A), with the same
/// live/settled header split as [`render_flow`].
pub(crate) fn render_flow_details_state(s: &StreamingState) -> String {
    let narration_cap = narration_cap_for(s.is_cli);
    let elapsed = s.turn_started_at.elapsed().as_secs();
    match s.flow_outcome {
        Some(outcome) => {
            let (icon, verb) = outcome.icon_verb();
            let duration = humanize_duration(elapsed);
            render_flow_details_chrome_pref(
                &flow_lines(s),
                &FlowHeader::Settled {
                    icon,
                    verb,
                    duration: &duration,
                },
                None,
                &s.sections,
                narration_cap,
                elapsed,
                s.bg_indicator.as_deref(),
            )
        }
        None => render_flow_details_chrome_pref(
            &flow_lines(s),
            &FlowHeader::Live(s.flow_status.as_deref()),
            s.header_preview.as_deref(),
            &s.sections,
            narration_cap,
            elapsed,
            s.bg_indicator.as_deref(),
        ),
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

/// How a failed rich-details edit should be recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RichEditError {
    /// Telegram reports the content is unchanged — nothing to do.
    NotModified,
    /// A 429 rate limit — skip and retry rich next tick (never fall back to
    /// HTML, whose smaller limit would split the block; #580).
    RateLimited,
    /// Any other failure — fall back to the HTML edit path.
    Fallback,
}

/// Classify a rich-edit error string into a recovery action. Kept pure so the
/// rate-limit-vs-fallback decision is unit-testable without a live Bot API.
pub(crate) fn classify_rich_edit_error(msg: &str) -> RichEditError {
    if msg.contains("message is not modified") {
        RichEditError::NotModified
    } else if msg.contains("429") || msg.contains("Too Many Requests") {
        RichEditError::RateLimited
    } else {
        RichEditError::Fallback
    }
}

/// Rich-details edit path (#420 path A). 32K char limit, 30K freeze
/// threshold. A not-modified response is a no-op; a 429 is retried on the rich
/// path next tick; any other failure falls back to the classic HTML edit so the
/// block never silently stops updating.
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
        freeze_flow_block_and_strip_kb(bot, chat, streaming, mid, "rich size limit reached").await;
        return;
    }
    match super::rich::api::edit_rich_html(bot.api_url().as_str(), bot.token(), chat.0, mid.0, &details, None, "turn", "-")
        .await
    {
        // The plan Approve/Discard keyboard now rides the persistent plan card,
        // not the flow block (#580), so the flow block carries no reply_markup.
        Ok(_) => {}
        Err(e) => match classify_rich_edit_error(&e.to_string()) {
            RichEditError::NotModified => {}
            // A transient 429 must NOT fall back to HTML. The rich API holds 32K
            // chars; the HTML path caps at 4096, so a large block that fits rich
            // gets frozen and split into a second message on the smaller HTML
            // limit — that is what split a long plan-execution block into "…23
            // tool calls" + a separate "Finished" block (#580). Skip this tick
            // instead: the ~1.5s edit loop retries the rich edit, and once the
            // rate limit clears the block updates whole, never splitting.
            RichEditError::RateLimited => {
                tracing::warn!(
                    "Telegram: rich details edit rate-limited for mid={:?} — skipping this tick, \
                     will retry rich (not HTML, to avoid a size-split)",
                    mid
                );
            }
            // Non-rate-limit errors (malformed content, etc.) fall back to HTML,
            // which is the right recovery there.
            RichEditError::Fallback => {
                tracing::warn!(
                    "Telegram: rich details edit failed for mid={:?}: {} — falling back to HTML",
                    mid,
                    e
                );
                refresh_flow_html(bot, chat, mid, streaming).await;
            }
        },
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
        freeze_flow_block_and_strip_kb(bot, chat, streaming, mid, "size limit reached").await;
        return;
    }
    // The plan Approve/Discard keyboard now rides the persistent plan card, not
    // the flow block (#580), so no reply_markup is attached here.
    let req = bot
        .edit_message_text(chat, mid, html)
        .parse_mode(ParseMode::Html);
    match req.await {
        Ok(_) => {}
        // Transient rate limit: wait it out and retry once with fresh
        // content. Deleting here used to wipe a fully rendered report off
        // the screen over a 9-second throttle (#356).
        Err(teloxide::RequestError::RetryAfter(secs)) => {
            super::rate_limit::wait_out(
                "refresh_flow",
                secs.duration(),
                &format!(" for mid={mid:?}, then retrying"),
            )
            .await;
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
                freeze_flow_block_and_strip_kb(bot, chat, streaming, mid, "MESSAGE_TOO_LONG").await;
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

/// A mid-turn user follow-up landed (queued-message injection). Since #475 the
/// open processing-log block is NOT closed or frozen and its entries are NOT
/// dropped: the block stays open with its content visible above the follow-up,
/// and only the response placeholder is marked for re-post. #451's restick then
/// relocates the SAME block below the newest message on the next tool round, so
/// the chat keeps flowing bottom-down with one block per turn (#404, #475).
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

/// Freeze the current flow block AND strip any plan keyboard from the sealed
/// prior message, so only the next live flow message can own Approve/Discard
/// (ADR 0005 Decision 6). [`freeze_flow_block`] is sync with no bot handle, so
/// the keyboard strip lives here where `bot`/`chat` are in scope. No-op strip
/// when the message never carried a plan keyboard (the common non-plan freeze).
async fn freeze_flow_block_and_strip_kb(
    bot: &Bot,
    chat: ChatId,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    mid: MessageId,
    reason: &str,
) {
    let had_kb = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.sections.plan_kb != super::flow_chrome::PlanKb::None
            || s.applied_plan_kb != super::flow_chrome::PlanKb::None
    };
    freeze_flow_block(streaming, mid, reason);
    if had_kb {
        // Omitting reply_markup clears the inline keyboard on the sealed message.
        if let Err(e) = bot.edit_message_reply_markup(chat, mid).await {
            tracing::debug!("Failed to strip plan keyboard from frozen flow {mid}: {e}");
        }
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.applied_plan_kb = super::flow_chrome::PlanKb::None;
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
    // Already open: edit in place, never post a second block.
    //
    // This function's contract is "send the open processing-log message for
    // the FIRST time", and nothing enforced it. append_tool_group guarded at
    // its call site, but the chrome tick calls open_flow directly on every
    // activity update, so a long turn posted a new block per tick. Once plan
    // title, prose and checklist moved to the card, the only content left was
    // the elapsed time, so the chat filled with timer-only bubbles seconds
    // apart. Guarding here covers every caller instead of relying on each one
    // to remember.
    let already_open = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.open_group_msg_id.is_some()
    };
    if already_open {
        refresh_flow(bot, chat, streaming).await;
        return;
    }

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
            match super::rich::api::send_rich_html_id(
                bot.api_url().as_str(),
                bot.token(),
                chat.0,
                thread_id,
                &details,
                None,
                "turn",
                "-",
            )
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
    if let Ok(mid) = send_html_or_plain(bot, chat, thread_id, &html, "turn").await {
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
/// Stable key for a line that is *progress on one condition* rather than a new
/// event (#982).
///
/// Repeated progress used to stack: five `nudge 1/5 .. 5/5` lines and one line
/// per fallback attempt, nine messages for two facts. Everything else in this
/// block already rewrites itself, a tool row flips its icon and the footer
/// rewrites ctx and tok/s, so these were the exception. Lines sharing a key
/// supersede each other; ordinary narration returns `None` and always appends.
pub(crate) fn progress_key(text: &str) -> Option<&'static str> {
    let t = text.trim_start_matches(|c: char| !c.is_alphanumeric());
    if t.starts_with("Model reasoned without answering") {
        Some("empty-answer-nudge")
    } else if t.starts_with("Trying fallback") {
        Some("fallback-attempt")
    } else if t.starts_with("Retry ") {
        Some("provider-retry")
    } else {
        None
    }
}

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
        // Supersede the previous line when this is progress on the same
        // condition, so a counter advances in place instead of stacking (#982).
        // Only ever the IMMEDIATELY preceding entry: anything in between means
        // the context moved on and the new line is genuinely new.
        let supersede = progress_key(text).is_some_and(|k| {
            matches!(s.flow_entries.last(), Some(FlowEntry::Text(prev)) if progress_key(prev) == Some(k))
        });
        if supersede {
            if let Some(FlowEntry::Text(slot)) = s.flow_entries.last_mut() {
                *slot = text.to_string();
            }
        } else {
            s.flow_entries.push(FlowEntry::Text(text.to_string()));
        }
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
        match super::rich::api::send_rich_html_id(
            bot.api_url().as_str(),
            bot.token(),
            chat.0,
            thread_id,
            &details,
            None,
            "turn",
            "-",
        )
        .await
        {
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
        match send_html_or_plain(bot, chat, thread_id, &html, "turn").await {
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
    // The plan Approve/Discard keyboard rides the persistent plan card, not the
    // flow block (#580), so a relocated block re-posts bare — nothing to
    // re-attach here.
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
/// without it (header-only when it empties), and returns the text.
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
    // Re-render the block without the promoted answer. An emptied block is
    // NOT deleted anymore: the flow message is the turn's chrome surface
    // (header, sections, ctx) and settles header-only at turn end, same as a
    // no-tool long turn.
    refresh_flow(bot, chat, streaming).await;
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
