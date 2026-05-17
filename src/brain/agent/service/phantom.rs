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

/// Detect "phantom tool calls" — the model narrates actions it claims to
/// have performed but never actually executed any tool calls.
///
/// Returns `true` when the response text contains strong action-intent
/// signals (modification verbs + file-path-like strings) suggesting the
/// model believed it was making changes. The caller should inject a retry
/// prompt so the model actually executes the tool calls on the next turn.
///
/// Deliberately conservative: requires BOTH an action verb AND a file
/// path pattern to avoid false-positives on conversational responses.
/// Shared intent phrases used by both the strict and relaxed phantom
/// detectors. Action verbs + read/inspection verbs + "I'll proceed"
/// variants. Lowered-cased match.
const INTENT_PHRASES: &[&str] = &[
    "now let me ",
    "now update ",
    "now fix ",
    "now add ",
    "now bump ",
    "now run ",
    "now check ",
    "now read ",
    "now commit",
    "now amend",
    // "Now + gerund" status-then-action drops: model reports what it did,
    // then says "Now cherry-picking/updating/fixing..." and stops with
    // zero tool calls. Seen: "Now cherry-picking to main and prod." and
    // (2026-05-05 14:47 incident on a 'slack'-named TUI session) "Now
    // creating the new tests file for signin/invitation error messages."
    // The model emitted that line, zero tool_use blocks followed, and
    // the phantom detector missed it because "now creating" wasn't in
    // INTENT_PHRASES — covered the git/deploy verbs but not the file-
    // operation gerunds. Add the full file-operation set.
    "now updating",
    "now fixing",
    "now committing",
    "now amending",
    "now pushing",
    "now cherry-picking",
    "now merging",
    "now rebasing",
    "now deploying",
    "now building",
    "now testing",
    "now checking",
    "now applying",
    "now restarting",
    "now creating",
    "now writing",
    "now editing",
    "now adding",
    "now removing",
    "now deleting",
    "now reading",
    "now running",
    "now starting",
    "now finishing",
    "now finalizing",
    "now installing",
    "now configuring",
    "now wiring",
    "now setting up",
    "i'll update",
    "i'll fix",
    "i'll modify",
    "i'll create",
    "i'll write",
    "i'll edit",
    "i'll add",
    "i'll change",
    "i'll replace",
    "i'll commit",
    "i'll amend",
    "i'll proceed",
    "i'll start",
    "i'll finish",
    "i'll run",
    "i'll check",
    "i'll see",
    "i'll look",
    "i'll prepare",
    "i'll take a look",
    "i will proceed",
    "let me update",
    "let me fix",
    "let me modify",
    "let me create",
    "let me write",
    "let me edit",
    "let me add",
    "let me change",
    "let me commit",
    "let me amend",
    "let me see",
    "let me check",
    "let me look",
    "let me read",
    "let me examine",
    "let me verify",
    "let me inspect",
    "let me review",
    "let me take",     // "let me take a look"
    "let me actually", // "let me actually look at the commits"
    "let me prepare",
    "let me proceed",
    "let me start",
    "let me first", // "let me first see where we stand"
    "let me finish",
    "let me finalize",
    "let me run",
    // "Let's" contraction variants — models frequently use "let's" instead
    // of "let me". These were completely missing and caused phantom misses
    // like "Let's check the actual paste flow" (2026-04-23).
    "let's update",
    "let's fix",
    "let's modify",
    "let's create",
    "let's write",
    "let's edit",
    "let's add",
    "let's change",
    "let's replace",
    "let's commit",
    "let's amend",
    "let's see",
    "let's check",
    "let's look",
    "let's read",
    "let's examine",
    "let's verify",
    "let's inspect",
    "let's review",
    "let's take a look",
    "let's prepare",
    "let's proceed",
    "let's start",
    "let's first",
    "let's finish",
    "let's finalize",
    "let's run",
    "let's dig",
    "let's investigate",
    "let's explore",
    "let's search",
    "let's find",
    "let's gather",
    "let's pull",
    "let's grab",
    "let's get",
    "let's fetch",
    "let's query",
    "let's scan",
    "let's hunt",
    "let's trace",
    "let's track",
    "let's look into",
    "let's check into",
    "let's find out",
    "let's dig into",
    // Investigative intents: model commits to research + never executes.
    // Seen in logs 2026-04-17 14:23 — response was literally "Let me dig
    // into this." (21 chars) for an "investigate opencode oauth" prompt,
    // and no tool call followed. Must phantom-retry these.
    "let me dig",
    "let me investigate",
    "let me explore",
    "let me search",
    "let me find",
    "let me gather",
    "let me pull",
    "let me grab",
    "let me get",
    "let me fetch",
    "let me query",
    "let me scan",
    "let me hunt",
    "let me trace",
    "let me track",
    "let me look into",
    "let me check into",
    "let me find out",
    "let me dig into",
    "i'll dig",
    "i'll investigate",
    "i'll explore",
    "i'll search",
    "i'll find",
    "i'll gather",
    "i'll pull",
    "i'll grab",
    "i'll get",
    "i'll fetch",
    "i'll query",
    "i'll scan",
    "i'll hunt",
    "i'll trace",
    "i'll track",
    "i'll look into",
    "i'll check into",
    "i'll find out",
    "i'll dig into",
    // Build / deploy / migration verbs — observed mid-2026-05 in a
    // transcript where an agent narrated "Let me check the schema… Let me
    // create the migration… Let me build and push" across nine paragraphs,
    // emitted zero tool calls, and signed off with "Pushed." The earlier
    // intents would have caught it, but bare cases ("Let me build and
    // push:" as the entire response) need their own coverage.
    "let me build",
    "let me push",
    "let me deploy",
    "let me sync",
    "let me migrate",
    "let me apply",
    "let me install",
    "let me configure",
    "let me set up",
    "let me wire",
    "let's build",
    "let's push",
    "let's deploy",
    "let's sync",
    "let's migrate",
    "let's apply",
    "let's install",
    "let's configure",
    "let's set up",
    "let's wire",
    "i'll build",
    "i'll push",
    "i'll deploy",
    "i'll sync",
    "i'll migrate",
    "i'll apply",
    "i'll install",
    "i'll configure",
    "i'll set up",
    "i'll wire",
    "now build",
    "now push",
    "now deploy",
    "now sync",
    "now migrate",
    "now apply",
];

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
    // Check the PROSE LEAD-IN — everything before the first structural
    // boundary (code fence, markdown table, numbered/bulleted list item).
    // A legitimate "put these commits in a table" answer starts with the
    // table itself; a phantom "Let me check the logs" followed by a
    // markdown bash block starts with intent narration. Scoping the
    // intent scan to the lead-in prevents both false positives (commit
    // messages quoted inside tables) and false negatives (model writes
    // `Let me check:\n\`\`\`bash...\`\`\`` instead of calling the tool).
    let lead = prose_lead_in(trimmed);
    if lead.is_empty() {
        // No prose before the structural content — not a phantom.
        return false;
    }
    let lower = lead.to_lowercase();
    if INTENT_PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // Past-tense completion claim with zero tool calls: the model wrote
    // "Pushed." / "Deployed." / "Merged." as a terminal sentence to wrap
    // up a series of supposed actions. Conversational past-tense ("I
    // pushed yesterday") is fine; the give-away is the claim standing
    // alone as a short statement *and* the iteration produced no tool
    // uses. Caller already gates on zero tools, so we just look for the
    // claim in the lead-in.
    has_past_tense_action_claim(&lower)
}

