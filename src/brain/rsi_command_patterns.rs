//! Detect repeated user requests that should become a slash command (#504).
//!
//! RSI already promotes high-frequency bash subsystems to tool/skill
//! candidates. This closes the other half: phrases the USER types over and
//! over (the `/standup` case). It groups recent user-message texts by a
//! normalized signature and surfaces any signature seen enough times as a
//! command-promotion candidate for the RSI agent to file via `rsi_propose`.
//!
//! Pure and side-effect free so the grouping rules are unit tested without a
//! database. Conservative by design: short/trivial messages are ignored and
//! the threshold is high, so a genuine recurring intent surfaces while
//! one-off chatter does not.

use std::collections::HashMap;

/// Minimum times a normalized request must recur to be a candidate.
pub const COMMAND_PATTERN_THRESHOLD: usize = 4;

/// A recurring user-request pattern worth a slash command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandCandidate {
    /// The normalized signature the requests share.
    pub signature: String,
    /// How many messages matched it (in the analyzed window).
    pub count: usize,
    /// A few original phrasings, for the proposal rationale.
    pub samples: Vec<String>,
}

/// Group `requests` by normalized signature and return the ones seen at least
/// `threshold` times, most frequent first. Each request contributes at most
/// once per exact original text (so a spammed identical line does not inflate
/// the count beyond its distinct phrasings... actually distinct COUNT is what
/// we want: repeated identical asks ARE the signal, so every occurrence
/// counts).
pub fn command_candidates(requests: &[String], threshold: usize) -> Vec<CommandCandidate> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for raw in requests {
        let Some(sig) = signature(raw) else {
            continue;
        };
        groups.entry(sig).or_default().push(raw.trim().to_string());
    }
    let mut out: Vec<CommandCandidate> = groups
        .into_iter()
        .filter(|(_, v)| v.len() >= threshold)
        .map(|(signature, originals)| {
            // Distinct sample phrasings, up to 3, for the rationale.
            let mut samples: Vec<String> = Vec::new();
            for o in &originals {
                if !samples.contains(o) {
                    samples.push(o.clone());
                }
                if samples.len() == 3 {
                    break;
                }
            }
            CommandCandidate {
                signature,
                count: originals.len(),
                samples,
            }
        })
        .collect();
    // Most frequent first; ties broken by signature for a stable order (so
    // the RSI dedup hash does not churn on equivalent state).
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.signature.cmp(&b.signature))
    });
    out
}

/// Normalize a request into a signature, or `None` when it is not a
/// command-shaped ask. Lowercases, collapses whitespace, strips leading
/// `@mentions` and trailing punctuation, and keeps the first few significant
/// words. Rejects messages that are too short, are already a slash command,
/// or read as freeform prose rather than a recurring directive.
fn signature(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.starts_with('/') {
        // Already a command, or empty.
        return None;
    }
    // Drop leading @mentions (group chats prefix the bot handle).
    let t: String = t
        .split_whitespace()
        .skip_while(|w| w.starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ");
    let lower = t.to_lowercase();
    // Keep only the leading run of significant words: a recurring ask is
    // usually a short imperative ("morning standup", "run the tests"). Long
    // freeform prose is not command-shaped.
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 2 || words.len() > 6 {
        // 1 word is too ambiguous; >6 is prose, not a command.
        return None;
    }
    Some(words.join(" "))
}
