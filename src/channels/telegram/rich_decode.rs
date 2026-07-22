//! Native readable decoding of rich Bot API content types (#359).
//!
//! When a message carries content the handler cannot surface as plain text
//! (checklists, polls, giveaways, paid media, gifts, stories, shared
//! users/chats), decode the raw Bot API payload into readable text instead of
//! handing the agent a JSON dump. Field names mirror the Bot API objects
//! exactly (verified against teloxide-core 0.13's serde definitions).
//!
//! `decode_rich_content` returns `None` for anything it does not recognize;
//! the raw-JSON path in the caller remains the permanent safety net for
//! whatever content type comes next.

use serde_json::Value;

/// Try to render the message's rich content as readable text. `raw` is the
/// full raw message object as delivered by getUpdates. Returns the first
/// recognized content rendering, or `None` when no decoder matches.
pub(crate) fn decode_rich_content(raw: &Value) -> Option<String> {
    let caption = raw.get("caption").and_then(Value::as_str);
    let decoded = if let Some(v) = raw.get("checklist") {
        decode_checklist(v)
    } else if let Some(v) = raw.get("poll") {
        decode_poll(v)
    } else if let Some(v) = raw.get("paid_media") {
        decode_paid_media(v)
    } else if let Some(v) = raw.get("giveaway") {
        decode_giveaway(v)
    } else if let Some(v) = raw.get("giveaway_winners") {
        decode_giveaway_winners(v)
    } else if let Some(v) = raw.get("giveaway_completed") {
        decode_giveaway_completed(v)
    } else if raw.get("giveaway_created").is_some() {
        Some("A giveaway was started in this chat.".to_string())
    } else if let Some(v) = raw.get("gift") {
        decode_gift(v)
    } else if let Some(v) = raw.get("unique_gift") {
        decode_unique_gift(v)
    } else if let Some(v) = raw.get("story") {
        decode_story(v)
    } else if let Some(v) = raw.get("users_shared") {
        decode_users_shared(v)
    } else if let Some(v) = raw.get("chat_shared") {
        decode_chat_shared(v)
    } else if let Some(v) = raw.get("rich_message") {
        decode_rich_message(v)
    } else {
        None
    }?;
    Some(match caption {
        Some(c) if !c.trim().is_empty() => format!("{decoded}\nCaption: {c}"),
        _ => decoded,
    })
}