/// Detects short past-tense completion claims like `"Pushed."`, `"Deployed."`,
/// `"Migration created."` — sentences that announce an action's done without
/// having executed any tool. Only used in the zero-tool-call path; loose
/// matching elsewhere would false-positive on conversational recaps.
fn has_past_tense_action_claim(lower: &str) -> bool {
    // Look at sentences (period/newline-terminated) that are short enough
    // to be summary claims. "Pushed." or "Done — three migrations added."
    // qualify; a 200-char sentence describing what someone *might* have
    // pushed does not.
    const ACTION_VERBS: &[&str] = &[
        "pushed",
        "deployed",
        "merged",
        "migrated",
        "committed",
        "rebased",
        "tagged",
        "released",
        "published",
        "synced",
        "rolled back",
        "rolled out",
    ];
    for raw_sentence in lower.split(['.', '\n', '!']) {
        let s = raw_sentence.trim();
        if s.is_empty() || s.len() > 80 {
            continue;
        }
        for verb in ACTION_VERBS {
            // Match the verb as a leading word: "pushed", "pushed.",
            // "pushed three migrations", "all pushed". Avoid matching
            // inside another word ("crushed", "ambushed"). Allow up to
            // ~3 leading filler words ("done — pushed", "now pushed").
            if s.split_whitespace().take(4).any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                w == *verb
            }) {
                return true;
            }
        }
    }
    false
}

