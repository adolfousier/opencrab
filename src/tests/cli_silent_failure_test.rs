//! A claude-cli turn that produces nothing must fail loudly (#1441).
//!
//! Reported symptom: on a subscription rate limit with no fallback chain
//! configured, the turn ended with NOTHING delivered to the channel. The user
//! saw silence, not an error.
//!
//! Three shapes produced that silence, all in `claude_cli.rs`:
//!
//! 1. `rate_limit_event` sent a `Ping` and recorded nothing, so a limit that
//!    never recovered was indistinguishable from a healthy pause.
//! 2. EOF without a `Result` synthesised `MessageDelta{EndTurn} + MessageStop`
//!    — a *successful* empty turn.
//! 3. A clean exit having emitted zero events only logged a warning.
//!
//! Nothing downstream caught any of them: CLI providers are deliberately exempt
//! from the empty-answer guard (`should_nudge_empty_answer`) because they run
//! their own inner loop, so the retry/fallback ladder never ran and the
//! placeholder was deleted. The evidence for which shape dominates came from
//! the logs: `CLI EOF without Result` appeared 54 times against 2 for the
//! working error path.
//!
//! The fix belongs in the provider, not in the guard: removing the CLI
//! exemption would nudge on every legitimate mid-loop iteration.

const CLI_SRC: &str = include_str!("../brain/provider/claude_cli.rs");
const HELPERS_SRC: &str = include_str!("../brain/agent/service/helpers.rs");

/// The stream runs inside a spawned task over a live child process, so these
/// are source-level sentinels. Each one fails if its half of the fix is
/// reverted.
#[test]
fn rate_limit_event_is_recorded_not_just_pinged() {
    let arm = CLI_SRC
        .split("CliMessage::RateLimitEvent {} =>")
        .nth(1)
        .expect("the rate_limit_event arm is gone");
    let arm = &arm[..arm.find("\n                    }").unwrap_or(arm.len())];

    assert!(
        arm.contains("rate_limited = true"),
        "a rate_limit_event must be recorded; without the flag a limit that \
         never recovers cannot be told apart from a healthy pause (#1441)"
    );
    assert!(
        arm.contains("StreamEvent::Ping"),
        "the stream must still be kept alive through the pause — the CLI \
         resumes on its own when the window rolls over"
    );
}

#[test]
fn an_empty_resultless_turn_is_reported_as_an_error() {
    assert!(
        CLI_SRC.contains("let produced_content = completed_blocks > 0 || current_block_chars > 0;"),
        "the EOF path must distinguish 'produced content' from 'produced nothing'"
    );

    let block = CLI_SRC
        .split("if !result_received && !produced_content {")
        .nth(1)
        .expect(
            "the empty-turn failure branch is gone — a resultless, \
                 contentless turn is being reported as success again (#1441)",
        );
    let block = &block[..block.find("\n            }").unwrap_or(block.len())];

    assert!(
        block.contains("ProviderError::RateLimitExceeded"),
        "a turn that died after a rate-limit event must fail as \
         RateLimitExceeded so the fallback chain is walked"
    );
    assert!(
        block.contains("ProviderError::Internal"),
        "a turn that died without any content must still fail, even when no \
         rate limit was seen"
    );
    assert!(
        block.contains(".send(Err("),
        "the failure must reach the stream; logging alone is what left the \
         channel silent"
    );
}

#[test]
fn a_clean_exit_with_no_output_is_not_silent() {
    let arm = CLI_SRC
        .split("claude CLI exited successfully but produced no stream events")
        .nth(1)
        .expect("the clean-exit branch is gone");
    let arm = &arm[..arm.find("\n                }").unwrap_or(arm.len())];

    // `.send(Err(` rather than `tx.send(Err(`: rustfmt splits the receiver
    // onto its own line, so the contiguous form is a formatting assertion,
    // not a behavioural one.
    assert!(
        arm.contains(".send(Err("),
        "a clean exit that emitted nothing must send an error; it previously \
         only logged, so the turn ended in silence (#1441)"
    );
}

#[test]
fn the_exit_branches_do_not_double_report() {
    assert_eq!(
        CLI_SRC.matches("!started && !failure_reported").count(),
        2,
        "both exit-status branches must respect failure_reported, or a turn \
         already failed at EOF gets a second error on top"
    );
}

#[test]
fn the_cli_carve_out_in_the_empty_answer_guard_is_deliberate() {
    // Pinning the constraint that forced the fix into the provider. If this
    // exemption is ever dropped, every legitimate mid-loop CLI iteration
    // starts getting nudged, and #1441's fix location stops making sense.
    assert!(
        HELPERS_SRC
            .contains("iteration > 0 && !is_cli_provider && iteration_text.trim().is_empty()"),
        "should_nudge_empty_answer still exempts CLI providers; the silent-turn \
         fix lives in claude_cli.rs precisely because this guard cannot catch it"
    );
}
