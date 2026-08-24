//! Tool-failure classification policy for the RSI feedback ledger.
//!
//! Not every tool call that returns `success = false` is a tool DEFECT worth
//! learning from. Many are expected, environmental, or user-driven outcomes:
//! a stale-hash mismatch the agent simply retries after re-reading; a
//! channel-send tool with no live bot connected; a user declining an
//! interactive prompt. Counting these as failures dragged tool success-rates
//! down and led the RSI to ban working built-in tools — e.g. escalating
//! `hashline_edit` to a "blanket DO NOT USE" over recoverable stale-hash
//! errors (#236).
//!
//! This module is the single place that decides whether a failed call is
//! *recoverable* (recorded, but kept OUT of the `tool_` success-rate
//! denominator) or a genuine failure.

/// Whether a failed tool call is a recoverable / environmental / user-driven
/// outcome rather than a tool defect.
///
/// Recoverable failures are still recorded in the ledger (under a non-`tool_`
/// event type) so they stay queryable, but they never count against the tool's
/// success rate and therefore never drive the RSI to disable a working tool.
pub fn is_recoverable_tool_failure(tool_name: &str, error: Option<&str>) -> bool {
    let Some(error) = error else {
        return false;
    };
    let e = error.to_ascii_lowercase();
    match tool_name {
        // Stale-hash mismatch: the file changed since the agent's last read, so
        // the line hash no longer resolves. The fix is to re-read and retry — a
        // normal step in the edit workflow, not a defect in the tool.
        "hashline_edit" => {
            e.contains("not found")
                || e.contains("may have changed")
                || e.contains("hash")
                || e.contains("collision")
        }
        // Channel send/connect tools need a live, connected bot. "Not
        // connected" (and connection errors generally) is an environment state,
        // not a tool defect — the tool works once the channel is up.
        n if n.ends_with("_send") || n.ends_with("_connect") => {
            e.contains("not connected") || e.contains("connect")
        }
        // bash carries two failure populations under one tool name (#1068). A
        // correctly written command that hit a missing file, a closed port or a
        // dead service says nothing about the tool, and counting it as a defect
        // made `tool_failure|bash` the ledger's loudest signal, steering RSI at
        // "bash is broken" when the real finding was a bad command or an absent
        // resource.
        //
        // Model errors stay hard failures on purpose: wrong syntax, an invented
        // binary, a bare REPL. Those are agent behaviour RSI should act on.
        // So does an unclassifiable failure, because guessing "environmental"
        // would quietly drop real defects out of the denominator.
        "bash" => {
            crate::brain::bash_failure::classify(error)
                == crate::brain::bash_failure::BashFailureKind::Environmental
        }
        _ => false,
    }
}
