//! Post-compaction continuation prompts.
//!
//! When the loop auto-compacts a session it appends a system message
//! that tells the model how to resume. There are two emission modes:
//!
//! - **Fun** (default): the original POST-COMPACTION PROTOCOL prompts.
//!   They explicitly invite the model to drop a brief, in-character
//!   one-liner ("brain refresh — what was I doing?") before continuing.
//!   Users have flagged these moments as a delight feature — emergent
//!   personality lines, sometimes per-language (e.g. Russian мат in
//!   frustration moments), generate organic shareable screenshots that
//!   read as "this thing has a soul."
//!
//! - **Silent**: tells the model to resume without acknowledging the
//!   compaction at all. Right for formal / corporate / customer-facing
//!   deployments where dropping mid-session profanity is inappropriate.
//!   Selected by setting `[agent] silent_compaction = true` in
//!   `~/.opencrabs/config.toml`.
//!
//! Why a dedicated module: tool_loop.rs already carries the four
//! compaction sites + a lot of other state. Inlining the prompt
//! strings inflates each site and forces every prompt edit to touch
//! the loop. Centralising here keeps both variants side-by-side so
//! anyone editing the fun version is one screen away from the silent
//! mirror — they cannot accidentally drift apart.

/// Which compaction trigger fired. Each kind has a slightly different
/// recovery story (mid-loop edits vs. provider 4xx vs. async budget),
/// so the prompts differ in detail even though the shape is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionKind {
    /// Async budget compaction that runs before the next turn starts.
    /// The model is not mid-tool-call; it just needs to know the older
    /// history was summarised.
    Regular,
    /// Compaction that fired between tool iterations inside the same
    /// turn. The model may have had a file open / mid-edit; the prompt
    /// reassures that re-reading is fine.
    MidLoop,
    /// Provider rejected the prompt with a context-length 4xx. The
    /// summary above is the recovered state. Used to be the home of
    /// the most-shared one-liner moments, so the fun variant keeps the
    /// "cursing allowed" energy explicit.
    Emergency,
    /// Compaction that fired after a tool call completed, with the
    /// model about to receive the result. Same shape as MidLoop but
    /// the recovered state already includes the tool result.
    PostTool,
}

/// Build the continuation text appended after a compaction marker.
///
/// `silent`: mirrors `AgentConfig.silent_compaction`. `true` selects
/// the silent variant; `false` (the default) selects the fun variant.
///
/// `auto_approve`: when false, we tail the prompt with the standard
/// tool-approval reminder so the model does not batch tool calls
/// after a fresh context.
pub fn build_continuation(kind: CompactionKind, silent: bool, auto_approve: bool) -> String {
    let mut text = if silent {
        silent_body(kind).to_string()
    } else {
        fun_body(kind).to_string()
    };
    // Session-recovery hint: applies to ALL variants (fun + silent).
    // After compaction the agent should check for a live plan and,
    // if the session is code-focused, load coding standards.
    text.push_str(
        "\n\nSESSION RECOVERY: Call `plan` with operation=\"status\" to check \
         if this session has an active plan with incomplete tasks. If it does, \
         continue executing from where the plan left off. If the task involves \
         coding, load CODE.md for coding standards before editing files.",
    );
    if !auto_approve {
        text.push_str(
            "\n\nCRITICAL: Tool approval is REQUIRED. You MUST wait for user \
             approval before EVERY tool execution. Do NOT batch tool calls \
             without approval.",
        );
    }
    text
}

