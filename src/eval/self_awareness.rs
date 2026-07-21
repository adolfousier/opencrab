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

    /// Second seed: a forwarded file whose text the agent must read from the
    /// persisted channel-attachments store instead of falsely claiming it can't
    /// access it — the same self-awareness failure class as the STT case, but
    /// about a persisted RESOURCE rather than a compiled capability (#659).
    pub fn seed_channel_attachment() -> Self {
        Self::from_json(SEED_CHANNEL_ATTACHMENT_JSON).expect("attachment scenario is valid JSON")
    }

    /// Tool-set awareness (#672): a task mapping to an extended tool the agent
    /// must reach for (via `tool_search`) rather than claim it can't do.
    pub fn seed_tool_set() -> Self {
        Self::from_json(SEED_TOOL_SET_JSON).expect("tool-set scenario is valid JSON")
    }

    /// Environment awareness (#672): the agent answers its model/version/OS from
    /// Runtime Info rather than denying it can introspect.
    pub fn seed_environment() -> Self {
        Self::from_json(SEED_ENVIRONMENT_JSON).expect("environment scenario is valid JSON")
    }

    /// All bundled self-awareness scenarios.
    pub fn seeds() -> Vec<Self> {
        vec![
            Self::seed(),
            Self::seed_channel_attachment(),
            Self::seed_tool_set(),
            Self::seed_environment(),
        ]
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

/// Synthetic seed scenario: a user forwarded a file whose text is not inline in
/// the chat history. The right response reads the persisted attachment from the
/// channel-attachments store; the wrong one falsely claims the file/message
/// isn't stored and can't be read (the real incident this locks in). Same
/// self-awareness class as the STT case: aware of a runtime RESOURCE it already
/// has rather than a compiled capability.
const SEED_CHANNEL_ATTACHMENT_JSON: &str = r#"{
    "name": "forwarded-file-not-read",
    "prompt": "In a chat channel a user forwarded a message with a file attached (a markdown report) and asked you to audit it. The report text is not inline in the chat history. Handle it.",
    "probes": [
        { "dimension": "reads_persisted_attachment", "question": "Does it look for and read the persisted attachment (the channel_attachments store, or reading the file from disk) instead of treating the contents as unavailable?", "expect_keywords": ["channel_attachments"] },
        { "dimension": "no_false_blocker", "question": "Does it AVOID falsely claiming the file or the other user's message is not stored / cannot be read?", "forbid_keywords": ["aren't stored", "not stored", "never landed", "can't read", "cannot read"] }
    ]
}"#;

/// Synthetic seed: a task that maps to an EXTENDED tool whose schema isn't
/// loaded in lazy mode. The right response reaches for its own tools
/// (`tool_search` to activate the browser tool); the wrong one claims it has no
/// browser ability. Most interesting under a lazy-mode brain (#672).
const SEED_TOOL_SET_JSON: &str = r#"{
    "name": "tool-set-awareness-lazy",
    "prompt": "You are in lazy-tools mode, so most tool schemas are not loaded yet. A user asks you to open a real web page in a browser and click a button on it. Handle it.",
    "probes": [
        { "dimension": "reaches_for_tool", "question": "Does it reach for its own tooling (call tool_search, or name the browser tool it will activate) rather than treating browser control as unavailable?", "expect_keywords": ["tool_search"] },
        { "dimension": "no_false_absence", "question": "Does it AVOID falsely claiming it has no browser / no tool for this / cannot do it?", "forbid_keywords": ["i can't", "i cannot", "i don't have", "no tool", "not able to", "unable to"] }
    ]
}"#;

/// Synthetic seed: the agent is asked about its own runtime environment. The
/// right response answers from Runtime Info (model, version, OS); the wrong one
/// refuses or hedges as if it can't know (#672).
const SEED_ENVIRONMENT_JSON: &str = r#"{
    "name": "environment-awareness",
    "prompt": "The user asks: what AI model, OpenCrabs version, and OS are you running right now? Answer from your runtime context.",
    "probes": [
        { "dimension": "states_runtime_facts", "question": "Does it state its model and OpenCrabs version from Runtime Info rather than refusing or guessing?", "expect_keywords": ["model"] },
        { "dimension": "no_capability_denial", "question": "Does it AVOID the generic can't-introspect denial?", "forbid_keywords": ["as an ai", "i don't know", "i'm not sure", "cannot determine", "no way to know"] }
    ]
}"#;
