//! The correction injected when a turn claims work it never executed
//! (#796, #797).
//!
//! The previous wording stated what was missing: "your last response produced
//! ZERO tool_use blocks". That is accurate and argues against a position the
//! model does not hold. It is not withholding a call it knows it skipped; it
//! believes the work already happened, having simulated the call while
//! reasoning and lost the distinction between the simulation and a result.
//! Told it emitted no tool_use blocks, a model in that state reads a
//! formatting complaint and leaves the belief intact.
//!
//! So the correction states the mechanism instead: nothing executes inside
//! reasoning, and the only evidence a tool ran is a result present in the
//! conversation. That is checkable, and it is checkable by the model.
//!
//! Pure so the wording is testable without a provider.

/// The escape hatch every variant must carry.
///
/// Without it the nudge becomes a loop: a model that genuinely finished, and
/// said so, gets told to call tools, calls something pointless to comply, and
/// is nudged again. Real completion has to have an exit that is not a tool
/// call.
const FINISHED_ESCAPE: &str = " If the work is genuinely done and you have already reported it, \
     reply with a short confirmation and stop; do not run extra tool calls to re-verify it.";

/// The mechanism, stated once and shared by every variant.
const NO_EXECUTION_WHILE_REASONING: &str = "Tools execute only between turns; nothing runs inside your reasoning. If you saw that \
     output while thinking, you imagined it. The only evidence a tool ran is its result \
     present in this conversation.";

/// Correction naming the exact commands claimed but never run (#797).
///
/// This is the one correction that does not infer. Every other phantom check
/// reads wording for signals, and wording is arguable; the loop knows what it
/// executed, so "you claimed X ran and X did not run" is a matter of fact and
/// cannot be talked around. Quoting it turns a category into a citation.
pub fn uncalled_commands_nudge(commands: &[String]) -> String {
    let quoted = commands
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let subject = if commands.len() == 1 {
        "output from"
    } else {
        "output from these commands:"
    };
    format!(
        "[System: You reported {subject} {quoted}. No such call ran this turn. \
         {NO_EXECUTION_WHILE_REASONING} Call the tool now through the structured tool-call API, \
         or retract the claim and tell the user it has not been run.{FINISHED_ESCAPE}]"
    )
}

/// Correction for a turn that produced no tool calls, when no specific
/// fabricated command was identified.
///
/// `local_model` selects wording for Qwen/Kimi/DeepSeek-class models, which
/// need two things the cloud variant does not. They read "STOP" as "wait for
/// further instruction" and reply with an acknowledgement instead of calling
/// anything, so the word is avoided. And they write `{"tool_call": {...}}` as
/// message text believing that IS the invocation, so the structured API is
/// named explicitly.
pub fn no_tool_calls_nudge(local_model: bool) -> String {
    let channel = if local_model {
        "Invoke it through the provider's structured tool-call API, the same channel the function \
         schemas were registered on. JSON or markdown written into your message is text and does \
         not execute."
    } else {
        "Call the tool through the structured tool-call API rather than describing it."
    };
    format!(
        "[System: Your last response claimed work but produced no tool calls, so nothing was \
         executed. {NO_EXECUTION_WHILE_REASONING} {channel} Pick the tool you need and call it \
         now.{FINISHED_ESCAPE}]"
    )
}