/// Decode a `rich_message` payload (`sendRichMessage`, Bot API 10.1) into
/// readable text (#686). Peer OpenCrabs bots post via this, and teloxide leaves
/// `text()`/`caption()` empty, so without this the content is lost or dumped as
/// raw JSON. Two input modes, mirroring how we SEND: `{"html": "..."}` (markdown
/// input mode) and `{"blocks": [...]}` (our `render_json` block AST). Returns
/// `None` when neither carries text.
fn decode_rich_message(v: &Value) -> Option<String> {
    if let Some(html) = v.get("html").and_then(Value::as_str) {
        let text = html_to_readable_text(html);
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    if let Some(blocks) = v.get("blocks").and_then(Value::as_array) {
        let text = collect_blocks_text(blocks);
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

/// Strip HTML tags and decode the handful of entities Telegram's HTML mode
/// uses, yielding plain readable text. Not a general HTML parser — the input is
/// our own restricted rich-message HTML.
fn html_to_readable_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

/// Extract readable text from a `render_json` block array: one line per
/// top-level block, inline text concatenated within a block. Empty blocks
/// (e.g. dividers with no text) are dropped.
fn collect_blocks_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .map(|b| {
            let mut s = String::new();
            gather_leaf_text(b, &mut s);
            s
        })
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Recursively gather `text` leaves from a block/inline value, walking the
/// `content` / `blocks` / `items` / `rows` / `cells` nesting the `render_json`
/// AST uses. Generic on purpose so a new block or inline variant still
/// contributes its text rather than silently dropping.
fn gather_leaf_text(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            if let Some(t) = map.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
            for key in ["content", "blocks", "items", "rows", "cells"] {
                if let Some(child) = map.get(key) {
                    gather_leaf_text(child, out);
                }
            }
        }
        Value::Array(arr) => arr.iter().for_each(|it| gather_leaf_text(it, out)),
        _ => {}
    }
}

/// Checklist (Bot API 9.1): title plus one `[x]`/`[ ]` line per task. A task
/// counts as done when the payload carries `completed_by_user` or a non-zero
/// `completion_date`.
fn decode_checklist(v: &Value) -> Option<String> {
    let title = v.get("title").and_then(Value::as_str)?;
    let mut out = format!("Checklist: {title}");
    for task in v.get("tasks").and_then(Value::as_array)? {
        let text = task.get("text").and_then(Value::as_str).unwrap_or("");
        let done = task.get("completed_by_user").is_some()
            || task
                .get("completion_date")
                .and_then(Value::as_i64)
                .is_some_and(|d| d != 0);
        let mark = if done { "x" } else { " " };
        out.push_str(&format!("\n[{mark}] {text}"));
    }
    Some(out)
}

fn decode_poll(v: &Value) -> Option<String> {
    let question = v.get("question").and_then(Value::as_str)?;
    let anon = if v.get("is_anonymous").and_then(Value::as_bool) == Some(true) {
        " (anonymous)"
    } else {
        ""
    };
    let closed = if v.get("is_closed").and_then(Value::as_bool) == Some(true) {
        " [closed]"
    } else {
        ""
    };
    let mut out = format!("Poll{anon}{closed}: {question}");
    for opt in v.get("options").and_then(Value::as_array)? {
        let text = opt.get("text").and_then(Value::as_str).unwrap_or("");
        let votes = opt.get("voter_count").and_then(Value::as_u64).unwrap_or(0);
        out.push_str(&format!("\n- {text} ({votes} votes)"));
    }
    if let Some(total) = v.get("total_voter_count").and_then(Value::as_u64) {
        out.push_str(&format!("\nTotal voters: {total}"));
    }
    Some(out)
}

fn decode_paid_media(v: &Value) -> Option<String> {
    let items = v.get("paid_media").and_then(Value::as_array)?;
    let stars = v.get("star_count").and_then(Value::as_u64).unwrap_or(0);
    let kinds: Vec<&str> = items
        .iter()
        .map(|i| i.get("type").and_then(Value::as_str).unwrap_or("media"))
        .collect();
    Some(format!(
        "Paid media: {} item(s) ({}) behind a {stars}-star paywall. The media \
         itself is not accessible until paid for.",
        items.len(),
        kinds.join(", ")
    ))
}

fn decode_giveaway(v: &Value) -> Option<String> {
    let winners = v.get("winner_count").and_then(Value::as_u64)?;
    let mut out = format!("Giveaway: {winners} winner(s) will be selected");
    if let Some(ts) = v
        .get("winners_selection_date")
        .and_then(Value::as_i64)
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
    {
        out.push_str(&format!(" on {}", ts.format("%Y-%m-%d %H:%M UTC")));
    }
    out.push('.');
    if let Some(desc) = v.get("prize_description").and_then(Value::as_str) {
        out.push_str(&format!("\nPrize: {desc}"));
    }
    if let Some(stars) = v.get("prize_star_count").and_then(Value::as_u64) {
        out.push_str(&format!("\nPrize pool: {stars} stars"));
    }
    if let Some(months) = v
        .get("premium_subscription_month_count")
        .and_then(Value::as_u64)
    {
        out.push_str(&format!("\nPrize: {months} month(s) of Telegram Premium"));
    }
    Some(out)
}

fn decode_giveaway_winners(v: &Value) -> Option<String> {
    let count = v.get("winner_count").and_then(Value::as_u64)?;
    let mut out = format!("Giveaway finished: {count} winner(s)");
    let names: Vec<String> = v
        .get("winners")
        .and_then(Value::as_array)
        .map(|ws| ws.iter().filter_map(user_label).collect())
        .unwrap_or_default();
    if !names.is_empty() {
        out.push_str(&format!(": {}", names.join(", ")));
    }
    if let Some(desc) = v.get("prize_description").and_then(Value::as_str) {
        out.push_str(&format!("\nPrize: {desc}"));
    }
    Some(out)
}

fn decode_giveaway_completed(v: &Value) -> Option<String> {
    let count = v.get("winner_count").and_then(Value::as_u64)?;
    let mut out = format!("Giveaway completed: {count} winner(s)");
    if let Some(unclaimed) = v.get("unclaimed_prize_count").and_then(Value::as_u64) {
        out.push_str(&format!(", {unclaimed} prize(s) unclaimed"));
    }
    Some(out)
}

fn decode_gift(v: &Value) -> Option<String> {
    let gift = v.get("gift")?;
    let stars = gift.get("star_count").and_then(Value::as_u64).unwrap_or(0);
    let mut out = format!("A gift was received (worth {stars} stars).");
    if let Some(text) = v.get("text").and_then(Value::as_str) {
        out.push_str(&format!("\nMessage with the gift: {text}"));
    }
    Some(out)
}

fn decode_unique_gift(v: &Value) -> Option<String> {
    let gift = v.get("gift")?;
    let base = gift.get("base_name").and_then(Value::as_str)?;
    let mut out = format!("A unique collectible gift was received: {base}");
    if let Some(num) = gift.get("number").and_then(Value::as_u64) {
        out.push_str(&format!(" #{num}"));
    }
    Some(out)
}

fn decode_story(v: &Value) -> Option<String> {
    let poster = v
        .get("chat")
        .map(chat_label)
        .unwrap_or_else(|| "an unknown chat".to_string());
    Some(format!(
        "A story posted by {poster}. Story media itself is not accessible \
         through the Bot API."
    ))
}

fn decode_users_shared(v: &Value) -> Option<String> {
    let users = v.get("users").and_then(Value::as_array)?;
    let labels: Vec<String> = users.iter().filter_map(user_label).collect();
    if labels.is_empty() {
        return None;
    }
    Some(format!("Shared user(s): {}", labels.join(", ")))
}

fn decode_chat_shared(v: &Value) -> Option<String> {
    let id = v.get("chat_id").and_then(Value::as_i64)?;
    let mut label = v
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.to_string())
        .unwrap_or_else(|| format!("chat id {id}"));
    if let Some(u) = v.get("username").and_then(Value::as_str) {
        label.push_str(&format!(" (@{u})"));
    }
    Some(format!("Shared chat: {label}"))
}

