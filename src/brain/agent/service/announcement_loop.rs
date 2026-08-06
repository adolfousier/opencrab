//! Cross-turn announcement loop guard (#957) — the text layer.
//!
//! Second half of the Luna fix: a per-session ring buffer of outgoing
//! assistant texts. The tool layer catches the near-identical `bash`
//! echoes; this layer catches the 18+ reworded announcements that each
//! land as a separate, internally clean turn, so every within-turn guard
//! (#507, phantom intent-loop, #740, #788) misses them.
//!
//! Detect and surface, don't prevent (the #954 guard philosophy): the
//! first trip still delivers the text plus a system nudge; only a second
//! trip with the nudge ignored escalates to an abort through the existing
//! repetition -> loop-message path (`format_user_error`).

use std::collections::{HashSet, VecDeque};

use super::helpers::normalize_loop_text;

/// Outgoing texts remembered per session (#957).
pub const TEXT_LOOP_RING_CAP: usize = 5;
/// Near-duplicate hits within the ring that trip the guard (#957).
pub const TEXT_LOOP_TRIP_AT: usize = 3;
/// Jaccard similarity threshold on normalized word sets (#957).
const NEAR_DUPLICATE_JACCARD: f64 = 0.8;
/// Below this many normalized words, word-set Jaccard is too coarse —
/// only exact normalized equality counts.
const NEAR_DUPLICATE_MIN_WORDS: usize = 3;

/// Near-duplicate check for loop detection (#957).
///
/// Jaccard similarity >= 0.8 on normalized word sets, with an equality
/// fallback: short texts (< 3 normalized words) only count as duplicates
/// when their normalized forms are identical, because word-set Jaccard is
/// too coarse below a handful of words.
pub fn near_duplicate(a: &str, b: &str) -> bool {
    let na = normalize_loop_text(a);
    let nb = normalize_loop_text(b);
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na == nb {
        return true;
    }
    let wa: HashSet<&str> = na.split(' ').collect();
    let wb: HashSet<&str> = nb.split(' ').collect();
    if wa.len() < NEAR_DUPLICATE_MIN_WORDS || wb.len() < NEAR_DUPLICATE_MIN_WORDS {
        return false;
    }
    let inter = wa.intersection(&wb).count() as f64;
    let union = wa.union(&wb).count() as f64;
    union > 0.0 && inter / union >= NEAR_DUPLICATE_JACCARD
}

/// Outcome of checking an outgoing text against the session ring (#957).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextLoopAction {
    /// No loop signal; deliver normally.
    Continue,
    /// First trip: deliver, but surface a system nudge for the next turn.
    Nudge,
    /// Second trip after an ignored nudge: abort through the loop-message
    /// path.
    Abort,
}

/// Per-session ring buffer of outgoing assistant texts (#957).
///
/// Lives on `AgentService` keyed by session id — NOT in the per-turn
/// context, which is reloaded from the DB every turn, so a loop that
/// spans turns needs state that outlives a single context load. Restart
/// re-arms the guard: same accepted semantics as #507's in-loop flags.
#[derive(Debug, Default)]
pub struct OutgoingTextRing {
    ring: VecDeque<String>,
    nudged: bool,
}

impl OutgoingTextRing {
    /// Record an outgoing text, then check it against the ring.
    ///
    /// The text is stored FIRST, so it counts as one hit against itself:
    /// three near-identical outgoing texts within the ring trip the guard.
    pub fn record_and_check(&mut self, text: &str) -> TextLoopAction {
        self.ring.push_back(text.to_string());
        while self.ring.len() > TEXT_LOOP_RING_CAP {
            self.ring.pop_front();
        }
        let hits = self.ring.iter().filter(|t| near_duplicate(text, t)).count();
        if hits >= TEXT_LOOP_TRIP_AT {
            if self.nudged {
                TextLoopAction::Abort
            } else {
                self.nudged = true;
                TextLoopAction::Nudge
            }
        } else {
            TextLoopAction::Continue
        }
    }
}
