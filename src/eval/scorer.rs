//! BinEval-style scorer + scorecard (#622).
//!
//! Following BinEval (arXiv 2506.27226), a holistic judgment is replaced with a
//! set of atomic yes/no [`BinaryQuestion`]s, each answered independently by a
//! [`Judge`], then aggregated into per-dimension and overall scores. This is
//! more interpretable and lower-variance than one holistic grade.
//!
//! Judging is abstracted behind the [`Judge`] trait so evals run fully offline:
//! tests use [`SequenceJudge`] (preset verdicts) or a [`ProviderJudge`] over the
//! fixture replay provider, while production would wrap a real LLM provider.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::brain::provider::{LLMRequest, Message, Provider};

/// System prompt steering the judge to answer a single atomic yes/no question.
const BINEVAL_JUDGE_SYSTEM: &str = "You are a strict evaluator. You are given an ARTIFACT and one \
     yes/no QUESTION about it. Answer with a single word on the first line: YES or NO. Judge only \
     what the artifact actually contains; if the fact is absent, answer NO.";

/// One atomic yes/no question, tagged with the dimension it scores.
#[derive(Debug, Clone)]
pub struct BinaryQuestion {
    pub dimension: String,
    pub text: String,
}

impl BinaryQuestion {
    pub fn new(dimension: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            dimension: dimension.into(),
            text: text.into(),
        }
    }
}

/// A judge's answer to one binary question.
#[derive(Debug, Clone)]
pub struct BinaryVerdict {
    pub yes: bool,
    pub explanation: Option<String>,
}

/// Parse a raw judge reply into a boolean. Looks at the first meaningful token;
/// defaults to `false` (fact not shown) when ambiguous, matching BINEVAL_JUDGE_SYSTEM.
pub fn parse_verdict(reply: &str) -> bool {
    let upper = reply.trim().to_ascii_uppercase();
    // Prefer an explicit leading YES/NO; fall back to first occurrence.
    for token in upper.split(|c: char| !c.is_ascii_alphabetic()) {
        match token {
            "YES" => return true,
            "NO" => return false,
            _ => {}
        }
    }
    false
}

/// Answers atomic yes/no questions about an artifact.
#[async_trait]
pub trait Judge: Send + Sync {
    async fn judge(&self, question: &str, artifact: &str) -> BinaryVerdict;
}

/// Deterministic offline judge that returns preset verdicts in order. For
/// testing aggregation without a provider.
pub struct SequenceJudge {
    verdicts: Mutex<std::collections::VecDeque<bool>>,
}

impl SequenceJudge {
    pub fn new(verdicts: impl IntoIterator<Item = bool>) -> Self {
        Self {
            verdicts: Mutex::new(verdicts.into_iter().collect()),
        }
    }
}

#[async_trait]
impl Judge for SequenceJudge {
    async fn judge(&self, _question: &str, _artifact: &str) -> BinaryVerdict {
        let yes = self
            .verdicts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .unwrap_or(false);
        BinaryVerdict {
            yes,
            explanation: None,
        }
    }
}

/// Judge backed by an LLM [`Provider`] (offline via the replay provider in
/// tests, a real provider in production).
pub struct ProviderJudge<'a> {
    provider: &'a dyn Provider,
    model: String,
}

impl<'a> ProviderJudge<'a> {
    pub fn new(provider: &'a dyn Provider, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

#[async_trait]
impl Judge for ProviderJudge<'_> {
    async fn judge(&self, question: &str, artifact: &str) -> BinaryVerdict {
        let prompt = format!("ARTIFACT:\n{artifact}\n\nQUESTION: {question}");
        let request = LLMRequest::new(self.model.clone(), vec![Message::user(prompt)])
            .with_system(BINEVAL_JUDGE_SYSTEM);
        match self.provider.complete(request).await {
            Ok(resp) => {
                let text = resp
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        crate::brain::provider::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                BinaryVerdict {
                    yes: parse_verdict(&text),
                    explanation: Some(text),
                }
            }
            // A judge failure is a NO (fact unverified), never a silent pass.
            Err(e) => BinaryVerdict {
                yes: false,
                explanation: Some(format!("judge error: {e}")),
            },
        }
    }
}

/// Per-dimension pass tally.
#[derive(Debug, Clone, Default)]
pub struct DimensionScore {
    pub passed: usize,
    pub total: usize,
}

impl DimensionScore {
    /// Fraction of questions in this dimension answered YES (0.0 for empty).
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
}

/// Aggregated result of scoring one artifact against a question set.
#[derive(Debug, Clone)]
pub struct Scorecard {
    pub results: Vec<(BinaryQuestion, BinaryVerdict)>,
    pub per_dimension: BTreeMap<String, DimensionScore>,
    pub passed: usize,
    pub total: usize,
}

impl Scorecard {
    /// Build a scorecard from pre-computed verdicts, aggregating per dimension
    /// and overall. Used by offline scorers (e.g. keyword survival) that do not
    /// route through a [`Judge`].
    pub fn from_verdicts(results: Vec<(BinaryQuestion, BinaryVerdict)>) -> Self {
        let mut per_dimension: BTreeMap<String, DimensionScore> = BTreeMap::new();
        let mut passed = 0;
        for (q, v) in &results {
            let entry = per_dimension.entry(q.dimension.clone()).or_default();
            entry.total += 1;
            if v.yes {
                entry.passed += 1;
                passed += 1;
            }
        }
        let total = results.len();
        Self {
            results,
            per_dimension,
            passed,
            total,
        }
    }

    /// Overall fraction of questions answered YES.
    pub fn overall(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }

    /// Whether the overall score meets or exceeds `threshold` (0.0..=1.0).
    pub fn passes(&self, threshold: f64) -> bool {
        self.overall() >= threshold
    }

    /// Stable, human-readable summary (overall + per-dimension breakdown).
    pub fn render(&self) -> String {
        let mut out = format!(
            "overall: {}/{} ({:.0}%)\n",
            self.passed,
            self.total,
            self.overall() * 100.0
        );
        for (dim, score) in &self.per_dimension {
            out.push_str(&format!(
                "  {dim}: {}/{} ({:.0}%)\n",
                score.passed,
                score.total,
                score.fraction() * 100.0
            ));
        }
        out
    }
}

/// Score an artifact by answering every question independently and aggregating.
pub async fn score(judge: &dyn Judge, questions: Vec<BinaryQuestion>, artifact: &str) -> Scorecard {
    let mut results = Vec::with_capacity(questions.len());
    for q in questions {
        let verdict = judge.judge(&q.text, artifact).await;
        results.push((q, verdict));
    }
    Scorecard::from_verdicts(results)
}
