//! RSI TUI notifications must not leak secrets.
//!
//! Regression (2026-06-07): RSI alerts (AgentCycleFailed / TemplateSyncFailed
//! / ImprovementOpportunity / *Complete) surface free-text error and summary
//! strings sourced from provider errors, feedback records, and tool output —
//! any of which can contain an API key, Bearer token, or credentialed URL.
//! They were rendered on the TUI verbatim, exposing keys on screen.
//!
//! `format_rsi_notification` now redacts at the single formatting point, so
//! every variant and every caller is covered.

use crate::brain::rsi::{RsiNotification, format_rsi_notification};

#[test]
fn agent_cycle_failed_redacts_bearer_token() {
    // A provider error bubbled up through an RSI agent cycle.
    let n = RsiNotification::AgentCycleFailed {
        error: r#"provider call failed: 401 from curl -H "Authorization: Bearer dgr_live_txDsdsDDv7zpSECRET" https://x.com"#
            .to_string(),
    };
    let out = format_rsi_notification(&n);
    assert!(
        !out.contains("dgr_live_txDsdsDDv7zpSECRET"),
        "RSI cycle-failed alert leaked the bearer token: {out}"
    );
    assert!(out.starts_with("RSI: agent cycle failed —"));
}

#[test]
fn template_sync_failed_redacts_api_key_param() {
    let n = RsiNotification::TemplateSyncFailed {
        error: "fetch failed: https://api.example.com/sync?api_key=sk-abc123def456ghi789"
            .to_string(),
    };
    let out = format_rsi_notification(&n);
    assert!(
        !out.contains("sk-abc123def456ghi789"),
        "RSI template-sync-failed alert leaked the api key: {out}"
    );
}

#[test]
fn improvement_opportunity_redacts_inline_token() {
    // A long opaque token embedded in a free-text description.
    let n = RsiNotification::ImprovementOpportunity {
        description: "saw repeated auth with token ghp_AbCdEf0123456789AbCdEf0123456789AbCd"
            .to_string(),
    };
    let out = format_rsi_notification(&n);
    assert!(
        !out.contains("ghp_AbCdEf0123456789AbCdEf0123456789AbCd"),
        "RSI improvement-opportunity alert leaked the token: {out}"
    );
}

#[test]
fn agent_cycle_complete_redacts_secret_in_summary() {
    let n = RsiNotification::AgentCycleComplete {
        summary: "applied fix; verified with Authorization: Bearer sk-live-9f8e7d6c5b4a3210"
            .to_string(),
    };
    let out = format_rsi_notification(&n);
    assert!(
        !out.contains("sk-live-9f8e7d6c5b4a3210"),
        "RSI cycle-complete alert leaked the bearer token: {out}"
    );
}

#[test]
fn benign_notifications_render_unchanged() {
    // Variants with no free-text secrets must be unaffected.
    assert_eq!(
        format_rsi_notification(&RsiNotification::CycleStarted),
        "RSI: analyzing feedback patterns..."
    );
    assert_eq!(
        format_rsi_notification(&RsiNotification::DigestWritten { total_events: 42 }),
        "RSI: digest written (42 events)"
    );
    // A normal summary with no secret stays intact.
    assert_eq!(
        format_rsi_notification(&RsiNotification::TemplateSyncComplete {
            summary: "3 files updated".to_string()
        }),
        "RSI: template sync complete — 3 files updated"
    );
}
