//! Pins secret redaction in the one-line tool-call summary.
//!
//! Regression (2026-06-07): when the agent ran a curl to test an endpoint,
//! the collapsed `bash:` tool line in the TUI showed the full command
//! including `Authorization: Bearer dgr_live_…` — the API key was exposed
//! on screen. The expanded view already redacted via redact_tool_input, but
//! the one-line summary (format_tool_description for the TUI, and the
//! mirror format_tool_summary for DB/channels) used the raw command.
//!
//! Both summaries now run the result through redact_command. The agent
//! still executes the real command; only the displayed/persisted summary
//! is redacted.

use crate::tui::app::App;
use crate::utils::sanitize::redact_command;
use serde_json::json;

#[test]
fn redact_command_hides_bearer_token() {
    // The exact shape from the leak screenshot.
    let cmd = r#"curl -s --max-time 15 https://dialagram.me/router/v1/chat/completions -H "Content-Type: application/json" -H "Authorization: Bearer dgr_live_txDsdsDDv7zpSECRETKEY" -d '{"model":"x"}'"#;
    let out = redact_command(cmd);
    assert!(
        !out.contains("dgr_live_txDsdsDDv7zpSECRETKEY"),
        "the bearer token must be redacted, got: {out}"
    );
    assert!(
        out.contains("[REDACTED]"),
        "should mark the redaction: {out}"
    );
    // Non-secret parts of the command stay visible for context.
    assert!(out.contains("curl"));
    assert!(out.contains("dialagram.me"));
}

#[test]
fn redact_command_hides_api_key_param_and_url_password() {
    assert!(
        !redact_command("curl https://api.example.com?api_key=sk-abc123def")
            .contains("sk-abc123def")
    );
    assert!(
        !redact_command("curl https://user:hunter2@host/path").contains("hunter2"),
        "URL-embedded password must be redacted"
    );
}

#[test]
fn redact_command_leaves_innocuous_commands_intact() {
    // No secret patterns → unchanged, so normal commands stay readable.
    let cmd = "grep -A2 'dialagram' ~/.opencrabs/keys.toml 2>/dev/null | head";
    assert_eq!(redact_command(cmd), cmd);
    let cmd2 = "cargo test --all-features";
    assert_eq!(redact_command(cmd2), cmd2);
}

#[test]
fn tui_bash_summary_redacts_bearer_token() {
    // The TUI one-line summary path. Must not leak the key.
    let input = json!({
        "command": r#"curl -H "Authorization: Bearer dgr_live_SUPERSECRET" https://x.com"#
    });
    let desc = App::format_tool_description("bash", &input);
    assert!(
        !desc.contains("dgr_live_SUPERSECRET"),
        "TUI bash summary leaked the token: {desc}"
    );
    assert!(desc.starts_with("bash: "), "shape preserved: {desc}");
    assert!(desc.contains("[REDACTED]"));
}

#[test]
fn tui_http_request_summary_redacts_url_password() {
    // http_request shows `METHOD url`; a credentialed URL must be redacted.
    let input = json!({
        "method": "GET",
        "url": "https://admin:topsecret@internal.example.com/health"
    });
    let desc = App::format_tool_description("http_request", &input);
    assert!(
        !desc.contains("topsecret"),
        "http_request summary leaked the URL password: {desc}"
    );
}

#[test]
fn tui_normal_bash_summary_unchanged() {
    // A non-secret command must render identically (no over-redaction).
    let input = json!({ "command": "ls -la ~/srv" });
    let desc = App::format_tool_description("bash", &input);
    assert_eq!(desc, "bash: ls -la ~/srv");
}
