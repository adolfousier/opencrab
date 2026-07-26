//! Null-effect calls must not buy phantom immunity (#825).
//!
//! The post-success exemption asks "did any tool succeed", when what vouches
//! for a claim is "did a tool DO anything". Observed: seven green tool calls,
//! none of them the one being claimed — two `true` no-ops and two `echo`s
//! narrating an intent (`echo "attempting telegram_send"`) — after which the
//! turn asserted it had "tried telegram_send a dozen times" and dumped an
//! 8.6KB report inline instead of sending the file.
//!
//! The calls were the substitute for the work, and they disabled every check
//! that keys off tool success.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::{all_calls_were_null_effect, is_null_effect_command};

fn call(command: &str) -> String {
    serde_json::json!({ "command": command }).to_string()
}

#[test]
fn the_observed_theatre_is_recognised() {
    // The exact shape from the incident.
    let turn = vec![
        call("echo \"calling telegram_send send_document next\""),
        call("true"),
        call("echo \"attempting telegram_send\""),
    ];
    assert!(all_calls_were_null_effect(&turn));
}

#[test]
fn one_real_call_is_enough_to_vouch() {
    // A turn that actually observed something must keep its exemption, or
    // every ordinary turn gets re-prompted.
    let turn = vec![
        call("true"),
        call("ls -la /srv/app"),
        call("echo \"about to run tests\""),
    ];
    assert!(!all_calls_were_null_effect(&turn));
}

#[test]
fn a_turn_with_no_calls_is_not_reported_here() {
    // Zero calls is already handled by its own branch; reporting it here
    // would double-count.
    assert!(!all_calls_were_null_effect(&[]));
}

#[test]
fn a_non_shell_tool_is_always_real_work() {
    // read_file, grep and friends observe something by definition.
    let turn = vec![serde_json::json!({ "path": "src/main.rs" }).to_string()];
    assert!(!all_calls_were_null_effect(&turn));
}

// ── Which commands count as null ────────────────────────────────────────────

#[test]
fn no_ops_are_null() {
    assert!(is_null_effect_command("true"));
    assert!(is_null_effect_command(":"));
    assert!(is_null_effect_command("   "));
}

#[test]
fn a_bare_echo_of_a_literal_is_null() {
    assert!(is_null_effect_command("echo \"attempting telegram_send\""));
    assert!(is_null_effect_command("echo hello"));
}

#[test]
fn an_echo_that_writes_a_file_is_real_work() {
    // echo legitimately builds files and here-docs. Only printing to nobody
    // is null.
    assert!(!is_null_effect_command("echo \"content\" > out.txt"));
    assert!(!is_null_effect_command("echo \"a\" >> log.txt"));
    assert!(!is_null_effect_command("echo hi | tee f.txt"));
}

#[test]
fn an_echo_whose_output_is_consumed_is_real_work() {
    assert!(!is_null_effect_command("echo hi && cargo test"));
    assert!(!is_null_effect_command("echo hi; ls"));
    assert!(!is_null_effect_command("echo $(git rev-parse HEAD)"));
}

#[test]
fn an_echo_reading_state_is_real_work() {
    // `echo $VAR` observes the environment, unlike a literal.
    assert!(!is_null_effect_command("echo $HOME"));
}

#[test]
fn ordinary_commands_are_never_null() {
    for cmd in [
        "ls -la",
        "cargo test --all-features",
        "git status --short",
        "grep -rn foo src/",
    ] {
        assert!(!is_null_effect_command(cmd), "must be real work: {cmd}");
    }
}
