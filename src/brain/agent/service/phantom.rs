//! Phantom-tool-call detection.
//!
//! Catches assistant text that narrates actions ("Let me check…", "I'll
//! update…", "Pushed.") without emitting any actual tool calls. Two
//! detectors:
//!
//! * `has_phantom_tool_intent_no_tools` — relaxed gate, used when the
//!   iteration already produced zero tool uses. Bare intent phrases or
//!   short past-tense terminal claims are sufficient.
//! * `has_phantom_tool_intent` — strict gate for the general path; needs
//!   either standalone strong signals (multi-step plans, completion
//!   claims, gerund drops) or an intent phrase + file-path corroboration.
//!
//! All language-dependent data (intent phrases, action verbs, regex
//! patterns) lives in `phantom_lang/` TOML files, loaded at compile time.
//! Language detection is automatic via character-set heuristics.

use regex::Regex;

use super::phantom_lang;

/// Relaxed phantom detection used when the caller already knows the
/// model emitted **zero tool_use blocks** this iteration. In that case
/// any bare intent phrase is phantom — no path or extension
/// corroboration required, because the tool count already proves
/// nothing happened.
///
/// Structured answers are exempt. Commit-log tables, code blocks, and
/// long bulleted lists inevitably contain intent-phrase substrings
/// (e.g. a commit message literally titled
/// `"fix(heal): phantom detector lets 'Let me check...' loops slide"`
/// — seen in logs 2026-04-17 03:38:37 — triggered this detector on
/// itself). A legitimate answer rendered as a table is NEVER a phantom,
/// even if its content happens to quote a phrase we watch for.
pub fn has_phantom_tool_intent_no_tools(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 20 {
        return false;
    }
    let lead = prose_lead_in(trimmed);
    if lead.is_empty() {
        return false;
    }
    let lower = lead.to_lowercase();
    let lang = phantom_lang::detect_language(trimmed);
    if lang_intent_match(&lower, &lang.intent_phrases) {
        return true;
    }
    has_past_tense_action_claim(&lower, &lang.action_verbs)
}

/// Detects short past-tense completion claims like `"Pushed."`, `"Deployed."`,
/// `"Migration created."` — sentences that announce an action's done without
/// having executed any tool. Only used in the zero-tool-call path; loose
/// matching elsewhere would false-positive on conversational recaps.
fn has_past_tense_action_claim(lower: &str, action_verbs: &[String]) -> bool {
    for raw_sentence in lower.split(['.', '\n', '!']) {
        let s = raw_sentence.trim();
        if s.is_empty() || s.len() > 80 {
            continue;
        }
        for verb in action_verbs {
            if s.split_whitespace().take(4).any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                w == verb
            }) {
                return true;
            }
        }
    }
    false
}

/// Does the text contain any investigative/intent phrases?
/// Used by the phantom tool-call detector to identify when the model is
/// narrating an action it should be executing via tools.
pub fn has_investigative_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    let lang = phantom_lang::detect_language(text);
    lang_intent_match(&lower, &lang.intent_phrases)
}

/// Count line-start intent phrases — `Let me <verb>`, `I'll <verb>`,
/// `Let's <verb>`, or `Now let me / Now I'll <verb>`. A high count in a
/// single iteration's text means the model is spinning in place: emitting
/// back-to-back narration instead of calling a tool.
///
/// Only line-starts (after optional whitespace / list bullet) count. Intent
/// phrases embedded mid-paragraph are normal prose, not narration spam.
pub fn count_intent_line_starts(text: &str) -> usize {
    let lang = phantom_lang::detect_language(text);
    if lang.line_start_re.is_empty() {
        return 0;
    }
    let re = Regex::new(&lang.line_start_re).unwrap_or_else(|_| {
        Regex::new(r"$^").unwrap() // never matches
    });
    re.find_iter(text).count()
}

