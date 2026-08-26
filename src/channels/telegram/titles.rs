//! Best-effort Bot API title lookups for human-readable sender labels on
//! session_notify echo bubbles (#1225). Raw HTTP, same wire style as
//! `rich/api.rs`: teloxide 0.17 has no `getForumTopic` binding (docs.rs 404,
//! verified) and a uniform raw call keeps `getChat` and `getForumTopic` on
//! one tiny code path.
//!
//! Every lookup is best-effort — `None` on any failure feeds the short-id
//! fallback in `resume.rs`, never an error bubble.

use std::time::Duration;
use teloxide::types::{ChatId, ThreadId};

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

/// Chat display name: `title` for groups/channels, `username` as fallback
/// for private chats (getChat leaves `title` absent on Private).
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
}

/// Forum topic display name: getForumTopic → `result.name`.
pub(crate) async fn topic_title(
    api_url: &str,
    token: &str,
    chat_id: ChatId,
    topic_id: ThreadId,
) -> Option<String> {
    let result = post_api(
        api_url,
        token,
        "getForumTopic",
        serde_json::json!({ "chat_id": chat_id.0, "message_thread_id": topic_id.0.0 }),
    )
    .await?;
    result
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}
