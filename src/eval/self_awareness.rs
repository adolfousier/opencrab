//! Capability self-awareness eval (#636, #644).
//!
//! Measures whether the agent is AWARE of its own tooling: faced with a
//! capability that is compiled but unconfigured, does it **reach for its own
//! config tooling** rather than **reimplement from scratch** — the voice-note-STT
//! failure mode the preamble fix (#635) targets. It deliberately does NOT score
//! single-turn task *completion* (fully enabling local-stt), which is a
//! multi-turn concern; awareness = reaches for config tooling AND does not build
//! an external replacement.
//!
//! A [`SelfAwarenessScenario`] pairs a user request with [`BehaviorProbe`]s that
//! reward the right behavior (mentions the built-in, configures via own tooling)
//! and forbid the wrong one (pip / Python / a homegrown transcriber). Scoring
//! parallels the compaction harness: a deterministic offline keyword path plus a
//! semantic judge path for live runs.

use serde::Deserialize;

use super::scorer::{BinaryQuestion, BinaryVerdict, Judge, Scorecard, score};

/// A behavioral check on the agent's response: required keywords that must all
/// appear AND forbidden keywords that must all be absent.
#[derive(Debug, Clone, Deserialize)]
pub struct BehaviorProbe {
    pub dimension: String,
    pub question: String,
    #[serde(default)]
    pub expect_keywords: Vec<String>,
    #[serde(default)]
    pub forbid_keywords: Vec<String>,
}

/// A user request plus the behavioral probes its response must satisfy.
#[derive(Debug, Clone, Deserialize)]
pub struct SelfAwarenessScenario {
    pub name: String,
    /// The user request (and any situational note, e.g. that a voice note
    /// arrived and STT is unset).
    pub prompt: String,
    pub probes: Vec<BehaviorProbe>,
}

impl SelfAwarenessScenario {
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// The bundled seed scenario (synthetic, no user identifiers).
    pub fn seed() -> Self {
        Self::from_json(SEED_SCENARIO_JSON).expect("seed scenario is valid JSON")
    }

    /// The probes as BinEval questions for the judge path.
    pub fn questions(&self) -> Vec<BinaryQuestion> {
        self.probes
            .iter()
            .map(|p| BinaryQuestion::new(p.dimension.clone(), p.question.clone()))
            .collect()
    }

    /// Deterministic offline scoring: a probe passes iff every expected keyword
    /// appears (case-insensitive) AND no forbidden keyword appears.
    pub fn keyword_scorecard(&self, response: &str) -> Scorecard {
        let hay = response.to_ascii_lowercase();
        let results = self
            .probes
            .iter()
            .map(|p| {
                let has_all_expected = p
                    .expect_keywords
                    .iter()
                    .all(|k| hay.contains(&k.to_ascii_lowercase()));
                let hit_forbidden: Vec<&str> = p
                    .forbid_keywords
                    .iter()
                    .filter(|k| hay.contains(&k.to_ascii_lowercase()))
                    .map(|k| k.as_str())
                    .collect();
                let passed = has_all_expected && hit_forbidden.is_empty();
                let explanation = if passed {
                    None
                } else if !hit_forbidden.is_empty() {
                    Some(format!("used forbidden: {}", hit_forbidden.join(", ")))
                } else {
                    Some("missing an expected signal".to_string())
                };
                (
                    BinaryQuestion::new(p.dimension.clone(), p.question.clone()),
                    BinaryVerdict {
                        yes: passed,
                        explanation,
                    },
                )
            })
            .collect();
        Scorecard::from_verdicts(results)
    }

    /// Semantic scoring via a [`Judge`] grading each probe against the response.
    pub async fn judge_scorecard(&self, judge: &dyn Judge, response: &str) -> Scorecard {
        score(judge, self.questions(), response).await
    }
}

/// Synthetic seed scenario: a voice note arrives with STT unconfigured while
/// `local-stt` is compiled in. The right response uses/enables the built-in
/// (ideally by CALLING its config tooling); the wrong one builds a transcriber.
///
/// The producer is given its real tools (#643), so a tool-calling agent can do
/// the right thing — call `config_manager` / `tool_search` to enable
/// `local-stt` — and the grader sees the rendered tool call rather than a dead
/// stub. The prompt stays natural: the agent is free to act.
const SEED_SCENARIO_JSON: &str = r#"{
    "name": "voice-note-stt-unconfigured",
    "prompt": "A user sent a voice note but speech-to-text is not configured, and local-stt is compiled into this binary. Handle it.",
    "probes": [
        { "dimension": "self_configures", "question": "Does it reach for OpenCrabs' own config tooling instead of asking the user to write code? A [tool_call] to config_manager, tool_search, or an /onboard command counts.", "expect_keywords": ["config"] },
        { "dimension": "no_reimplement", "question": "Does it avoid building a transcription service from scratch?", "forbid_keywords": ["pip install", "python codebase", "def transcribe"] }
    ]
}"#;
