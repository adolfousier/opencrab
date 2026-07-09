//! Progress and approval callbacks for the headless CLI (`run`, `agent`).
//!
//! The non-interactive CLI paths run the same tool loop as every channel
//! (#492). These small helpers give the loop a stdout progress reporter and,
//! for the interactive REPL, a stdin approval prompt, so headless runs both
//! execute tools and show what they are doing.

use std::sync::Arc;

use crate::brain::agent::{ApprovalCallback, ProgressCallback, ProgressEvent, ToolApprovalInfo};

/// Print tool activity and mid-turn narration to stdout while the tool loop
/// runs. Keeps a headless `run`/`agent` transparent instead of silent until
/// the final answer.
pub(crate) fn cli_progress_callback() -> ProgressCallback {
    Arc::new(|_session_id, event| match event {
        ProgressEvent::ToolStarted { tool_name, .. } => {
            println!("🔧 {tool_name}");
        }
        ProgressEvent::ToolCompleted {
            tool_name,
            success,
            summary,
            ..
        } => {
            let icon = if success { "✅" } else { "❌" };
            println!("{icon} {tool_name}: {summary}");
        }
        ProgressEvent::IntermediateText { text, .. } => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                println!("{trimmed}");
            }
        }
        _ => {}
    })
}

/// Map a stdin approval answer to `(approved, always)`. `y`/`yes` approve
/// once, `a`/`always` approve the rest of this loop, anything else denies.
/// Case- and whitespace-insensitive. Kept pure for testing.
pub(crate) fn approval_from_answer(answer: &str) -> (bool, bool) {
    let a = answer.trim().to_lowercase();
    let always = matches!(a.as_str(), "a" | "always");
    let approved = always || matches!(a.as_str(), "y" | "yes");
    (approved, always)
}

/// Prompt on stdin for each approval-gated tool. Only for the interactive
/// REPL, which already owns stdin; single-shot `run` passes `--auto-approve`
/// instead (there is no TTY to prompt on when backgrounded).
pub(crate) fn stdin_approval_callback() -> ApprovalCallback {
    Arc::new(|info: ToolApprovalInfo| {
        Box::pin(async move {
            let prompt = format!(
                "\n⚠️  Tool '{}' wants to run (capabilities: {}). Approve? [y/N/a=always] ",
                info.tool_name,
                if info.capabilities.is_empty() {
                    "none".to_string()
                } else {
                    info.capabilities.join(", ")
                }
            );
            let answer = tokio::task::spawn_blocking(move || {
                use std::io::{self, Write};
                print!("{prompt}");
                let _ = io::stdout().flush();
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                line
            })
            .await
            .unwrap_or_default();

            Ok(approval_from_answer(&answer))
        })
    })
}
