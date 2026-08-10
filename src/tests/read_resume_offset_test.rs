//! Truncation warnings must carry the exact resume offset. Without it the
//! model reconstructs where the window ended by counting output lines or
//! guessing: wrong guesses re-read the same window (paying twice) or skip
//! content (silently wrong edits). The value is already known at the cut
//! point (#988, from the Command Code read_file audit).

use crate::brain::tools::read::ReadTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::io::Write;
use uuid::Uuid;

#[tokio::test]
async fn truncation_warning_names_the_exact_resume_offset() {
    // 100,005 lines x ~110 bytes = ~11MB: above LARGE_FILE_THRESHOLD so the
    // buffered path runs, and above MAX_LINES (100,000) so it truncates.
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
    let total = 100_005usize;
    for i in 0..total {
        writeln!(f, "line {i:0>100}").unwrap();
    }
    f.flush().unwrap();

    let tool = ReadTool;
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            serde_json::json!({ "path": f.path().to_str().unwrap() }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.success);
    let warning = result
        .metadata
        .get("warning")
        .expect("a truncated read must carry a warning");
    assert!(
        warning.contains("Resume with start_line=100000 (0-indexed)"),
        "warning must name the exact resume offset: {warning}"
    );
    assert!(
        warning.contains("100005 total lines"),
        "warning must report the true total: {warning}"
    );
}
