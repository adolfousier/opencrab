//! OpenCode Zen (`opencode.ai`) wire-level headers.
//!
//! OpenCrabs talks to OpenCode Zen through the standard OpenAI-compatible
//! endpoint, so its requests carried nothing identifying the client. The
//! gateway asked for two things:
//!
//! - a `User-Agent`, because `reqwest` sends none by default and our traffic
//!   arrived as "Unknown client";
//! - `X-Opencode-Session`, a stable per-conversation id, so a conversation's
//!   requests group together instead of each looking like a fresh client.
//!   Requests without it may be rejected.
//!
//! The identity pair (`X-Title` / `HTTP-Referer`) mirrors what the OpenRouter
//! block already sends, so OpenCrabs names itself the same way on every
//! gateway that asks who is calling.
//!
//! All of it is scoped to OpenCode targets on purpose. A gateway fingerprints
//! the header set it receives, and [`super::qwen`] documents how headers a
//! provider does not expect drop us into a stricter bucket, so none of this
//! leaks onto other providers.

use uuid::Uuid;

/// Header carrying the per-conversation id.
const SESSION_HEADER: &str = "X-Opencode-Session";

/// Whether `base_url` points at OpenCode Zen.
///
/// Matched on the host, so every catalogue variant a deployment configures
/// (`opencode-kimi`, `opencode-qwen`, ...) is covered by one rule: they differ
/// only in model list and all post to the same host. Host-matching also means
/// a URL that merely mentions `opencode` in a path or query never qualifies.
pub fn is_opencode_target(base_url: &str) -> bool {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or(rest);
    // Strip userinfo and any port before comparing.
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.to_ascii_lowercase();
    host == "opencode.ai" || host.ends_with(".opencode.ai")
}

/// The `User-Agent` OpenCrabs identifies itself with, e.g. `OpenCrabs/0.3.83`.
/// The version comes from the crate so a release never has to remember to bump
/// a second copy.
pub fn user_agent() -> String {
    format!("OpenCrabs/{}", crate::VERSION)
}

/// A stable id for requests that reach the provider outside any conversation,
/// such as the model-catalogue fetch made before a session exists. Held for
/// the process lifetime so those stay in one bucket instead of looking like a
/// new client on every call.
fn process_session_id() -> &'static str {
    use std::sync::OnceLock;
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| Uuid::new_v4().to_string())
}

/// Headers for one request to `base_url`; empty for every non-OpenCode target.
///
/// `session` is the conversation the request belongs to. When it is absent the
/// stable per-process id stands in, because the gateway needs a well-formed id
/// more than an accurate one, and omitting the header is the failure case.
pub fn extra_headers(base_url: &str, session: Option<Uuid>) -> Vec<(String, String)> {
    if !is_opencode_target(base_url) {
        return Vec::new();
    }
    let session = session
        .map(|s| s.to_string())
        .unwrap_or_else(|| process_session_id().to_string());
    vec![
        ("User-Agent".to_string(), user_agent()),
        (SESSION_HEADER.to_string(), session),
        ("X-Title".to_string(), "OpenCrabs".to_string()),
        (
            "HTTP-Referer".to_string(),
            "https://opencrabs.com".to_string(),
        ),
    ]
}
