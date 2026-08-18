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

/// Where a session's background-task completion must be delivered, keyed by
/// session rather than by whichever surface happened to run the command.
///
/// Every surface builds its own `BackgroundTaskManager` from its own enqueue
/// callback, so the completion used to follow the *executing* service. A
/// channel-bound session driven from the TUI therefore reported back to the
/// TUI, and the channel that started the work never heard the answer (#940).
/// A channel registers its session here when it binds one; the manager
/// consults this first and only falls back to its own callback when nothing
/// claims the session (a genuinely TUI-local or CLI-local session).
static SESSION_ROUTES: Mutex<Option<HashMap<Uuid, MessageEnqueueCallback>>> = Mutex::new(None);

/// Bind `session_id`'s background-task completions to `enqueue`.
///
/// Idempotent: re-binding the same session replaces the route, which is what
/// a reconnect or a bot restart needs.
pub fn register_session_route(session_id: Uuid, enqueue: MessageEnqueueCallback) {
    match SESSION_ROUTES.lock() {
        Ok(mut guard) => {
            guard
                .get_or_insert_with(HashMap::new)
                .insert(session_id, enqueue.clone());
            // Startup recovery runs before any channel connects, so this
            // session may already have reports waiting for someone to claim
            // it. Hand them over now that there is somewhere to send them
            // (#1037). Done after the insert so the route is live first.
            super::restart_recovery::claim_session(session_id, &enqueue);
        }
        Err(e) => {
            // Worth saying out loud: without the route this session's next
            // background completion silently goes to the wrong surface.
            tracing::error!(
                target: "background_task",
                "Could not register resume route for session {session_id}: {e}"
            );
        }
    }
}

/// Who should receive `session_id`'s completion: the surface that claimed the
/// session, falling back to `executing` when nothing did.
///
/// The whole fix in one line — pick by session, never by who ran the command —
/// so it is a pure function and directly testable.
pub fn resolve_route(
    session_id: Uuid,
    executing: &MessageEnqueueCallback,
) -> MessageEnqueueCallback {
    session_route(session_id).unwrap_or_else(|| executing.clone())
}

/// The surface this process booted on, used when no channel claims a session.
///
/// `spawn_command` carries the executing service's callback on the manager, so
/// it always has a fallback. A sub-agent has no such handle — it is reached
/// from a tool with no service context — so the local surface is registered
/// once at startup and resolved on demand instead (#1036).
static LOCAL_ROUTE: Mutex<Option<MessageEnqueueCallback>> = Mutex::new(None);

/// Record the booting surface as the fallback destination. Called once per
/// process start; re-registering replaces it.
pub fn register_local_route(enqueue: MessageEnqueueCallback) {
    match LOCAL_ROUTE.lock() {
        Ok(mut guard) => *guard = Some(enqueue),
        Err(e) => {
            // Without it, a sub-agent finishing on a session no channel owns
            // has nowhere to report and its output is dropped.
            tracing::error!(
                target: "background_task",
                "Could not register the local delivery route: {e}"
            );
        }
    }
}

/// Deliver `msg` to whoever owns `session_id`, falling back to the booting
/// surface. Returns whether it went anywhere at all.
pub fn deliver_to_session(session_id: Uuid, msg: QueuedUserMessage) -> bool {
    if let Some(route) = session_route(session_id) {
        route(session_id, msg);
        return true;
    }
    let local = match LOCAL_ROUTE.lock() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read the local delivery route for session {session_id}: {e}"
            );
            None
        }
    };
    match local {
        Some(route) => {
            route(session_id, msg);
            true
        }
        None => {
            tracing::error!(
                target: "background_task",
                "Nothing can receive a message for session {session_id}; it is dropped: {}",
                msg.display_text
            );
            false
        }
    }
}

