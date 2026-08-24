//! Shared mechanics for the suggestion tools
//! (#764 R1/R2/R3/R5). The two tools stay semantically different
//! (blocking vs non-blocking); only their duplicated *mechanics* live here.

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
/// suggest_followups just returns without rendering when neither lands.
pub(crate) async fn resolve_channel_or_silent<T>(
    session_lookup: impl std::future::Future<Output = Option<T>>,
    owner_lookup: impl std::future::Future<Output = Option<T>>,
) -> Option<T> {
    match session_lookup.await {
        Some(id) => Some(id),
        None => owner_lookup.await,
    }
}
