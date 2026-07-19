//! Majority-vote judge panel over real providers (live-L1, #630).
//!
//! A [`PanelJudge`] asks every member judge the same question independently and
//! passes it only when the fraction of YES verdicts exceeds a threshold
//! (default 0.5 — a strict majority, so ties fail). Polling several models
//! reduces single-model bias and verdict variance (the BinEval motivation).
//!
//! The panel is provider-agnostic: members are any [`Judge`], so the voting
//! logic is tested offline with scripted judges. [`panel_from_providers`] builds
//! a live panel of [`LiveJudge`]s, each pinned to its provider's default model;
//! the real network calls only happen in an on-demand live run.

use std::sync::Arc;

use async_trait::async_trait;

use super::scorer::{BinaryVerdict, Judge, judge_via_provider};
use crate::brain::provider::Provider;

/// A judge backed by an owned provider `Arc`, pinned to a model. Owned (unlike
/// [`super::scorer::ProviderJudge`]) so it can live inside a panel.
pub struct LiveJudge {
    provider: Arc<dyn Provider>,
    model: String,
}

impl LiveJudge {
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

#[async_trait]
impl Judge for LiveJudge {
    async fn judge(&self, question: &str, artifact: &str) -> BinaryVerdict {
        judge_via_provider(self.provider.as_ref(), &self.model, question, artifact).await
    }
}

/// A panel of judges that votes on each question.
pub struct PanelJudge {
    members: Vec<Box<dyn Judge>>,
    /// A question passes when the YES fraction is strictly greater than this
    /// (default 0.5 → strict majority; ties fail).
    threshold: f64,
}

impl PanelJudge {
    pub fn new(members: Vec<Box<dyn Judge>>) -> Self {
        Self {
            members,
            threshold: 0.5,
        }
    }

    /// Set the YES-fraction threshold a question must exceed to pass.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[async_trait]
impl Judge for PanelJudge {
    async fn judge(&self, question: &str, artifact: &str) -> BinaryVerdict {
        if self.members.is_empty() {
            // An empty panel cannot confirm anything.
            return BinaryVerdict {
                yes: false,
                explanation: Some("empty judge panel".to_string()),
            };
        }
        let mut yes = 0usize;
        for member in &self.members {
            if member.judge(question, artifact).await.yes {
                yes += 1;
            }
        }
        let total = self.members.len();
        let fraction = yes as f64 / total as f64;
        BinaryVerdict {
            yes: fraction > self.threshold,
            explanation: Some(format!("{yes}/{total} judges said YES")),
        }
    }
}

/// Build a live judge panel from resolved providers, each pinned to its own
/// configured `default_model`.
pub fn panel_from_providers(providers: &[Arc<dyn Provider>]) -> PanelJudge {
    let members: Vec<Box<dyn Judge>> = providers
        .iter()
        .map(|p| {
            let model = p.default_model().to_string();
            Box::new(LiveJudge::new(p.clone(), model)) as Box<dyn Judge>
        })
        .collect();
    PanelJudge::new(members)
}
