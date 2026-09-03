//! OpenCode Zen identifies clients by header: a `User-Agent` (ours arrived as
//! "Unknown client" because `reqwest` sends none by default) and a stable
//! `X-Opencode-Session` per conversation, without which requests may be
//! rejected.

use crate::brain::provider::opencode::{extra_headers, is_opencode_target, user_agent};
use uuid::Uuid;

const ZEN: &str = "https://opencode.ai/zen/go/v1";

fn get<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn every_configured_catalogue_variant_shares_one_host_rule() {
    // The deployment configures several providers that differ only in model
    // list; all post to the same host and all must be recognised.
    assert!(is_opencode_target(ZEN));
    assert!(is_opencode_target(
        "https://opencode.ai/zen/go/v1/chat/completions"
    ));
    assert!(is_opencode_target("https://api.opencode.ai/v1"));
    assert!(is_opencode_target("HTTPS://OpenCode.AI/zen/go/v1"));
    assert!(is_opencode_target("https://opencode.ai:443/zen/go/v1"));
}

/// Host-matched, so a look-alike host or an unrelated URL that merely mentions
/// the name never picks up the headers.
#[test]
fn non_opencode_targets_are_not_matched() {
    assert!(!is_opencode_target("https://openrouter.ai/api/v1"));
    assert!(!is_opencode_target("https://api.z.ai/api/coding/paas/v4"));
    assert!(!is_opencode_target("http://localhost:8888/v1"));
    assert!(!is_opencode_target(
        "https://evil-opencode.ai.example.com/v1"
    ));
    assert!(!is_opencode_target("https://example.com/opencode.ai/v1"));
}

#[test]
fn opencode_requests_carry_a_user_agent_and_the_session() {
    let session = Uuid::new_v4();
    let h = extra_headers(ZEN, Some(session));

    assert_eq!(
        get(&h, "User-Agent"),
        Some(user_agent().as_str()),
        "requests arrived as 'Unknown client' without this"
    );
    assert!(user_agent().starts_with("OpenCrabs/"));
    assert_eq!(
        get(&h, "X-Opencode-Session"),
        Some(session.to_string().as_str()),
        "the session header must carry the conversation's own id"
    );
}

/// Same conversation, many requests: the id has to be identical every time or
/// it is not a session.
#[test]
fn the_session_id_is_stable_across_requests_in_one_conversation() {
    let session = Uuid::new_v4();
    let first = extra_headers(ZEN, Some(session));
    let second = extra_headers(ZEN, Some(session));
    assert_eq!(
        get(&first, "X-Opencode-Session"),
        get(&second, "X-Opencode-Session")
    );

    let other = extra_headers(ZEN, Some(Uuid::new_v4()));
    assert_ne!(
        get(&first, "X-Opencode-Session"),
        get(&other, "X-Opencode-Session"),
        "two conversations must not share one session id"
    );
}

/// Calls made before any conversation exists (the model-catalogue fetch) still
/// have to carry a well-formed id, and stay in one bucket across the process.
#[test]
fn a_sessionless_call_still_sends_a_stable_well_formed_id() {
    let first = extra_headers(ZEN, None);
    let second = extra_headers(ZEN, None);

    let id = get(&first, "X-Opencode-Session").expect("session header must always be present");
    assert!(Uuid::parse_str(id).is_ok(), "id must be well formed: {id}");
    assert_eq!(id, get(&second, "X-Opencode-Session").unwrap());
}

/// OpenCrabs names itself the same way here as it already does on OpenRouter.
#[test]
fn opencode_requests_identify_opencrabs() {
    let h = extra_headers(ZEN, Some(Uuid::new_v4()));
    assert_eq!(get(&h, "X-Title"), Some("OpenCrabs"));
    assert_eq!(get(&h, "HTTP-Referer"), Some("https://opencrabs.com"));
}

/// A gateway fingerprints the header set it receives, so none of this may
/// leak onto another provider.
#[test]
fn other_providers_get_no_opencode_headers() {
    assert!(extra_headers("https://openrouter.ai/api/v1", Some(Uuid::new_v4())).is_empty());
    assert!(extra_headers("https://api.z.ai/api/coding/paas/v4", None).is_empty());
}

/// Every emitted name and value must survive reqwest's header parsing, or the
/// request would silently go out without the session the gateway requires.
#[test]
fn emitted_headers_are_valid_on_the_wire() {
    for (k, v) in extra_headers(ZEN, Some(Uuid::new_v4())) {
        assert!(
            k.parse::<reqwest::header::HeaderName>().is_ok(),
            "invalid header name: {k}"
        );
        assert!(
            v.parse::<reqwest::header::HeaderValue>().is_ok(),
            "invalid header value for {k}: {v}"
        );
    }
}
