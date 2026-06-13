//! The `analytics_report` tool renders an `McAnalytics` snapshot as Markdown
//! the agent sends to a chat, so every section must be present and the output
//! must stay em-dash-free (it is published into channels).

use crate::brain::mission_control::{McAnalytics, McBrainFile, McToolStat};
use crate::brain::tools::analytics_report::render_markdown;

fn sample() -> McAnalytics {
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
    let md = render_markdown(&sample());
    assert!(md.contains("OpenCrabs Analytics"));
    assert!(md.contains("Tools: 100 calls, 7 fails (7.0%)"));
    assert!(md.contains("RSI applied: 12"));
    assert!(md.contains("Brain: 312.4 KB across 1 files"));
    assert!(md.contains("bash: 60 calls (8.3% fail)"));
    assert!(md.contains("web_fetch: 22.2% fail (9 calls)"));
    assert!(md.contains("tool_loop: 8"));
    assert!(md.contains("MEMORY.md: 120.3 KB"));
    assert!(!md.contains('—'), "channel output must have no em dashes");
}

#[test]
fn empty_analytics_still_renders_header() {
    let md = render_markdown(&McAnalytics::default());
    assert!(md.contains("OpenCrabs Analytics"));
    assert!(md.contains("Tools: 0 calls, 0 fails (0.0%)"));
    // No section bodies when there's no data.
    assert!(!md.contains("Top tools"));
}
