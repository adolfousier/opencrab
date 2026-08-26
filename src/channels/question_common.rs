//! Shared mechanics for the merged suggestion tool (#764 R-items,
//! #1178 merge) plus the per-channel label budgets every renderer
//! shares (#1176 G1/G3).

use std::collections::HashSet;

/// Neutral validation outcome for [`check_options`]. Callers own their
/// tool-specific error wording — both tools' messages are pinned
/// byte-for-byte by existing tests, so this layer never formats text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OptionsError {
    TooFew { got: usize, min: usize },
    TooMany(usize),
    Duplicate(String),
}

/// Shared trim/filter/dedup validation (#764 R1): trim each entry, drop
/// empties, enforce the min..=max window and distinctness.
pub(crate) fn check_options(
    raw: Vec<String>,
    min: usize,
    max: usize,
) -> Result<Vec<String>, OptionsError> {
    let options: Vec<String> = raw
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if options.len() < min {
        return Err(OptionsError::TooFew {
            got: options.len(),
            min,
        });
    }
    if options.len() > max {
        return Err(OptionsError::TooMany(options.len()));
    }
    let mut seen = HashSet::new();
    for opt in &options {
        if !seen.insert(opt.as_str()) {
            return Err(OptionsError::Duplicate(opt.clone()));
        }
    }
    Ok(options)
}

/// Silent twin of [`resolve_channel_or_error`] for the non-blocking tool:
/// suggest_options just returns without rendering when neither lands.
pub(crate) async fn resolve_channel_or_silent<T>(
    session_lookup: impl std::future::Future<Output = Option<T>>,
    owner_lookup: impl std::future::Future<Output = Option<T>>,
) -> Option<T> {
    match session_lookup.await {
        Some(id) => Some(id),
        None => owner_lookup.await,
    }
}

/// Per-channel option-label budgets (#1176 G1): the widest label a renderer
/// prints before folding to an ellipsized form. Values are CHARACTERS, not
/// bytes; all truncation goes through [`truncate_label`].
///
/// Telegram is deliberately absent. It used to truncate at 60 and fold the
/// whole keyboard past a 30-char threshold; #1204 replaced both with a
/// measured layout ladder in `telegram::suggest_options`, which never
/// truncates — it changes the SHAPE of the controls instead, so a label too
/// long for a button becomes a numbered entry with its full text in the body.
pub(crate) const DISCORD_LABEL_BUDGET: usize = 80;
pub(crate) const SLACK_LABEL_BUDGET: usize = 75;

/// The single char-based truncation helper (#1176 G3): passthrough under the
/// budget, otherwise cut to leave room for a literal `...` tail. Replaces the
/// byte-based `truncate_str` at every suggestion-label site.
pub(crate) fn truncate_label(s: &str, budget: usize) -> String {
    if s.chars().count() <= budget {
        return s.to_string();
    }
    let mut out = crate::utils::truncate_chars(s, budget.saturating_sub(3)).to_string();
    out.push_str("...");
    out
}