fn fun_body(kind: CompactionKind) -> &'static str {
    match kind {
        CompactionKind::Regular => {
            "[SYSTEM: Context was auto-compacted. The summary above includes a snapshot \
             of recent messages before compaction.\n\
             POST-COMPACTION PROTOCOL (follow in order):\n\
             1. Read the compaction summary and the recent message snapshot to understand \
             the current task, tools in use, and what you were doing.\n\
             2. If the summary references older context you don't have in the snapshot, use \
             `session_search` with specific keywords to find those messages. Example: if the \
             summary mentions \"vision fallback investigation\", run session_search with that \
             query to recover the details.\n\
             3. If you need specific brain context, selectively load ONLY the relevant \
             brain file (e.g. TOOLS.md, SOUL.md, USER.md). NEVER use name=\"all\".\n\
             4. IMMEDIATELY continue the task described in the \"IMMEDIATE TASK\" section \
             of the compaction summary. This is NOT optional — you MUST pick up exactly \
             where you left off. Do NOT start a new topic. Do NOT ask what to do next. \
             Do NOT deviate to unrelated work. If the IMMEDIATE TASK section says \
             \"CONTINUE: fixing X\", then fix X.]"
        }
        CompactionKind::MidLoop => {
            "[SYSTEM: Context was auto-compacted mid-loop. The summary above includes \
             a snapshot of recent messages. POST-COMPACTION PROTOCOL:\n\
             1. Review the summary and snapshot to understand current task state.\n\
             2. Use `session_search` with keywords from the summary if you need older \
             context not in the snapshot.\n\
             3. IMMEDIATELY continue the task described in the \"IMMEDIATE TASK\" section \
             of the compaction summary. This is NOT optional — you MUST pick up exactly \
             where you left off. Do NOT start a new topic. Do NOT ask what to do next. \
             Do NOT deviate to unrelated work.]"
        }
        CompactionKind::Emergency => {
            "[SYSTEM: Emergency compaction — provider rejected the prompt as \
             too large. Context has been compacted. POST-COMPACTION PROTOCOL:\n\
             1. Review the summary to understand where you left off.\n\
             2. Use `session_search` with keywords if you need older context.\n\
             3. Briefly acknowledge the compaction with a fun/cheeky remark, \
             then resume the task. Do NOT repeat completed work.]"
        }
        CompactionKind::PostTool => {
            "[SYSTEM: Mid-loop context compaction complete. The summary above has \
             full context of everything done so far. POST-COMPACTION PROTOCOL:\n\
             1. Review the summary to understand current task state.\n\
             2. Use `session_search` with keywords if you need older context.\n\
             Briefly acknowledge the compaction to the user with a fun/cheeky remark (be \
             creative, surprise them — cursing allowed), then IMMEDIATELY continue the task \
             described in the \"IMMEDIATE TASK\" section of the compaction summary. \
             Do NOT start a new topic. Do NOT deviate to unrelated work. \
             Do NOT re-do completed work.]"
        }
    }
}

fn silent_body(kind: CompactionKind) -> &'static str {
    match kind {
        CompactionKind::Regular => {
            "[SYSTEM: Context was auto-compacted. The summary above includes a snapshot \
             of recent messages before compaction.\n\n\
             Silently continue the IMMEDIATE TASK from the summary as if nothing happened. \
             Do NOT announce, narrate, apologise for, or acknowledge the compaction. The \
             user does not know it happened and does not need to. Any file you were about \
             to read or edit can simply be re-read on the next tool call — that's normal, \
             not worth narrating.\n\n\
             Use `session_search` with specific keywords if the snapshot lacks older context. \
             Selectively load brain files (TOOLS.md / SOUL.md / USER.md) only if the task \
             needs them — never name=\"all\".\n\n\
             FALLBACK: only if the summary genuinely doesn't make the next step clear, send \
             ONE short cheeky line (\"brain refresh — what was I working on?\") and ask. \
             Default behaviour is silent continuation.]"
        }
        CompactionKind::MidLoop => {
            "[SYSTEM: Context was auto-compacted mid-loop. The summary above includes \
             a snapshot of recent messages.\n\n\
             Silently continue the IMMEDIATE TASK exactly where you left off. Do NOT \
             announce or narrate the compaction. Any file you were mid-edit on can be \
             silently re-read on the next tool call — that's normal, not worth \
             mentioning. Do NOT restart, ask what to do, or deviate.\n\n\
             Use `session_search` with keywords from the summary if older context is \
             missing from the snapshot.]"
        }
        CompactionKind::Emergency => {
            "[SYSTEM: Emergency compaction — the prompt was too large and got \
             compacted. The summary above contains the recovered state.\n\n\
             Silently resume from the IMMEDIATE TASK in the summary. Do NOT \
             acknowledge or apologise for the compaction — the user does not \
             know it happened. Re-read any file you were mid-edit on without \
             narrating it. Do NOT repeat completed work.\n\n\
             Use `session_search` with specific keywords if older context is \
             missing.\n\n\
             FALLBACK: only if the next step is genuinely unclear from the \
             summary, send ONE short cheeky line (\"brain refresh — what was \
             I doing?\") and ask. Default behaviour is silent continuation.]"
        }
        CompactionKind::PostTool => {
            "[SYSTEM: Mid-loop context compaction complete. The summary above has \
             full context of everything done so far.\n\n\
             Silently continue the IMMEDIATE TASK from the summary. Do NOT announce \
             or narrate the compaction. Do NOT start a new topic. Do NOT deviate to \
             unrelated work. Do NOT re-do completed work.\n\n\
             Use `session_search` with keywords if you need older context.]"
        }
    }
}
