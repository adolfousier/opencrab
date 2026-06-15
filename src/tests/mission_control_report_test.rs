//! The `mission_control_report` tool renders analytics + activity + inbox +
//! schedule as Markdown the agent sends to a chat, so every section must be
//! present and the output must stay em-dash-free (it is published into channels).

use crate::brain::mission_control::types::{
    McActivity, McActivityLevel, McAnalytics, McBrainFile, McInboxItem, McInboxKind,
    McScheduleItem, McScheduleKind, McToolStat,
};
use crate::brain::tools::mission_control_report::render_markdown;
use chrono::Utc;

fn sample_analytics() -> McAnalytics {
    McAnalytics {
        tool_total_calls: 100,
        tool_total_fails: 7,
        top_tools: vec![McToolStat {
            name: "bash".into(),
            total: 60,
            failures: 5,
            fail_rate: 8.3,
        }],
        flakiest_tools: vec![McToolStat {
            name: "web_fetch".into(),
            total: 9,
            failures: 2,
            fail_rate: 22.2,
        }],
        rsi_applied_total: 12,
        rsi_top_dimensions: vec![("tool_loop".into(), 8), ("provider".into(), 4)],
        brain_files: vec![McBrainFile {
            name: "MEMORY.md".into(),
            kb: 120.3,
        }],
        brain_total_kb: 312.4,
    }
}

#[test]
fn report_includes_all_sections() {
    let analytics = sample_analytics();
    let activity = vec![McActivity {
        timestamp: Utc::now(),
        detail: "Add conciseness guideline -> SOUL.md".into(),
        level: McActivityLevel::Success,
        source: "rsi".into(),
    }];
    let inbox = vec![McInboxItem {
        id: "prop_tool_123".into(),
        label: "deploy_staging".into(),
        summary: "shell: deploy.sh staging".into(),
        kind: McInboxKind::ProposedTool,
        source: "rsi-autonomous".into(),
        created_at: Utc::now(),
        detail: None,
    }];
    let schedule = vec![McScheduleItem {
        id: "1".into(),
        label: "backup_job".into(),
        schedule: "0 9 * * * (UTC)".into(),
        kind: McScheduleKind::Cron,
        awaiting_user: false,
    }];

    let md = render_markdown(&analytics, &activity, &inbox, &schedule);

    // Analytics section
    assert!(md.contains("Mission Control"));
    assert!(md.contains("Analytics"));
    assert!(md.contains("Tools: 100 calls, 7 fails (7.0%)"));
    assert!(md.contains("RSI applied: 12"));
    assert!(md.contains("Brain: 312.4 KB across 1 files"));
    assert!(md.contains("bash: 60 calls (8.3% fail)"));
    assert!(md.contains("web_fetch: 22.2% fail (9 calls)"));
    assert!(md.contains("tool_loop: 8"));
    assert!(md.contains("MEMORY.md: 120.3 KB"));

    // Inbox section
    assert!(md.contains("Inbox (RSI Proposals)"));
    assert!(md.contains("deploy_staging"));
    assert!(md.contains("shell: deploy.sh staging"));

    // Activity section
    assert!(md.contains("Activity Feed"));
    assert!(md.contains("Add conciseness guideline"));

    // Schedule section
    assert!(md.contains("Schedule"));
    assert!(md.contains("backup_job"));
    assert!(md.contains("0 9 * * *"));

    // No em dashes in channel output
    assert!(
        !md.contains('\u{2014}'),
        "channel output must have no em dashes"
    );
}

#[test]
fn empty_sections_are_omitted() {
    let md = render_markdown(&McAnalytics::default(), &[], &[], &[]);
    assert!(md.contains("Mission Control"));
    assert!(md.contains("Tools: 0 calls, 0 fails (0.0%)"));
    // Empty sections should not appear
    assert!(!md.contains("Inbox"));
    assert!(!md.contains("Activity Feed"));
    assert!(!md.contains("Schedule"));
}