/// Threshold above which a single iteration's intent-phrase repetitions
/// are treated as "model stuck in a phantom loop".
pub const STUCK_INTENT_LOOP_THRESHOLD: usize = 3;

/// Convenience predicate: does the text show 3+ line-start intent
/// repetitions?
pub fn is_stuck_in_intent_loop(text: &str) -> bool {
    count_intent_line_starts(text) >= STUCK_INTENT_LOOP_THRESHOLD
}

pub fn has_phantom_tool_intent(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 40 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let lang = phantom_lang::detect_language(trimmed);

    // ── Strong signals (standalone — no corroboration needed) ─────────

    // 2+ imperative "Now <verb>" / "Let me <verb>" at line start = multi-step plan
    if !lang.now_imperative_re.is_empty()
        && let Ok(re) = Regex::new(&lang.now_imperative_re)
        && re.find_iter(&lower).count() >= 2
    {
        return true;
    }

    // 2+ numbered steps with action verbs = narrated plan
    if !lang.numbered_steps_re.is_empty()
        && let Ok(re) = Regex::new(&lang.numbered_steps_re)
        && re.find_iter(&lower).count() >= 2
    {
        return true;
    }

    // 2+ past-tense standalone sentences = phantom completion narration
    if !lang.past_tense_standalone_re.is_empty()
        && let Ok(re) = Regex::new(&lang.past_tense_standalone_re)
        && re.find_iter(&lower).count() >= 2
    {
        return true;
    }

    // ── Completion claims (standalone) ────────────────────────────────
    if lang_completion_match(&lower, &lang.completion_claims) {
        return true;
    }

    // ── Now + gerund status-then-action drops (standalone) ─────────────
    if !lang.gerund_re.is_empty()
        && let Ok(re) = Regex::new(&lang.gerund_re)
        && re.is_match(trimmed)
    {
        return true;
    }

    // ── Trailing-colon intent ─────────────────────────────────────────
    if !lang.trailing_colon_re.is_empty()
        && let Ok(re) = Regex::new(&lang.trailing_colon_re)
        && re.is_match(trimmed)
    {
        return true;
    }

    // ── Weak signals (need corroboration) ─────────────────────────────
    let has_intent = lang_intent_match(&lower, &lang.intent_phrases);

    if has_intent {
        // Corroborate with file paths, extensions, or backtick code refs
        let path_match = !lang.path_re.is_empty()
            && Regex::new(&lang.path_re)
                .map(|re| re.is_match(trimmed))
                .unwrap_or(false);
        let ext_match = !lang.ext_re.is_empty()
            && Regex::new(&lang.ext_re)
                .map(|re| re.is_match(trimmed))
                .unwrap_or(false);
        let backtick_match = !lang.backtick_code_re.is_empty()
            && Regex::new(&lang.backtick_code_re)
                .map(|re| re.is_match(trimmed))
                .unwrap_or(false);
        if path_match || ext_match || backtick_match {
            return true;
        }
    }

    false
}

// ── Language-agnostic helpers ──────────────────────────────────────────

/// Check if `lower` contains any phrase from the list (case-insensitive).
fn lang_intent_match(lower: &str, phrases: &[String]) -> bool {
    phrases.iter().any(|p| lower.contains(p.as_str()))
}

/// Check if `lower` contains any completion claim.
fn lang_completion_match(lower: &str, claims: &[String]) -> bool {
    claims.iter().any(|c| lower.contains(c.as_str()))
}

/// Slice of the text before the first code fence, markdown table row,
/// or list-item line — the "narration" portion.
fn prose_lead_in(text: &str) -> &str {
    let mut byte_offset: usize = 0;
    for (idx, line) in text.lines().enumerate() {
        let trimmed_line = line.trim_start();
        let is_structural = trimmed_line.starts_with("```")
            || (trimmed_line.starts_with('|') && trimmed_line.contains('|'))
            || trimmed_line.starts_with("- ")
            || trimmed_line.starts_with("* ")
            || trimmed_line.starts_with("• ")
            || (trimmed_line
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
                && trimmed_line.contains(". "));
        if is_structural {
            return text[..byte_offset].trim_end();
        }
        if idx >= 6 {
            break;
        }
        byte_offset += line.len() + 1;
    }
    text
}

