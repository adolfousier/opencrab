//! One-shot proactive fallback-chain setup suggestion (#1008).
//!
//! Most recoverable provider failures (rate limits, overloads, HTTP 400
//! parameter mismatches) only self-heal when a fallback chain exists, but
//! users never discover `[providers.fallback]` on their own: the template
//! ships disabled and nothing at runtime mentions it. So the first REAL
//! user turn after startup carries a system note instructing the agent to
//! propose the guided setup when no chain is configured. A marker file in
//! the profile home keeps it strictly one-shot — never a nag.

use std::path::{Path, PathBuf};

const MARKER_FILE: &str = ".fallback-chain-suggested";

/// Note prepended to the first real user turn when no fallback chain is
/// configured. Instructs the agent to run the guided flow from #1008:
/// name, base URL, key, live `/v1/models` fetch, model pick, config +
/// keys write, chain update, restart.
pub(crate) const SUGGESTION_NOTE: &str = "[System: FALLBACK CHAIN SETUP — \
this OpenCrabs instance appears to have providers configured but NO \
fallback chain ([providers.fallback] in config.toml is missing, disabled, \
or empty). Verify that against the config first; if it really is missing, \
proactively suggest fixing it: at the end of your reply, in one or two \
friendly sentences, tell the user there is no failover configured so any \
provider failure (rate limit, outage, invalid request) drops their \
requests, and offer the tappable suggestion \"Yes let's setup a fallback \
provider now\". If the user accepts, guide them ONE step per message: \
1) ask a custom provider name, 2) base URL, 3) API key, 4) fetch the live \
model list via HTTP GET {base_url}/v1/models with header \
Authorization: Bearer <key> and let them pick a default model, \
5) write [providers.custom.<name>] (base_url, model) into config.toml and \
the key into keys.toml, 6) add the provider name to [providers.fallback] \
with enabled = true and providers = [...], 7) verify the config parses \
and tell them /restart picks it up. Never ask for all fields at once, and \
do not repeat this suggestion on later turns.]";

pub(crate) fn marker_path(home: &Path) -> PathBuf {
    home.join(MARKER_FILE)
}

/// True when no chain is configured and this install has never been shown
/// the suggestion. Pure decision so it is unit-testable (#1008).
pub(crate) fn should_suggest(home: &Path, chain_configured: bool) -> bool {
    !chain_configured && !marker_path(home).exists()
}

pub(crate) fn mark_suggested(home: &Path) {
    let _ = std::fs::write(marker_path(home), "suggested\n");
}

/// Prepend the one-shot suggestion note to a genuine user message.
/// `[System:` messages (resumes, background-task results) never trigger
/// it: the suggestion rides a real user turn so it lands on whichever
/// channel the user is actually talking on.
pub(crate) fn maybe_inject(home: &Path, chain_configured: bool, user_message: String) -> String {
    if user_message.starts_with("[System:") || !should_suggest(home, chain_configured) {
        return user_message;
    }
    mark_suggested(home);
    format!("{SUGGESTION_NOTE}\n\n{user_message}")
}
