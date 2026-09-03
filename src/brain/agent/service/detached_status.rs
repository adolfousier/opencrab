//! Detached-command progress via JSON status files (#1160).
//!
#![allow(dead_code)]
// upstream rework (#1313-#1319) removed the lib callers
// (run_detached reworked); kept intact for the forkwin port cycle to
// disposition (round-4/5 precedent: tag, don't delete).
//! Mirrors the sub-agent status-file pattern
//! (`brain/tools/subagent/status.rs`): each detached bash command writes
//! `<opencrabs_home>/tmp/detached/<task_id>.json` at spawn and rewrites it on
//! completion, so the model has something readable between launch and the
//! completion inject. `run_detached` buffers output instead of streaming it,
//! so there is honestly no MID-run output size: the file carries spawn
//! metadata while running and gains exit status + output size at the end.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Base directory for all detached-task status files.
pub fn status_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(p) = test_override::get() {
            return p;
        }
    }
    crate::config::opencrabs_home().join("tmp").join("detached")
}

#[cfg(test)]
pub(crate) mod test_override {
    use std::cell::RefCell;
    use std::path::PathBuf;
    thread_local! {
        static DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }
    pub fn set(p: PathBuf) {
        DIR.with(|d| *d.borrow_mut() = Some(p));
    }
    pub fn get() -> Option<PathBuf> {
        DIR.with(|d| d.borrow().clone())
    }
}

/// On-disk status of one detached command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedTaskStatus {
    pub task_id: String,
    pub session_id: String,
    pub label: String,
    pub command: String,
    /// Unix seconds at spawn.
    pub spawned_at: u64,
    /// Present once the process exited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<DetachedFinish>,
}

/// Exit information, written when the process is gone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedFinish {
    pub success: bool,
    pub code: i32,
    pub elapsed_secs: f32,
    pub output_bytes: usize,
}

fn path_for(task_id: Uuid) -> PathBuf {
    status_dir().join(format!("{task_id}.json"))
}

/// Persist the spawn record. Failures are logged, never fatal: the command
/// itself still runs and still resumes the session (same contract as the
/// interrupted-run repo writes in `spawn_command`).
pub fn write_started(task_id: Uuid, session_id: Uuid, label: &str, command: &str) {
    let status = DetachedTaskStatus {
        task_id: task_id.to_string(),
        session_id: session_id.to_string(),
        label: label.to_string(),
        command: command.to_string(),
        spawned_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        finished: None,
    };
    write_status(task_id, &status);
}

/// Rewrite the record with exit information. Falls back to a minimal record
/// if the spawn write never landed.
pub fn write_finished(
    task_id: Uuid,
    session_id: Uuid,
    label: &str,
    command: &str,
    finish: DetachedFinish,
) {
    let mut status = match fs::read_to_string(path_for(task_id))
        .ok()
        .and_then(|raw| serde_json::from_str::<DetachedTaskStatus>(&raw).ok())
    {
        Some(existing) => existing,
        None => DetachedTaskStatus {
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
            spawned_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            finished: None,
        },
    };
    let _ = (session_id, label, command); // identity comes from the existing record
    status.finished = Some(finish);
    write_status(task_id, &status);
}

fn write_status(task_id: Uuid, status: &DetachedTaskStatus) {
    let dir = status_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!(
            target: "background_task",
            "Could not create detached-status dir {}: {e}",
            dir.display()
        );
        return;
    }
    match serde_json::to_string_pretty(status) {
        Ok(body) => {
            if let Err(e) = fs::write(path_for(task_id), body) {
                tracing::warn!(
                    target: "background_task",
                    "Could not write detached status for {task_id}: {e}"
                );
            }
        }
        Err(e) => tracing::warn!(
            target: "background_task",
            "Could not serialize detached status for {task_id}: {e}"
        ),
    }
}