/// Does the text contain any investigative/intent phrases from `INTENT_PHRASES`?
/// Used by the phantom tool-call detector to identify when the model is
/// narrating an action it should be executing via tools.
pub fn has_investigative_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    INTENT_PHRASES.iter().any(|p| lower.contains(p))
}

/// Count line-start intent phrases — `Let me <verb>`, `I'll <verb>`,
/// `Let's <verb>`, or `Now let me / Now I'll <verb>`. A high count in a
/// single iteration's text means the model is spinning in place: emitting
/// back-to-back narration instead of calling a tool.
///
/// Seen 2026-05-16 on `qwen-3.6-max-preview-thinking` via the `dialagram`
/// custom provider: nine consecutive `Let me fetch issue #81 …` /
/// `Let me pull up the issue …` paragraphs in one streamed response,
/// zero tool calls. Nudging that pattern with the standard retry prompt
/// just produced another batch of intent narration; the model is stuck.
///
/// Only line-starts (optionally preceded by list bullets) count. Intent
/// phrases embedded mid-paragraph are normal prose, not narration spam.
/// `Now <verb>` alone is intentionally excluded — `Now check the logs`
/// is one signal among many, but `Now is the time` would false-match.
pub fn count_intent_line_starts(text: &str) -> usize {
    use regex::Regex;
    use std::sync::LazyLock;
    // Line-start (after optional whitespace / list bullet) followed by an
    // intent opener, whitespace, and the next verb's leading character.
    // Both ASCII `'` and curly `’` apostrophes accepted — markdown
    // round-trips frequently convert one to the other.
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?im)^[\s\-*•>]*(?:(?:ok|okay|sure|alright|right|hmm|well|actually|so)\s*,?\s+)?(?:now\s+)?(?:let\s+me|i['’]ll|let['’]s)\s+\w",
        )
        .unwrap()
    });
    RE.find_iter(text).count()
}

/// Threshold above which a single iteration's intent-phrase repetitions
/// are treated as "model stuck in a phantom loop". 3+ separate `Let me X`
/// / `I'll X` line-starts in one response is the signature: a working
/// model would either call a tool or write prose, not narrate the same
/// imminent action three times.
pub const STUCK_INTENT_LOOP_THRESHOLD: usize = 3;

/// Convenience predicate: does the text show 3+ line-start intent
/// repetitions? Caller should treat true as "abort the turn" — retrying
/// rarely recovers this pattern and just leaks more text to the TUI.
pub fn is_stuck_in_intent_loop(text: &str) -> bool {
    count_intent_line_starts(text) >= STUCK_INTENT_LOOP_THRESHOLD
}

/// Slice of the text before the first code fence, markdown table row,
/// or list-item line — the "narration" portion. If the text starts
/// directly with structural content, returns an empty string.
fn prose_lead_in(text: &str) -> &str {
    // Check the first ~6 lines for a structural boundary.
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
        // Cap the lead-in so we don't spend forever scanning a pure-prose
        // response for structural content that doesn't exist — anything
        // beyond 6 lines of prose is clearly not a "lead-in to a
        // structured answer", just free-form narration.
        if idx >= 6 {
            break;
        }
        byte_offset += line.len() + 1; // +1 for the \n
    }
    text
}

