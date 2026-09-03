//! Tests for plan-tool summary rendering in `markdown_to_telegram_html`.
//!
//! The `plan` tool's `summary` op emits markdown that triggers rich
//! detection (heading + task list items). When the rich path fails
//! or is unavailable, `markdown_to_telegram_html` handles the fallback.
//!
//! New format:
//!
//!     ## Plan: My Plan
//!
//!     **Status:** InProgress
//!     **Description:** refactor X
//!
//!     **Tasks** (3 total):
//!     - [x] First task
//!     - [>] Second task
//!     - [ ] Third task
//!
//!     **Progress:** 33.3%  ✅1 ❌0 ▶️1 ⏸️1 ⏭️0 🚫0
//!     **Success Rate:** 100.0% | Retries: 0 | Tool Calls: 0

use crate::channels::telegram::handler::markdown_to_telegram_html;

const PLAN_BLOCK: &str = "## Plan: My Plan\n\n\
                          **Status:** InProgress\n\
                          **Description:** refactor X\n\n\
                          **Tasks** (3 total):\n\
                          - [x] First task\n\
                          - [>] Second task\n\
                          - [ ] Third task\n\n\
                          **Progress:** 33.3%  ✅1 ❌0 ▶️1 ⏸️1 ⏭️0 🚫0\n\
                          **Success Rate:** 100.0% | Retries: 0 | Tool Calls: 0";

#[test]
fn plan_block_heading_renders_as_html_heading() {
    let html = markdown_to_telegram_html(PLAN_BLOCK);
    assert!(
        html.contains("<b>Plan: My Plan</b>"),
        "plan heading must render as bold heading; got: {html}"
    );
}

#[test]
fn plan_block_task_list_items_render() {
    let html = markdown_to_telegram_html(PLAN_BLOCK);
    // markdown_to_telegram_html converts [x] → ☑, [ ] → ☐
    assert!(
        html.contains("☑ First task") || html.contains("[x] First task"),
        "completed task must render; got: {html}"
    );
    assert!(
        html.contains("Second task"),
        "in-progress task must render; got: {html}"
    );
    assert!(
        html.contains("☐ Third task") || html.contains("[ ] Third task"),
        "pending task must render; got: {html}"
    );
}

#[test]
fn plan_block_bold_labels_render() {
    let html = markdown_to_telegram_html(PLAN_BLOCK);
    assert!(
        html.contains("<b>Status:</b>"),
        "Status label must be bold; got: {html}"
    );
    assert!(
        html.contains("<b>Progress:</b>"),
        "Progress label must be bold; got: {html}"
    );
    assert!(
        html.contains("<b>Success Rate:</b>"),
        "Success Rate label must be bold; got: {html}"
    );
}

#[test]
fn plan_block_progress_line_intact() {
    let html = markdown_to_telegram_html(PLAN_BLOCK);
    assert!(
        html.contains("✅1 ❌0 ▶️1 ⏸️1 ⏭️0 🚫0"),
        "progress stats must survive; got: {html}"
    );
}

#[test]
fn text_before_and_after_plan_block_processes_normally() {
    let mixed = format!(
        "I've made progress on the refactor. Here's where we are:\n\n\
         {PLAN_BLOCK}\n\n\
         Next step: implement the second task."
    );
    let html = markdown_to_telegram_html(&mixed);
    assert!(
        html.contains("I&#x27;ve made progress on the refactor")
            || html.contains("I've made progress on the refactor"),
        "leading prose must render normally; got: {html}"
    );
    assert!(
        html.contains("Next step: implement the second task"),
        "trailing prose must render normally; got: {html}"
    );
}

#[test]
fn html_chars_inside_plan_are_escaped() {
    let plan = "## Plan: Fix <legacy>\n\n\
                **Tasks** (1 total):\n\
                - [x] Fix <legacy> handler\n\n\
                **Progress:** 100%  ✅1 ❌0 ▶️0 ⏸️0 ⏭️0 🚫0\n\
                **Success Rate:** 100.0% | Retries: 0 | Tool Calls: 0";
    let html = markdown_to_telegram_html(plan);
    assert!(
        html.contains("&lt;legacy&gt;"),
        "angle brackets must be escaped; got: {html}"
    );
    assert!(
        !html.contains("<legacy>"),
        "raw angle brackets must NOT leak; got: {html}"
    );
}

#[test]
fn message_without_plan_block_is_unchanged() {
    let plain = "Just a regular reply with **bold** and `code`.";
    let html = markdown_to_telegram_html(plain);
    assert!(
        !html.contains("<b>Plan:"),
        "non-plan messages must not have plan heading; got: {html}"
    );
}

#[test]
fn plan_block_rich_structure_detected() {
    // The new format must trigger has_rich_structure so it goes
    // through try_send_intermediate_rich instead of HTML fallback.
    assert!(
        crate::channels::telegram::rich::detect::has_rich_structure(PLAN_BLOCK),
        "plan block must trigger rich detection"
    );
}

#[test]
fn old_emoji_format_does_not_trigger_rich() {
    // Verify the OLD format didn't trigger rich (the bug we're fixing).
    let old_format = "📊 Plan Summary\n\n\
                      Plan: My Plan\n\
                      Status: InProgress\n\n\
                      Tasks (3 total):\n  \
                      ✅ 1. First task\n  \
                      ▶️ 2. Second task\n  \
                      ⏸️ 3. Third task";
    assert!(
        !crate::channels::telegram::rich::detect::has_rich_structure(old_format),
        "old emoji format must NOT trigger rich detection (confirms the bug)"
    );
}