/// Readable label for a Bot API User or SharedUser object: name parts plus
/// @username when present, falling back to the numeric id.
fn user_label(u: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(first) = u.get("first_name").and_then(Value::as_str) {
        parts.push(first.to_string());
    }
    if let Some(last) = u.get("last_name").and_then(Value::as_str) {
        parts.push(last.to_string());
    }
    let mut label = parts.join(" ");
    if let Some(username) = u.get("username").and_then(Value::as_str) {
        if label.is_empty() {
            label = format!("@{username}");
        } else {
            label.push_str(&format!(" (@{username})"));
        }
    }
    if label.is_empty() {
        let id = u
            .get("id")
            .or_else(|| u.get("user_id"))
            .and_then(Value::as_i64)?;
        label = format!("user id {id}");
    }
    Some(label)
}

/// Readable label for a Bot API Chat object: title, @username, or first name.
fn chat_label(c: &Value) -> String {
    if let Some(title) = c.get("title").and_then(Value::as_str) {
        return format!("\"{title}\"");
    }
    if let Some(username) = c.get("username").and_then(Value::as_str) {
        return format!("@{username}");
    }
    if let Some(first) = c.get("first_name").and_then(Value::as_str) {
        return first.to_string();
    }
    "an unknown chat".to_string()
}