/// Heuristic: does `text` look like it was truncated mid-sentence?
///
/// Local reasoning models occasionally hit an internal EOS token
/// mid-word ("Standard Get I", "…1,890"). The stream ends cleanly
/// with finish_reason=stop + usage, so we can't use protocol signals
/// — we have to look at the content. We call it "complete" when the
/// trailing non-whitespace character is a terminal punctuation mark,
/// a close-bracket / close-quote, a table pipe, or a code-fence
/// closer. Anything else (letter, digit, open-punctuation, trailing
/// comma or colon) counts as mid-sentence.
///
/// Returns `false` for very short texts so we don't retry one-word
/// replies ("yes", "ok").
pub fn looks_truncated_mid_sentence(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.chars().count() < 40 {
        return false;
    }
    // Fenced code block ending with ``` counts as complete.
    if trimmed.ends_with("```") {
        return false;
    }
    // Markdown table row ending with `|` is a complete row.
    if trimmed.ends_with('|') {
        return false;
    }
    // URL-terminated response: a message ending with a URL is
    // complete, even though the URL's last char is alphanumeric or
    // a path separator. Previous heuristic treated "Done. Uploaded
    // to Drive: https://…/view" as truncated and triggered a
    // retry, which made the model restate the whole answer — the
    // duplication visible on Telegram AND TUI (2026-04-18 23:12/23:13
    // log: Block 0 "Done. Uploaded to Drive …" emitted in iteration
    // N and again in iteration N+1).
    if ends_with_url(trimmed) {
        return false;
    }
    let last = match trimmed.chars().next_back() {
        Some(c) => c,
        None => return false,
    };
    // Mid-word / mid-phrase: letters, digits, and tokens that signal
    // "more is coming" (opening punctuation, trailing comma/colon, etc).
    if last.is_alphanumeric() {
        return true;
    }
    matches!(
        last,
        ',' | ';' | ':' | '-' | '(' | '[' | '{' | '<' | '/' | '\\' | '&' | '@' | '#'
    )
}

/// Detect whether `text` ends with a URL. Scans back from the end for
/// whitespace or an open-paren/bracket boundary, then checks if the
/// trailing token contains "://". Covers http(s), ftp, file, and any
/// other scheme the model might emit.
fn ends_with_url(text: &str) -> bool {
    let trimmed = text.trim_end();
    let boundary = trimmed
        .rfind(|c: char| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '<' | '"' | '\''))
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail = &trimmed[boundary..];
    tail.contains("://")
}

