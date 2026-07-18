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
use std::sync::LazyLock;

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
    // <<react:🔥>> and sibling inline directives prefix the narration and
    // broke every ^-anchored pattern (#464: "<<react:🔥>> Pushing the 3
    // commits now." sailed through). Strip them before any matching.
    let cleaned = strip_inline_directives(text);
    let trimmed = cleaned.trim();
    let lead = prose_lead_in(trimmed);
    if lead.is_empty() {
        return false;
    }
    // Brief present-continuous work announcements ("Running checks now.",
    // "Checking the logs…") are phantom on their own — the model says it's
    // acting but emitted no tool call. At 19 bytes "Running checks now." fell
    // under the length floor below and the turn dropped with zero tools
    // (2026-06-12), so check this before the floor.
    if matches_work_announcement(lead) {
        return true;
    }
    // Leading-imminence announcement ("... Now downloading the fonts.") — the
    // work-announcement regex needs a trailing marker and misses it.
    if matches_now_gerund(lead) {
        return true;
    }
    if trimmed.len() < 20 {
        return false;
    }
    let lower = lead.to_lowercase();
    if lang_intent_match_any(&lower) {
        return true;
    }
    // Past-tense completion claims stay gated to the detected language:
    // action_verbs are short single words with real cross-language
    // collision risk, unlike the multi-word intent phrases above.
    let lang = phantom_lang::detect_language(trimmed);
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
        let words: Vec<&str> = s
            .split_whitespace()
            .take(4)
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .collect();
        for verb in action_verbs {
            if let Some(pos) = words.iter().position(|w| w == verb) {
                // Passive/copular description ("the config is loaded from
                // disk") is prose about state, not a completion claim —
                // only an ACTIVE claim ("Loaded.", "Both loaded properly")
                // counts. Guard on the auxiliary right before the verb.
                const AUX: &[&str] = &[
                    "is", "are", "was", "were", "been", "be", "being", "gets", "get", "got",
                    "está", "están", "foi", "é", "est", "sont",
                ];
                let passive = pos > 0 && AUX.contains(&words[pos - 1]);
                if !passive {
                    return true;
                }
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
    lang_intent_match_any(&lower)
}

/// Forward-looking intent detector for the post-success path.
///
/// Behaves like `has_phantom_tool_intent_no_tools` but DROPS the
/// past-tense completion-claim branch. Used as the eligibility gate
/// for phantom self-heal AFTER a turn has already produced at least
/// one successful tool call: at that point past-tense summaries
/// (`Pushed.`, `Committed.`, `On main.`) are legitimate completion
/// acks and must not re-fire the detector — that's the whole reason
/// the post-success exemption exists. But FORWARD-looking intent
/// (`Let me dig into …`, `I'll check the …`, `Let me read the …`,
/// `need to update the …`) signals more tool calls promised and
/// dropped, which IS phantom regardless of how many tools already
/// ran this turn.
///
/// Logs 2026-06-03: a turn that ran one `git branch --show-current`
/// tool call then emitted "Good, on main. Let me dig into the delete
/// invitation endpoint, the email send path, and the invite flow to
/// find the bugs." silently ended without ever dispatching the three
/// promised investigations because the original exemption gate
/// (`phantom_eligible = tools_completed == 0`) disabled phantom
/// detection entirely for the post-tool-call portion of the turn.
///
/// Uses `prose_lead_in` so structural content (tables, code blocks,
/// bullet lists) past the lead-in doesn't contribute matches —
/// matches the host detector's own filter and keeps commit-message
/// tables from re-triggering the original false positive that
/// `e843f405` fixed.
pub fn has_forward_intent_post_success(text: &str) -> bool {
    let cleaned = strip_inline_directives(text);
    let trimmed = cleaned.trim();
    if trimmed.len() < 20 {
        return false;
    }
    let lead = prose_lead_in(trimmed);
    if lead.is_empty() {
        return false;
    }
    let lower = lead.to_lowercase();
    // A present-continuous work announcement ("Pushing the 3 commits
    // now.") is inherently FORWARD-looking — it can never be a completion
    // ack, no matter how many tools already ran this turn. The gate used
    // to check intent phrases only, so announcements after a successful
    // tool call closed turns as fake completions (#464). `matches_now_gerund`
    // adds the leading-imminence form ("... Now downloading the fonts.") that
    // the trailing-marker work-announcement regex misses.
    lang_intent_match_any(&lower) || matches_work_announcement(lead) || matches_now_gerund(lead)
}

/// Remove inline channel directives (`<<react:🔥>>`, `<<IMG:path>>`, …)
/// so anchored phantom patterns see the narration, not the marker (#464).
fn strip_inline_directives(text: &str) -> String {
    static DIRECTIVE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<<[^<>\n]{1,120}>>").expect("directive regex"));
    DIRECTIVE_RE.replace_all(text, "").into_owned()
}

/// Language-agnostic phantom tell (#463): the text names a REAL registered
/// tool while the turn executed zero tool calls. Models hallucinate tool
/// usage by naming the tool ("loaded via load_brain_file"), in ANY language,
/// so this catches narration the phrase lists cannot. Only multi-word
/// (underscore) tool names count: bare names like "bash" or "plan" are
/// ordinary prose words and would false-positive constantly.
pub fn mentions_registered_tool(text: &str, tool_names: &[String]) -> bool {
    let lower = text.to_lowercase();
    for name in tool_names {
        if !name.contains('_') {
            continue;
        }
        let mut from = 0;
        while let Some(rel) = lower[from..].find(name.as_str()) {
            let start = from + rel;
            let end = start + name.len();
            let before_ok = start == 0
                || !lower[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let after_ok = end == lower.len()
                || !lower[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if before_ok && after_ok {
                return true;
            }
            from = end;
        }
    }
    false
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

/// Threshold above which a repeated intent line is treated as "model stuck in
/// a phantom loop".
pub const STUCK_INTENT_LOOP_THRESHOLD: usize = 3;

/// The highest number of times the SAME intent line-start appears (normalized:
/// trimmed, lowercased, whitespace collapsed). A genuine phantom loop repeats
/// the *same* line ("Let me check the file." over and over); a legitimate
/// multi-step plan ("check X… then Y… actually Z first") has many DISTINCT
/// intent lines, which must NOT be mistaken for a loop.
pub fn max_repeated_intent_line(text: &str) -> usize {
    let lang = phantom_lang::detect_language(text);
    if lang.line_start_re.is_empty() {
        return 0;
    }
    let Ok(re) = Regex::new(&lang.line_start_re) else {
        return 0;
    };
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut max = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Only lines that START with an intent phrase count.
        if re.find(trimmed).map(|m| m.start() == 0).unwrap_or(false) {
            let norm = trimmed
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let c = counts.entry(norm).or_insert(0);
            *c += 1;
            max = max.max(*c);
        }
    }
    max
}

/// Whether the text shows a genuine phantom loop: the SAME intent line repeated
/// `STUCK_INTENT_LOOP_THRESHOLD`+ times. Distinct intent lines (a varied plan)
/// are NOT a loop — that false positive used to kill legitimate planful replies.
pub fn is_stuck_in_intent_loop(text: &str) -> bool {
    max_repeated_intent_line(text) >= STUCK_INTENT_LOOP_THRESHOLD
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

/// Check if `lower` matches an intent phrase in ANY supported language.
///
/// `detect_language` only routes Cyrillic and accented-Latin text
/// reliably, so accent-free non-English narration (e.g.
/// `"Voy a usar write_file…"`) falls through to English and would slip
/// past a detected-language-only check. Intent phrases are multi-word and
/// carry language-distinctive tokens, so the cross-language union is
/// collision-free — a Spanish phrase can't match English prose and vice
/// versa. 2026-06-12.
fn lang_intent_match_any(lower: &str) -> bool {
    phantom_lang::all_langs()
        .iter()
        .any(|lang| lang_intent_match(lower, &lang.intent_phrases))
}

/// Does `lead` OPEN with a present-continuous work announcement in ANY
/// supported language ("Running checks now.", "Verificando ahora…")? Scanned
/// across all languages like the intent phrases. Each regex is anchored to the
/// message start and requires the announcement's imminence marker (now / ahora
/// / agora / maintenant / сейчас / … / trailing :) at a sentence boundary, so
/// the model leading with the announcement and then continuing ("Running fmt,
/// clippy, tests now. Then fetching…") still matches, while an ordinary
/// sentence that merely opens with a gerund ("Reading the file is
/// straightforward.") or uses "now" as an adverb ("Running it now takes a
/// minute.") does not.
pub(crate) fn matches_work_announcement(lead: &str) -> bool {
    let lead = strip_inline_directives(lead);
    let lead = lead.trim();
    phantom_lang::all_langs().iter().any(|lang| {
        !lang.work_announcement_re.is_empty()
            && Regex::new(&lang.work_announcement_re)
                .map(|re| announcement_matches_anywhere(&re, lead))
                .unwrap_or(false)
    })
}

/// Run an anchored announcement regex against every sentence start in the
/// lead, tolerating a short lead clause before the gerund. The live escapes
/// (#464) were all anchor evasions: "Internet's back, pushing now." (clause
/// prefix), "Apologies Adolfo, you're right. Pushing now." (sentence
/// prefix). Suffix slices keep the terminal imminence markers intact.
fn announcement_matches_anywhere(re: &Regex, lead: &str) -> bool {
    let mut starts: Vec<usize> = vec![0];
    let mut after_ender = false;
    for (idx, ch) in lead.char_indices() {
        if after_ender && !ch.is_whitespace() {
            starts.push(idx);
            after_ender = false;
        }
        if matches!(ch, '.' | '!' | '?' | '\n' | '…') {
            after_ender = true;
        }
    }
    for &start in &starts {
        let suffix = &lead[start..];
        if re.is_match(suffix) {
            return true;
        }
        // Short lead clause before the announcement ("internet's back, ").
        let window_end = suffix
            .char_indices()
            .take_while(|(i, _)| *i <= 48)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        if let Some(comma) = suffix[..window_end].find(", ")
            && re.is_match(&suffix[comma + 2..])
        {
            return true;
        }
    }
    false
}

/// Does `text` contain a "Now &lt;gerund&gt;" work announcement at a sentence
/// start in ANY supported language (`gerund_re`)? This catches the
/// leading-imminence form ("Issue #22 filed. Now downloading the fonts.") that
/// `matches_work_announcement` misses — that regex requires a TRAILING imminence
/// marker (now / … / :), so an announcement that leads with "Now" and ends on a
/// plain period slips it. Scanned across all languages like the other tells.
pub(crate) fn matches_now_gerund(text: &str) -> bool {
    let text = strip_inline_directives(text);
    phantom_lang::all_langs().iter().any(|lang| {
        !lang.gerund_re.is_empty()
            && Regex::new(&lang.gerund_re)
                .map(|re| re.is_match(&text))
                .unwrap_or(false)
    })
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

/// Does the user message contain an analysis / data-interpretation verb?
///
/// Used to detect "the user asked me to AUDIT something" vs. "the user
/// asked me to COMMIT something" so the runtime can react when a turn
/// ends with `finish_reason: stop` and ZERO text after successful tool
/// calls. For side-effect tasks (commit / push / edit / deploy), the
/// tool call IS the deliverable — empty-text completion is fine. For
/// analysis tasks, the tool fetched data the user expected the model
/// to interpret — empty-text completion is a regression we shipped via
/// the `FINISHING A TURN` directive in commit e843f405.
///
/// Matches at a word boundary so prose like "you describe this
/// pattern" does NOT trip on "describe" inside another sentence. Only
/// the leading-imperative / question form counts.
///
/// Coverage is intentionally English-only for now. Spanish / Portuguese
/// / French / Russian variants follow the same shape; this MVP catches
/// the common case and can be expanded as patterns emerge in logs.
pub fn is_analysis_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Strip the channel prefix if present so `[Channel: Telegram ...]\n<msg>`
    // matches on `<msg>` content, not on the bracketed wrapper.
    let body = lower.rsplit('\n').next().unwrap_or(&lower);
    // Look at the first ~200 chars only — the verb is in the request,
    // not buried in a long quote.
    let head: String = body.chars().take(200).collect();
    // Phrase patterns to match. Each entry is matched as a contained
    // substring on the head — short verbs need leading whitespace or
    // start-of-string to avoid matching inside another word
    // ("examine" should not trigger on "exam"; "audit" must not
    // trigger on "auditorium" in a quoted URL).
    let leading_word = |w: &str| -> bool {
        // Match at start or after whitespace/punct, followed by space.
        // Cheap manual scan rather than a regex — keeps this hot path
        // allocation-free for the common no-match case.
        let needle = format!(" {w} ");
        if head.starts_with(&format!("{w} ")) {
            return true;
        }
        head.contains(&needle)
    };
    const ANALYSIS_VERBS: &[&str] = &[
        "audit",
        "review",
        "compare",
        "explain",
        "summarise",
        "summarize",
        "check",
        "describe",
        "analyse",
        "analyze",
        "find",
        "look up",
        "look at",
        "what does",
        "how does",
        "why does",
        "what is",
        "what are",
        "tell me",
        "show me",
        "investigate",
        "diagnose",
    ];
    // "report" deliberately omitted — too noun-ambiguous. "the report
    // says X" and "your report failed" would false-positive the
    // analysis-nudge while no analysis was requested. `report on X`
    // is rare enough that users who want it can rephrase as "explain
    // X" or "summarise X" without losing precision.
    ANALYSIS_VERBS.iter().any(|v| leading_word(v))
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
