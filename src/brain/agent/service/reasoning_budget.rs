//! Per-turn reasoning token budget (#970).
//!
//! A turn was observed spending 900+ seconds and 30k+ tokens producing nothing
//! but reasoning. It ended with the "reasoned without answering" nudge, and the
//! nudge fixed it on the FIRST attempt. So the nudge is the cure; the defect is
//! that we wait out an unbounded reasoning stream before applying it.
//!
//! `agent.thinking_loop_timeout_secs` (#890) already existed at a 600s default
//! and did not save it, for three reasons:
//!
//! 1. 600s is far above normal. Healthy reasoning sits well below it; only one
//!    model family runs away.
//! 2. It latches off for the rest of the stream after a single tool call
//!    (`helpers.rs`, `if !has_tool_call`), so reason-then-call-then-reason is
//!    unguarded.
//! 3. It is armed per stream, so a turn can burn far past the ceiling in total
//!    while no single iteration ever reaches it.
//!
//! Point 3 is why this budget is scoped to the TURN: a per-stream cap inherits
//! exactly the same hole. Tokens rather than seconds because tokens describe
//! the runaway itself, are what providers report, and do not punish a slow
//! provider doing correct work.
//!
//! # Why a session-keyed registry and not a task-local
//!
//! A task-local requires wrapping the turn's future in a scope future.
//! `run_tool_loop_inner` is an enormous async state machine, and nesting it one
//! frame deeper overflowed the 2 MiB tokio worker stack during tests. Keying on
//! the session id that `stream_complete` already receives keeps the future
//! shape untouched, which is the same reason the background-task routing table
//! is a static map rather than scoped state.

use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Reasoning tokens still available, per in-flight turn.
///
/// Absent means no budget is armed, which is the correct reading for streams
/// that run outside a tool loop (compaction, title generation).
static REMAINING: Mutex<Option<HashMap<Uuid, usize>>> = Mutex::new(None);

/// Arms the budget for `session_id` and disarms it on drop, so an early
/// return, a cancel, or a panic cannot leave the entry behind to throttle the
/// session's next turn.
pub struct BudgetGuard(Option<Uuid>);

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0
            && let Ok(mut map) = REMAINING.lock()
            && let Some(map) = map.as_mut()
        {
            map.remove(&id);
        }
    }
}

/// Arm a reasoning budget for one turn.
///
/// A `budget` of 0 disables enforcement: nothing is recorded, so [`charge`]
/// reports headroom forever.
pub fn arm(session_id: Uuid, budget: usize) -> BudgetGuard {
    if budget == 0 {
        return BudgetGuard(None);
    }
    if let Ok(mut map) = REMAINING.lock() {
        map.get_or_insert_with(HashMap::new)
            .insert(session_id, budget);
        return BudgetGuard(Some(session_id));
    }
    BudgetGuard(None)
}

/// Spend `tokens` of this session's turn budget.
///
/// Returns `true` when the budget is exhausted and the stream should stop.
/// With no budget armed, always returns `false`.
pub fn charge(session_id: Uuid, tokens: usize) -> bool {
    let Ok(mut guard) = REMAINING.lock() else {
        return false;
    };
    let Some(map) = guard.as_mut() else {
        return false;
    };
    let Some(left) = map.get_mut(&session_id) else {
        return false;
    };
    if *left == 0 {
        return true;
    }
    *left = left.saturating_sub(tokens);
    *left == 0
}

/// Tokens left for this session's turn, or `None` when no budget is armed.
pub fn remaining(session_id: Uuid) -> Option<usize> {
    REMAINING.lock().ok()?.as_ref()?.get(&session_id).copied()
}