/// Heuristic: does `text` look like it was truncated mid-sentence?
pub fn looks_truncated_mid_sentence(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.chars().count() < 40 {
        return false;
    }
    if trimmed.ends_with("```") {
        return false;
    }
    if trimmed.ends_with('|') {
        return false;
    }
    if ends_with_url(trimmed) {
        return false;
    }
    let last = match trimmed.chars().next_back() {
        Some(c) => c,
        None => return false,
    };
    if last.is_alphanumeric() {
        return true;
    }
    matches!(
        last,
        ',' | ';' | ':' | '-' | '(' | '[' | '{' | '<' | '/' | '\\' | '&' | '@' | '#'
    )
}

/// Detect whether `text` ends with a URL.
fn ends_with_url(text: &str) -> bool {
    let trimmed = text.trim_end();
    let boundary = trimmed
        .rfind(|c: char| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '<' | '"' | '\''))
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail = &trimmed[boundary..];
    tail.contains("://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_phantom_detected() {
        assert!(has_phantom_tool_intent_no_tools(
            "Let me check the logs and fix the issue."
        ));
    }

    #[test]
    fn russian_phantom_detected() {
        assert!(has_phantom_tool_intent_no_tools(
            "Давайте проверю логи и исправлю ошибку."
        ));
    }

    #[test]
    fn spanish_phantom_detected() {
        assert!(has_phantom_tool_intent_no_tools(
            "Déjame revisar el archivo y voy a actualizar la configuración ¿ok?"
        ));
    }

    #[test]
    fn portuguese_phantom_detected() {
        assert!(has_phantom_tool_intent_no_tools(
            "Vou verificar o arquivo e corrigir a configuração do irmão"
        ));
    }

    #[test]
    fn french_phantom_detected() {
        assert!(has_phantom_tool_intent_no_tools(
            "Laissez-moi vérifier le fichierête et corriger l'erreur être"
        ));
    }

    #[test]
    fn structured_answer_not_phantom() {
        // A table/list answer should never be flagged
        let table = "| Commit | Message |\n|---|---|\n| abc123 | fix stuff |\n";
        assert!(!has_phantom_tool_intent_no_tools(table));
    }

    #[test]
    fn short_text_not_phantom() {
        assert!(!has_phantom_tool_intent_no_tools("ok"));
    }

    #[test]
    fn english_completion_claim() {
        assert!(has_phantom_tool_intent(
            "I've updated the file and all changes have been applied."
        ));
    }

    #[test]
    fn english_trailing_colon() {
        assert!(has_phantom_tool_intent(
            "Let me check the logs and verify the configuration settings:"
        ));
    }

    #[test]
    fn english_intent_with_path() {
        assert!(has_phantom_tool_intent(
            "Let me update src/main.rs with the new configuration."
        ));
    }

    #[test]
    fn investigative_intent_english() {
        assert!(has_investigative_intent("Let me dig into this issue."));
    }

    #[test]
    fn stuck_in_intent_loop_english() {
        let text = "Let me check the logs\nLet me verify the config\nLet me read the file\n";
        assert!(is_stuck_in_intent_loop(text));
    }

    #[test]
    fn not_stuck_single_intent() {
        let text = "Let me check the logs and see what happened.";
        assert!(!is_stuck_in_intent_loop(text));
    }

    #[test]
    fn looks_truncated() {
        assert!(looks_truncated_mid_sentence(
            "This is a long response that got cut off in the middle of a wor"
        ));
    }

    #[test]
    fn not_truncated_with_period() {
        assert!(!looks_truncated_mid_sentence(
            "This is a complete response that ends with a period."
        ));
    }
}
