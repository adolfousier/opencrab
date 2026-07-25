//! Background task manager (#722).
//!
//! Runs a genuinely long command detached (so it doesn't churn the bash 600s
//! cap) and, on completion, enqueues a synthetic `QueuedUserMessage` into the
//! originating session via the surface enqueue callback. The tool loop drains
//! that at the next iteration boundary — injected mid-turn if the agent is still
//! working, or starting a fresh turn if it went idle.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use uuid::Uuid;

use super::types::{MessageEnqueueCallback, QueuedUserMessage};

/// Result of a finished background command.
#[derive(Debug, Clone)]
pub struct CmdResult {
    pub success: bool,
    pub code: i32,
    pub output: String,
}

/// One in-flight background command.
#[derive(Debug, Clone)]
pub struct RunningTask {
    /// Short label for the command, e.g. `cargo test`.
    pub label: String,
    /// When it was spawned, for the elapsed time a surface displays.
    pub started: std::time::Instant,
}

/// Manages background commands and resumes their sessions on completion.
pub struct BackgroundTaskManager {
    enqueue: MessageEnqueueCallback,
    /// In-flight background tasks per session.
    ///
    /// Holds the label and start time, not just a count, because a surface has
    /// to be able to say WHAT is running and for how long. A detached task
    /// takes the turn idle, so without this the TUI has nothing at all to draw
    /// while a long build runs and the wait looks like a hang (#762).
    running: Mutex<HashMap<Uuid, Vec<RunningTask>>>,
}

impl BackgroundTaskManager {
    pub fn new(enqueue: MessageEnqueueCallback) -> Self {
        Self {
            enqueue,
            running: Mutex::new(HashMap::new()),
        }
    }

    /// How many background tasks are currently running for `session_id`.
    pub fn running_for(&self, session_id: Uuid) -> usize {
        self.running
            .lock()
            .map(|m| m.get(&session_id).map(Vec::len).unwrap_or(0))
            .unwrap_or(0)
    }

    /// What is running for `session_id`, oldest first, for surfaces that show
    /// progress. Returns owned data so the caller never holds the lock.
    pub fn running_tasks(&self, session_id: Uuid) -> Vec<RunningTask> {
        self.running
            .lock()
            .map(|m| m.get(&session_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    fn mark_started(&self, session_id: Uuid, label: &str) {
        if let Ok(mut m) = self.running.lock() {
            m.entry(session_id).or_default().push(RunningTask {
                label: label.to_string(),
                started: std::time::Instant::now(),
            });
        }
    }

    fn mark_finished(&self, session_id: Uuid, label: &str) {
        if let Ok(mut m) = self.running.lock()
            && let Some(tasks) = m.get_mut(&session_id)
        {
            // Remove the OLDEST entry with this label: two `cargo test` runs are
            // indistinguishable here, and dropping the oldest keeps the elapsed
            // time shown for the survivor honest.
            if let Some(pos) = tasks.iter().position(|t| t.label == label) {
                tasks.remove(pos);
            }
            if tasks.is_empty() {
                m.remove(&session_id);
            }
        }
    }

    /// Spawn `command` (via `sh -c`) in `cwd`, detached; on completion enqueue a
    /// system message into `session_id` summarizing the result. Returns
    /// immediately — the caller's turn is free to end.
    pub fn spawn_command(
        self: std::sync::Arc<Self>,
        session_id: Uuid,
        cwd: PathBuf,
        label: String,
        command: String,
    ) {
        self.mark_started(session_id, &label);
        let this = std::sync::Arc::clone(&self);
        tokio::spawn(async move {
            let result = run_detached(&command, &cwd).await;
            tracing::info!(
                target: "background_task",
                "Background task '{label}' for session {session_id} finished (success={})",
                result.success
            );
            let msg = completion_message(&label, &command, &result);
            (this.enqueue)(session_id, msg);
            this.mark_finished(session_id, &label);
        });
    }
}

/// Run `command` through `sh -c` in `cwd`, capturing merged stdout+stderr.
async fn run_detached(command: &str, cwd: &std::path::Path) -> CmdResult {
    use tokio::process::Command;
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .output()
        .await;
    match output {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&err);
            }
            CmdResult {
                success: out.status.success(),
                code: out.status.code().unwrap_or(-1),
                output: combined,
            }
        }
        Err(e) => CmdResult {
            success: false,
            code: -1,
            output: format!("failed to launch: {e}"),
        },
    }
}

/// Command substrings that mark a genuinely long-running task (#722). When bash
/// sees one AND a background manager is wired, it runs the command detached and
/// resumes the session on completion instead of blocking toward the 600s cap.
/// Matched as a substring so `cd x && cargo test` still counts.
const KNOWN_LONG_MARKERS: &[&str] = &[
    "cargo test",
    "cargo build",
    "cargo run",
    "cargo clippy",
    "npm test",
    "npm run build",
    "pnpm test",
    "pnpm build",
    "yarn test",
    "yarn build",
    "npx remotion render",
    "remotion render",
    "gh run watch",
    "gh pr checks --watch",
];

/// Is `command` a known long-running task that should run in the background?
pub(crate) fn is_known_long(command: &str) -> bool {
    let lower = command.to_lowercase();
    KNOWN_LONG_MARKERS.iter().any(|m| lower.contains(m))
}

/// A short human label for a command (first meaningful token sequence), for the
/// "running in the background" acknowledgement and the completion tag.
pub(crate) fn short_label(command: &str) -> String {
    let trimmed = command.trim();
    let after_cd = trimmed.rsplit("&&").next().unwrap_or(trimmed).trim();
    let label: String = after_cd.chars().take(60).collect();
    if after_cd.chars().count() > 60 {
        format!("{label}…")
    } else {
        label
    }
}

/// Keep only the last `n` lines of `text`.
pub(crate) fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Build the resume message from a finished background command (#722). Pure so
/// the framing is unit-testable without spawning anything.
pub(crate) fn completion_message(
    label: &str,
    command: &str,
    result: &CmdResult,
) -> QueuedUserMessage {
    let status = if result.success {
        "exit 0 (success)".to_string()
    } else {
        format!("exit {} (failure)", result.code)
    };
    let tail = tail_lines(&result.output, 50);
    let context = format!(
        "[System: the background task you started has finished.\n\
         Task: {label}\n\
         Command: {command}\n\
         Status: {status}\n\
         Output (last 50 lines):\n{tail}\n\n\
         Report the result to the user and continue anything that was waiting on it. \
         Do not re-run the command — this IS its result.]"
    );
    let display = format!(
        "🔧 background task {}: {label}",
        if result.success { "finished" } else { "failed" }
    );
    QueuedUserMessage::system(context, display)
}
