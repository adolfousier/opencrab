//! Detect recurring tool SEQUENCES that should become a skill (#504).
//!
//! The command detector ([`super::rsi_command_patterns`]) codifies what the
//! user keeps TYPING. This codifies what the agent keeps DOING: an ordered
//! run of tools (e.g. `read_file -> edit_file -> bash`) that recurs across
//! many separate sessions is a workflow worth a `SKILL.md`. It slides an
//! n-gram window over each session's tool sequence and counts how many
//! DISTINCT sessions contain each n-gram; one seen in enough sessions is a
//! skill-promotion candidate the RSI agent files via `rsi_propose kind=skill`.
//!
//! Pure and side-effect free so the windowing rules are unit tested without a
//! database. Conservative: cross-SESSION recurrence (not raw repetition
//! inside one session) is the signal, and single-tool runs are ignored, so a
//! tight loop of one tool never masquerades as a workflow.

use std::collections::{HashMap, HashSet};

/// Sequence length to look for. 3 is the sweet spot: long enough to be a
/// real workflow, short enough to recur across sessions.
pub const SEQUENCE_LEN: usize = 3;

/// Minimum distinct sessions an n-gram must appear in to be a candidate.
pub const SEQUENCE_MIN_SESSIONS: usize = 3;

/// A recurring tool sequence worth a skill.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillSequenceCandidate {
    /// The ordered tool names in the sequence.
    pub sequence: Vec<String>,
    /// How many distinct sessions ran this sequence.
    pub sessions: usize,
}

/// Find tool sequences of length `n` present in at least `min_sessions`
/// distinct sessions, most widespread first. Each element of `sessions` is
/// one session's tool names in execution order.
pub fn skill_sequence_candidates(
    sessions: &[Vec<String>],
    n: usize,
    min_sessions: usize,
) -> Vec<SkillSequenceCandidate> {
    if n == 0 {
        return Vec::new();
    }
    // n-gram -> set of session indices that contain it. A session counts an
    // n-gram once no matter how many times it repeats it, so a within-session
    // loop cannot inflate the cross-session signal.
    let mut by_ngram: HashMap<Vec<String>, HashSet<usize>> = HashMap::new();
    for (idx, seq) in sessions.iter().enumerate() {
        if seq.len() < n {
            continue;
        }
        let mut seen_here: HashSet<Vec<String>> = HashSet::new();
        for window in seq.windows(n) {
            // Skip single-tool runs (all identical): a tight loop of one
            // tool is not a workflow.
            if window.iter().all(|t| t == &window[0]) {
                continue;
            }
            let ngram = window.to_vec();
            if seen_here.insert(ngram.clone()) {
                by_ngram.entry(ngram).or_default().insert(idx);
            }
        }
    }
    let mut out: Vec<SkillSequenceCandidate> = by_ngram
        .into_iter()
        .filter(|(_, sessions)| sessions.len() >= min_sessions)
        .map(|(sequence, sessions)| SkillSequenceCandidate {
            sequence,
            sessions: sessions.len(),
        })
        .collect();
    // Most widespread first; ties broken by the sequence for a stable order
    // (so the RSI dedup hash does not churn on equivalent state).
    out.sort_by(|a, b| {
        b.sessions
            .cmp(&a.sessions)
            .then_with(|| a.sequence.cmp(&b.sequence))
    });
    out
}

/// Group flat `(session_id, tool_name)` rows (ordered so each session's rows
/// are adjacent and in execution order) into per-session tool sequences.
pub fn group_sessions(rows: &[(String, String)]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut current_id: Option<&str> = None;
    for (sid, tool) in rows {
        if current_id != Some(sid.as_str()) {
            out.push(Vec::new());
            current_id = Some(sid.as_str());
        }
        out.last_mut().expect("pushed above").push(tool.clone());
    }
    out
}
