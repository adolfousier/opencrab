//! Shared prompt/warning builders for the `/goal` slash command.
//!
//! Both the TUI dispatch (`tui/app/messaging.rs`) and the channel command
//! handler (`channels/commands.rs`) turn a user's `/goal …` into the same
//! behaviour on every surface (TUI, Telegram, Discord, WhatsApp, Slack):
//!
//! - A **bare** `/goal` (no text) is DENIED. It shows a usage warning
//!   directly and does NOT touch the agent or the conversation context, so
//!   a fat-fingered `/goal` just leaves the chat exactly as it was.
//! - `/goal <text>` (or a management subcommand like `status`/`pause`/
//!   `resume`/`clear`) is forwarded to the agent as a `[SYSTEM:]` directive
//!   that calls the `slash_command` tool.
//!
//! Keeping the wording in one place stops the surfaces from drifting apart.

/// Warning shown when a user types a bare `/goal` with no text. Displayed
/// directly on every surface — never sent to the LLM, never persisted to
/// context. Tells the user the correct shape: command followed by the goal.
pub fn goal_usage_warning() -> String {
    "⚡ `/goal` needs a goal. Set one with `/goal <your goal>` — \
     for example `/goal fix the failing tests and commit`.\n\
     Other actions: `/goal status`, `/goal pause`, `/goal resume`, `/goal clear`."
        .to_string()
}

/// Build the `[SYSTEM:]` directive that turns a user's `/goal <text>` into a
/// `slash_command` tool call. `args` is everything typed after `/goal`
/// (whitespace trimmed here). Callers MUST check [`is_bare`] first and show
/// [`goal_usage_warning`] instead of calling this with empty args.
///
/// The directive ALWAYS instructs `command='/goal'` with the rest in the
/// separate `args` field. Folding the text into `command=` (e.g.
/// `command='/goal debug the build'`) makes the dispatch match — which
/// compares against `"/goal"` exactly — fall through and fail, so the call
/// has to be retried. Keeping command and args split avoids that round-trip.
pub fn goal_command_prompt(args: &str) -> String {
    let args = args.trim();
    format!(
        "[SYSTEM: The user wants to manage their autonomous goal. Call the \
         slash_command tool with command='/goal' and args='{args}'. The value of \
         args selects the action: a free-form description SETS a goal, 'status' \
         checks progress, 'pause' pauses, 'resume' resumes, 'clear' removes it.]"
    )
}

/// True when the `/goal` argument string is empty after trimming, i.e. the
/// user typed a bare `/goal` and should get the usage warning.
pub fn is_bare(args: &str) -> bool {
    args.trim().is_empty()
}
