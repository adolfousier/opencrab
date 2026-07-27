//! Which opportunities are capability gaps, and what must answer them (#842).
//!
//! The detectors that emit promotion, command-pattern and skill-sequence
//! opportunities already know the answer: they name `rsi_propose` and the kind
//! in the text they build. That knowledge was then thrown away, leaving the
//! agent to classify each one from a flat bullet list. It classified them all
//! as guidance and answered with `self_improve`, writing another brain-file
//! rule. A recurring command shape is a missing capability; no rule closes it,
//! so the gap survives into the next cycle and is answered the same way.
//!
//! This module recovers the disposition from the opportunity text and turns it
//! into an instruction the agent cannot read as optional.

/// What a capability-gap opportunity must be filed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalKind {
    Tool,
    Command,
    Skill,
}

impl ProposalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Command => "command",
            Self::Skill => "skill",
        }
    }
}

/// The kind an opportunity demands, or `None` when it is a guidance issue.
///
/// Keyed off the `rsi_propose kind=<k>` marker the detectors embed. Matching
/// the marker rather than re-deriving the classification keeps this in step
/// with the detectors by construction: an opportunity that does not name the
/// tool is guidance work, which is the correct default.
pub fn required_kind(opportunity: &str) -> Option<ProposalKind> {
    let lower = opportunity.to_lowercase();
    // A single opportunity may offer a choice ("a tool ... or a skill ...").
    // First marker wins, matching the order the detector presents them in.
    let first = |needle: &str| lower.find(needle);
    let mut found: Vec<(usize, ProposalKind)> = [
        ("kind=tool", ProposalKind::Tool),
        ("kind=command", ProposalKind::Command),
        ("kind=skill", ProposalKind::Skill),
    ]
    .into_iter()
    .filter_map(|(needle, kind)| first(needle).map(|at| (at, kind)))
    .collect();
    found.sort_by_key(|(at, _)| *at);
    found.first().map(|(_, kind)| *kind)
}

/// How many of these opportunities are capability gaps.
pub fn capability_count(opportunities: &[String]) -> usize {
    opportunities
        .iter()
        .filter(|o| required_kind(o).is_some())
        .count()
}

/// The required-actions block appended to the cycle prompt.
///
/// Empty when nothing in the batch is a capability gap, so a guidance-only
/// cycle keeps its original shape.
pub fn required_actions_block(opportunities: &[String]) -> String {
    let gaps: Vec<(usize, ProposalKind)> = opportunities
        .iter()
        .enumerate()
        .filter_map(|(i, o)| required_kind(o).map(|k| (i + 1, k)))
        .collect();
    if gaps.is_empty() {
        return String::new();
    }

    let mut block = String::from(
        "\nREQUIRED ACTIONS\n\n\
         The opportunities below are capability gaps, not guidance gaps. They \
         describe something you cannot currently do. A brain-file rule cannot \
         close them: the gap survives, gets re-detected next cycle, and the \
         rule becomes prompt bloat that degrades every later cycle.\n\n\
         `self_improve` does NOT answer these. `rsi_propose` does. You are not \
         installing anything; you are filing a proposal for the user to accept \
         or reject.\n\n",
    );
    for (n, kind) in &gaps {
        block.push_str(&format!(
            "- Opportunity {n}: call rsi_propose with kind={}\n",
            kind.as_str()
        ));
    }
    block.push_str(&format!(
        "\nThat is {} required rsi_propose call(s) this cycle. If you judge one \
         to be a duplicate of an existing proposal or genuinely not worth \
         filing, say which and why in your summary. Do not silently skip it.\n",
        gaps.len()
    ));
    block
}
