//! Best-effort Bot API title lookups for human-readable sender labels on
//! session_notify echo bubbles (#1225). Raw HTTP, same wire style as
//! `rich/api.rs`, kept on one tiny code path.
//!
//! Only `getChat` lives here: it is a real Bot API method and covers groups,
//! channels and private chats. Forum-TOPIC names do NOT belong here — the
//! Bot API has no method to query a topic's title (bots can only learn names
//! passively; telegram-bot-api issues #634/#356), so `resume.rs` resolves
//! those from the local `channel_messages.topic_name` store instead.
//!
//! Every lookup is best-effort — `None` on any failure feeds the short-id
//! fallback in `resume.rs`, never an error bubble.

use std::time::Duration;
use teloxide::types::ChatId;

fn api_base(api_url: &str) -> &str {
    api_url.trim_end_matches('/')
}

/// One-shot POST to a Bot API method; returns `result` on `ok: true`.
async fn post_api(
    api_url: &str,
    token: &str,
    method: &str,
    body: serde_json::Value,
) -> Option<serde_json::Value> {
    let url = format!("{}/bot{token}/{method}", api_base(api_url));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let resp = client.post(url).json(&body).send().await.ok()?;
    let parsed: serde_json::Value = resp.json().await.ok()?;
    if parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        parsed.get("result").cloned()
    } else {
        None
    }
}

/// Chat display name: `title` for groups/channels; on private chats (no
/// title) the account's `username`, else its first/last name.
pub(crate) async fn chat_title(api_url: &str, token: &str, chat_id: ChatId) -> Option<String> {
    let result = post_api(
        api_url,
        token,
        "getChat",
        serde_json::json!({ "chat_id": chat_id.0 }),
    )
    .await?;
    result
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            result
                .get("username")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            // Private chats have no `title` and not every account has a
            // username — fall back to the account's display name.
            let first = result.get("first_name").and_then(serde_json::Value::as_str);
            let last = result.get("last_name").and_then(serde_json::Value::as_str);
            match (first, last) {
                (Some(f), Some(l)) => Some(format!("{f} {l}")),
                (Some(f), None) => Some(f.to_owned()),
                _ => None,
            }
        })
}
