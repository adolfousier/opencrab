//! Tool-name self-heal.
//!
//! Weaker models sometimes call a tool by a near-miss name they invented
//! rather than the registered one — e.g. `tg_send_message` for the real
//! `telegram_send` (observed 2026-06-07, issue #176). The tool schema IS
//! in every request, so the model knew it wanted to send a Telegram
//! message; it just guessed the wrong identifier.
//!
//! This mirrors the per-parameter `PARAM_ALIASES` heal in `registry.rs`
//! but at the tool-name level: map a requested-but-unknown tool name to
//! the closest registered tool so the call routes instead of erroring.
//!
//! Conservative by design — a wrong route could fire an unintended (maybe
//! destructive) tool, so a heal only happens on a UNIQUE, high-confidence
//! match. When in doubt, return `None` and let the caller surface the
//! normal "tool not found" error.

/// Common abbreviations models use in invented tool names. Expanded before
/// token matching so `tg` lines up with `telegram`, `msg` with `message`, etc.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("tg", "telegram"),
    ("tgram", "telegram"),
    ("dc", "discord"),
    ("wa", "whatsapp"),
    ("msg", "message"),
    ("img", "image"),
    ("vid", "video"),
];

/// Resolve a requested-but-unknown tool name to the closest registered
/// tool, or `None` when there is no confident, unambiguous match.
pub fn resolve_tool_name(requested: &str, registered: &[String]) -> Option<String> {
    if requested.is_empty() || registered.is_empty() {
        return None;
    }

    // 0. Exact (defensive — the caller usually checked already).
    if let Some(r) = registered.iter().find(|r| r.as_str() == requested) {
        return Some(r.clone());
    }

    // 1. Normalized-exact: lowercase + strip non-alphanumerics. Catches
    //    `tg-send` vs `tg_send`, casing, and stray punctuation. Must be
    //    unique to heal.
    let req_norm = normalize(requested);
    let norm_matches: Vec<&String> = registered
        .iter()
        .filter(|r| normalize(r) == req_norm)
        .collect();
    if norm_matches.len() == 1 {
        return Some(norm_matches[0].clone());
    }

    // 2. Token overlap with abbreviation expansion. A registered tool
    //    matches when ALL of its (expanded) name tokens appear in the
    //    requested (expanded) tokens AND it has at least two tokens — so
    //    `telegram_send` matches `tg_send_message`
    //    (telegram, send ⊆ telegram, send, message) but `send_photo` does
    //    NOT match `telegram_send` (telegram absent). Pick the most-
    //    specific candidate; bail on a tie (ambiguous).
    let req_tokens = expand_tokens(requested);
    let mut candidates: Vec<&String> = Vec::new();
    let mut best_len = 0usize;
    for r in registered {
        let r_tokens = expand_tokens(r);
        if r_tokens.len() >= 2 && r_tokens.iter().all(|t| req_tokens.contains(t)) {
            match r_tokens.len().cmp(&best_len) {
                std::cmp::Ordering::Greater => {
                    best_len = r_tokens.len();
                    candidates = vec![r];
                }
                std::cmp::Ordering::Equal => candidates.push(r),
                std::cmp::Ordering::Less => {}
            }
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }

    // 3. Typo fallback: small edit distance on the normalized names, must
    //    be unique. Catches `telegram_sned` → `telegram_send`. Budget is
    //    ~1 edit per 6 chars (min 1, cap 2) so unrelated names don't match.
    let limit = (req_norm.len() / 6).clamp(1, 2);
    let close: Vec<&String> = registered
        .iter()
        .filter(|r| levenshtein(&normalize(r), &req_norm) <= limit)
        .collect();
    if close.len() == 1 {
        return Some(close[0].clone());
    }

    None
}

/// Lowercase + keep only ASCII alphanumerics.
fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Split a tool name into lowercase tokens on `_`/`-`/other separators and
/// camelCase boundaries, then expand known abbreviations.
fn expand_tokens(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_was_lower_or_digit = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            // camelCase boundary: lower→Upper starts a new token.
            if c.is_ascii_uppercase() && prev_was_lower_or_digit && !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            cur.push(c.to_ascii_lowercase());
            prev_was_lower_or_digit = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
            prev_was_lower_or_digit = false;
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
        .into_iter()
        .map(|t| {
            ABBREVIATIONS
                .iter()
                .find(|(abbr, _)| *abbr == t)
                .map(|(_, full)| full.to_string())
                .unwrap_or(t)
        })
        .collect()
}

/// Classic Levenshtein edit distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
