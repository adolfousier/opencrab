//! Compaction-fidelity harness + dataset format (#621).
//!
//! Compaction (the 65% LLM summary in the agent service) is where context most
//! silently degrades. This harness measures whether critical facts survive a
//! summary. A [`CompactionDataset`] pairs a synthetic conversation with fact
//! [`FactProbe`]s (pending tasks, user prefs, file edits, errors).
//!
//! Two scoring paths, both producing a [`Scorecard`]:
//! - [`CompactionDataset::keyword_scorecard`] — deterministic, fully offline:
//!   a probe survives iff its expected keywords appear in the summary. This is
//!   the default CI-able signal.
//! - [`CompactionDataset::judge_scorecard`] — semantic grading via a [`Judge`]
//!   (offline over the replay provider in tests, a real LLM in a live run).

use serde::Deserialize;

use super::scorer::{BinaryQuestion, BinaryVerdict, Judge, Scorecard, score};
use crate::brain::provider::Message;

/// One message in a dataset conversation.
#[derive(Debug, Clone, Deserialize)]
pub struct DatasetMessage {
    pub role: String,
    pub text: String,
}

/// A fact that should survive compaction, plus the question a judge would ask
/// and the keywords whose presence proves survival offline.
#[derive(Debug, Clone, Deserialize)]
pub struct FactProbe {
    pub dimension: String,
    pub question: String,
    #[serde(default)]
    pub expect_keywords: Vec<String>,
}

/// A compaction-fidelity dataset: a conversation and the facts that must
/// survive its summary.
#[derive(Debug, Clone, Deserialize)]
pub struct CompactionDataset {
    pub name: String,
    pub conversation: Vec<DatasetMessage>,
    pub probes: Vec<FactProbe>,
}

impl CompactionDataset {
    /// Parse a dataset from JSON.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// The bundled synthetic seed dataset (no user identifiers).
    pub fn seed() -> Self {
        Self::from_json(SEED_DATASET_JSON).expect("seed dataset is valid JSON")
    }

    /// Build provider messages from the conversation, for feeding a real
    /// compaction run.
    pub fn messages(&self) -> Vec<Message> {
        self.conversation
            .iter()
            .map(|m| match m.role.as_str() {
                "assistant" => Message::assistant(m.text.clone()),
                "system" => Message::system(m.text.clone()),
                _ => Message::user(m.text.clone()),
            })
            .collect()
    }

    /// The probes as BinEval questions for the judge path.
    pub fn questions(&self) -> Vec<BinaryQuestion> {
        self.probes
            .iter()
            .map(|p| BinaryQuestion::new(p.dimension.clone(), p.question.clone()))
            .collect()
    }

    /// Deterministic offline fact-survival: each probe passes iff every one of
    /// its expected keywords appears (case-insensitive) in the summary. A probe
    /// with no keywords cannot be checked offline and is scored as not-survived.
    pub fn keyword_scorecard(&self, summary: &str) -> Scorecard {
        let haystack = summary.to_ascii_lowercase();
        let results = self
            .probes
            .iter()
            .map(|p| {
                let survived = !p.expect_keywords.is_empty()
                    && p.expect_keywords
                        .iter()
                        .all(|k| haystack.contains(&k.to_ascii_lowercase()));
                let missing: Vec<&str> = p
                    .expect_keywords
                    .iter()
                    .filter(|k| !haystack.contains(&k.to_ascii_lowercase()))
                    .map(|k| k.as_str())
                    .collect();
                let explanation = if survived {
                    None
                } else if p.expect_keywords.is_empty() {
                    Some("no keywords to check offline".to_string())
                } else {
                    Some(format!("missing: {}", missing.join(", ")))
                };
                (
                    BinaryQuestion::new(p.dimension.clone(), p.question.clone()),
                    BinaryVerdict {
                        yes: survived,
                        explanation,
                    },
                )
            })
            .collect();
        Scorecard::from_verdicts(results)
    }

    /// Semantic fact-survival via a [`Judge`] grading each probe against the
    /// summary.
    pub async fn judge_scorecard(&self, judge: &dyn Judge, summary: &str) -> Scorecard {
        score(judge, self.questions(), summary).await
    }
}

/// Synthetic seed dataset. A short agentic coding session carrying a user
/// preference, a pending task, a concrete file edit, and an error, each with
/// probe keywords. No real names, handles, or timestamps.
const SEED_DATASET_JSON: &str = r#"{
    "name": "seed-coding-session",
    "conversation": [
        { "role": "user", "text": "Refactor the retry logic in net/client.rs. Always use tabs, never spaces." },
        { "role": "assistant", "text": "Understood. I will edit net/client.rs and keep tab indentation." },
        { "role": "assistant", "text": "Edited net/client.rs: extracted retry_with_backoff. The test suite fails with error E0433: unresolved import crate::net::Backoff." },
        { "role": "user", "text": "Fix the import, then also update the CHANGELOG before you finish." },
        { "role": "assistant", "text": "Import fixed. Pending: update the CHANGELOG entry for the retry refactor." }
    ],
    "probes": [
        { "dimension": "user_prefs", "question": "Does the summary preserve the tabs-not-spaces indentation preference?", "expect_keywords": ["tabs"] },
        { "dimension": "pending_tasks", "question": "Does the summary keep the pending CHANGELOG update task?", "expect_keywords": ["CHANGELOG"] },
        { "dimension": "files_modified", "question": "Does the summary record the edit to net/client.rs?", "expect_keywords": ["net/client.rs"] },
        { "dimension": "errors", "question": "Does the summary retain the E0433 import error encountered?", "expect_keywords": ["E0433"] }
    ]
}"#;