/// The surface that owns `session_id`'s completions, if one claimed it.
pub(crate) fn session_route(session_id: Uuid) -> Option<MessageEnqueueCallback> {
    match SESSION_ROUTES.lock() {
        Ok(guard) => guard.as_ref()?.get(&session_id).cloned(),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read resume route for session {session_id}: {e}"
            );
            None
        }
    }
}

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
        let task_id = Uuid::new_v4();
        tokio::spawn(async move {
            // Log the START as well as the finish. Only completions were
            // logged, so a task that never finished left no trace of having
            // begun, and reconstructing which commands got detached meant
            // inferring it from the completions that did arrive.
            tracing::info!(
                target: "background_task",
                "Background task '{label}' started for session {session_id} \
                 (id={task_id}, cwd={})",
                cwd.display()
            );
            // Persist BEFORE running: a restart mid-command must find a row to
            // report as interrupted, otherwise the session waits forever on a
            // resume that can no longer come (#763).
            if let Some(repo) = task_repo() {
                let cwd_str = cwd.to_string_lossy().to_string();
                if let Err(e) = repo
                    .record(task_id, session_id, &label, &command, &cwd_str)
                    .await
                {
                    // Not fatal: the command still runs and still resumes the
                    // session in this process. Only restart accounting is lost.
                    tracing::error!(
                        target: "background_task",
                        "Failed to persist background task '{label}': {e:#}"
                    );
                }
            }
            let result = run_detached(&command, &cwd).await;
            tracing::info!(
                target: "background_task",
                "Background task '{label}' for session {session_id} finished (success={})",
                result.success
            );
            let msg = completion_message(&label, &command, &result);
            if let Some(repo) = task_repo()
                && let Err(e) = repo.clear(task_id).await
            {
                // A stale row makes the NEXT startup report a phantom
                // interruption, so this must be visible even though the
                // command itself succeeded.
                tracing::error!(
                    target: "background_task",
                    "Failed to clear background task '{label}' after completion: {e:#}"
                );
            }
            // Clear the indicator BEFORE delivering, not after. The task is
            // over the moment the process exits, but mark_finished sat behind
            // the enqueue callback, so the "running" badge outlived the work by
            // however long delivery took — on a killed task the user saw the
            // agent confirm it had stopped while the input border still showed
            // it running.
            //
            // Only touches the in-memory map, so moving it earlier cannot
            // affect what gets delivered.
            this.mark_finished(session_id, &label);
            // Deliver to the surface that OWNS the session, not to whichever
            // one executed the command. A channel session driven from the TUI
            // runs on the TUI's service, so `this.enqueue` would answer into
            // the TUI and leave the channel that asked for the work waiting on
            // a reply that never comes (#940).
            resolve_route(session_id, &this.enqueue)(session_id, msg);
        });
    }
}

/// The background-task repository, when a pool exists.
///
/// Resolved per call through the global pool rather than threaded through the
/// manager, because `spawn_command` is reached from the bash tool which has no
/// pool in its context. `None` before the DB is initialized (early startup,
/// tests), which simply means restart accounting is skipped.
fn task_repo() -> Option<crate::db::BackgroundTaskRepository> {
    crate::db::global_pool().map(|p| crate::db::BackgroundTaskRepository::new(p.clone()))
}

/// Account for background tasks that were running when a previous process
/// died, then clear them.
///
/// Every surviving row belonged to a process that no longer exists, so its
/// child is gone too: there is nothing to reattach to and no result coming.
/// Each one is reported into its session as an interruption so the agent can
/// decide whether to re-run it, rather than waiting forever on a resume that
/// can never arrive (#763). Returns how many were reported.
pub async fn report_interrupted() -> usize {
    let Some(repo) = task_repo() else {
        return 0;
    };
    let rows = match repo.all().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(target: "background_task", "Failed to read background tasks: {e:#}");
            return 0;
        }
    };
    if rows.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for row in rows {
        tracing::warn!(
            target: "background_task",
            "Background task '{}' for session {} was interrupted by a restart",
            row.label,
            row.session_id
        );
        // By session, never by whoever booted. This path used to take the
        // caller's callback directly, so a channel session's interruption
        // landed on the local surface — the shape #940 fixed for completions
        // and left standing here. Startup runs before channels register, so
        // an unclaimed session parks rather than mis-delivers (#1037).
        super::restart_recovery::deliver_or_park(row.session_id, interrupted_message(&row));
        count += 1;

        // Clear per row, only after it is accounted for. clear_all() used to
        // run regardless, so a row whose report never got produced was
        // dropped from the table anyway and its session never heard anything.
        if let Err(e) = repo.clear(row.id).await {
            // A surviving row re-reports the same interruption next start,
            // which is noisy but recoverable; the report itself already
            // landed, so this is not fatal.
            tracing::error!(
                target: "background_task",
                "Failed to clear background task '{}' after reporting it: {e:#}",
                row.label
            );
        }
    }
    count
}

/// What the agent is told about a command a restart killed. Deliberately
/// states that it did NOT finish and hands the decision back, rather than
/// re-running something expensive on the agent's behalf.
fn interrupted_message(row: &crate::db::BackgroundTaskRow) -> QueuedUserMessage {
    let context_text = format!(
        "[BACKGROUND TASK INTERRUPTED] `{}` was still running when OpenCrabs restarted, so it \
         was killed and produced no result. The command was:\n\n```\n{}\n```\n\nIt did NOT \
         complete. Decide whether to run it again based on what you were doing; do not assume \
         it passed or failed.",
        row.label, row.command
    );
    QueuedUserMessage {
        context_text,
        display_text: format!("⚠️ Background task interrupted by restart: {}", row.label),
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

/// A short human label for a command (first meaningful token sequence), for the
/// "running in the background" acknowledgement and the completion tag.
pub(crate) fn short_label(command: &str) -> String {
    let after_cd = crate::utils::command_label::command_label(command);
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
