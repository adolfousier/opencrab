//! Tests for the `Known paths` runtime-info section that teaches the
//! agent where logs / config / brain files / plans live on disk.
//!
//! Without this section the agent grepped the repo working directory
//! when the user said "check logs", instead of reading
//! `~/.opencrabs/logs/opencrabs.YYYY-MM-DD`. These tests are
//! sentinels: they fail loudly if a future refactor accidentally
//! strips the path guidance, sending the agent back to wandering.

use crate::brain::prompt_builder::push_known_paths;

fn rendered() -> String {
    let mut s = String::new();
    push_known_paths(&mut s);
    s
}

#[test]
fn paths_section_mentions_log_directory() {
    let out = rendered();
    assert!(
        out.contains("~/.opencrabs/logs/"),
        "must surface the log directory path; got: {out}"
    );
}

#[test]
fn paths_section_mentions_daily_log_file_pattern() {
    let out = rendered();
    assert!(
        out.contains("opencrabs.YYYY-MM-DD"),
        "must surface the daily file naming pattern so the agent \
         knows logs are rotated by date; got: {out}"
    );
}

#[test]
fn paths_section_warns_against_repo_grepping() {
    // The whole reason this section exists: stop the agent from
    // walking the working-directory tree looking for log files.
    let out = rendered();
    let lower = out.to_lowercase();
    assert!(
        lower.contains("do not grep") || lower.contains("never write"),
        "must explicitly tell the agent NOT to look in the repo working dir for logs; got: {out}"
    );
}

#[test]
fn paths_section_mentions_config_file() {
    let out = rendered();
    assert!(
        out.contains("config.toml"),
        "must surface the config path; got: {out}"
    );
}

#[test]
fn paths_section_mentions_keys_file() {
    let out = rendered();
    assert!(
        out.contains("keys.toml"),
        "must surface the API-keys path; got: {out}"
    );
}

#[test]
fn paths_section_mentions_brain_files() {
    let out = rendered();
    // Don't assert on every individual file name — that would be
    // brittle if SOUL/USER/AGENTS/TOOLS/MEMORY/CODE ever changes.
    // Assert the canonical location is given.
    assert!(
        out.contains("Brain files"),
        "must surface the brain-files location; got: {out}"
    );
}

#[test]
fn paths_section_mentions_plan_files() {
    let out = rendered();
    assert!(
        out.contains("Plans:") && out.contains("opencrabs_plan_"),
        "must surface where the plan tool persists JSON state; got: {out}"
    );
}

#[test]
fn paths_section_lists_channel_attachments() {
    // #681: the channel-attachments store is a real, profile-resolved path the
    // agent must read from — it belongs in Known paths (profile-anchored on
    // {home}) instead of being hardcoded to the default ~/.opencrabs/ root in
    // the CHANNEL ATTACHMENTS directive.
    let out = rendered();
    assert!(
        out.contains("Channel attachments") && out.contains("channel_attachments/<platform>/"),
        "must surface the profile-resolved channel_attachments path; got: {out}"
    );
}

#[test]
fn paths_section_is_short() {
    // Sentinel: keep this section tight. The whole point is the
    // agent reads it once and remembers — bloating it with every
    // path under the sun defeats the purpose.
    let out = rendered();
    let lines = out.lines().count();
    assert!(
        lines <= 15,
        "Known paths section should stay compact; got {lines} lines"
    );
}