pub fn has_phantom_tool_intent(text: &str) -> bool {
    let trimmed = text.trim();
    // Short responses are usually direct answers, not phantom narrations
    if trimmed.len() < 40 {
        return false;
    }
    let lower = trimmed.to_lowercase();

    // ── Strong signals (standalone — no corroboration needed) ─────────

    use regex::Regex;

    // 2+ imperative "Now <verb>" / "Let me <verb>" at line start = multi-step plan
    let now_imperative =
        Regex::new(r"(?m)^[\s\-*]*(?:now\s+(?:let\s+me\s+)?|let\s+me\s+)\w").unwrap();
    if now_imperative.find_iter(&lower).count() >= 2 {
        return true;
    }

    // 2+ numbered steps with action verbs = narrated plan
    let numbered_steps =
        Regex::new(r"(?m)^\s*\d+\.\s+(?:update|fix|modify|create|write|edit|add|change|remove|delete|check|read|run|bump|amend|verify|test|deploy|install)")
            .unwrap();
    if numbered_steps.find_iter(&lower).count() >= 2 {
        return true;
    }

    // 2+ past-tense standalone sentences = phantom completion narration
    let past_tense_standalone = Regex::new(
        r"(?m)^[\s\-*]*(?:amended|updated|fixed|modified|created|written|saved|deleted|removed|replaced|bumped|deployed|committed)[.!]"
    ).unwrap();
    if past_tense_standalone.find_iter(&lower).count() >= 2 {
        return true;
    }

    // ── Completion claims (standalone — model claims it finished work) ─
    // These are strong because a text-only response saying "I've updated
    // the file" with zero tool calls is always phantom.
    const COMPLETION_CLAIMS: &[&str] = &[
        "here's what changed",
        "here's what's changed",
        "here are the changes",
        "here's what i did",
        "here is what i did",
        "changes applied",
        "updated the file",
        "updated the code",
        "updated src/",
        "modified the file",
        "modified src/",
        "fixed the file",
        "fixed the bug",
        "fixed the issue",
        "fixed src/",
        "created the file",
        "wrote the file",
        "everything is updated",
        "i've made the changes",
        "i've completed",
        "i've finished",
        "i've updated",
        "i've written",
        "i've created",
        "i've saved",
        "i've modified",
        "i've fixed",
        "i've replaced",
        "i've amended",
        "i've committed",
        "i've bumped",
        "i've made all",
        "all changes have been",
        "all files have been",
        "the changes have been applied",
        "changes are now in place",
        "the file now contains",
        "the file has been",
        "file updated",
        "file created",
        "file saved",
        "changes saved",
        // Git-specific phantom claims
        "amended.",
        "committed.",
        "amended the commit",
        "bumped the version",
        "version bumped",
    ];
    if COMPLETION_CLAIMS.iter().any(|c| lower.contains(c)) {
        return true;
    }

    // ── Now + gerund status-then-action drops (standalone) ─────────────
    // Model reports status then announces a gerund action and drops:
    // "Fix committed. Now cherry-picking to main and prod." — inherently
    // phantom because the status report proves work was done but the
    // announced next action never executes. No corroboration needed.
    // Must appear at a sentence boundary (start of text or after .!?)
    // to avoid false positives like "Are you now checking the logs?"
    let now_gerund_re = Regex::new(
        r"(?im)(?:^|[.!?]\s+)\s*now\s+(?:updating|fixing|committing|amending|pushing|cherry-picking|merging|rebasing|deploying|building|testing|checking|applying|restarting|creating|writing|editing|adding|removing|deleting|reading|running|starting|finishing|finalizing|installing|configuring|wiring)\b"
    ).unwrap();
    if now_gerund_re.is_match(trimmed) {
        return true;
    }

    // ── Weak signals (need corroboration) ─────────────────────────────
    // A single "let me check" or "I'll look" is normal conversation.
    // Only flag as phantom if ALSO accompanied by file-path-like patterns,
    // meaning the model is narrating specific file operations it should
    // be executing via tools.

    let has_intent = INTENT_PHRASES.iter().any(|v| lower.contains(v));

    // Trailing-colon "Let me X:" at end of response is a strong signal all
    // on its own — the model set up an action then emitted nothing after.
    // No path corroboration needed: the colon announces a follow-up that
    // never came.
    let trailing_colon_intent = Regex::new(
        r"(?im)(?:^|\n)\s*(?:let\s+me|i'll|i\s+will|now\s+let\s+me|now\s+i'll)\s+\w[^:\n]{0,80}:\s*$",
    )
    .unwrap();
    if trailing_colon_intent.is_match(trimmed) {
        return true;
    }

    if has_intent {
        // Corroborate: does the text reference file paths or code identifiers?
        // e.g. src/foo/bar.rs, ./config.toml, Cargo.toml, `some_function`
        let path_re =
            Regex::new(r"(?:^|[\s`(])(?:\./)?[a-zA-Z_][\w\-]*/[\w\-/]*\.\w{1,6}(?:[\s`),:;]|$)")
                .unwrap();
        let ext_re = Regex::new(
            r"(?:^|[\s`(])[\w\-]+\.(?:rs|py|ts|tsx|js|jsx|go|sh|toml|yaml|yml|json|md)(?:[\s`),:;]|$)",
        )
        .unwrap();
        // Backtick code references like `auth_invalidate_fn` or `MyStruct`
        let backtick_code_re = Regex::new(r"`[a-zA-Z_]\w+`").unwrap();
        if path_re.is_match(trimmed)
            || ext_re.is_match(trimmed)
            || backtick_code_re.is_match(trimmed)
        {
            return true;
        }
    }

    false
}
