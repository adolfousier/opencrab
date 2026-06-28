//! Tests for the channel reconcile decision (issues #239, #240).
//!
//! The WhatsApp QR never reappeared on reconnect because a dead agent left a
//! stale `JoinHandle` in the map and `contains_key` treated it as running, so
//! reconcile never restarted it. `channel_action` keys the decision off whether
//! the agent is ALIVE, so a finished (crashed/exited) agent yields `Start`.

use crate::channels::manager::{ChannelAction, channel_action};

#[test]
fn enabled_and_dead_agent_restarts() {
    // The core fix: should run, but the agent task is not alive (crashed, or a
    // stale finished handle) -> restart instead of being wedged forever.
    assert_eq!(channel_action(true, false), ChannelAction::Start);
}

#[test]
fn disabled_and_alive_stops() {
    assert_eq!(channel_action(false, true), ChannelAction::Stop);
}

#[test]
fn enabled_and_alive_is_noop() {
    assert_eq!(channel_action(true, true), ChannelAction::Noop);
}

#[test]
fn disabled_and_dead_is_noop() {
    assert_eq!(channel_action(false, false), ChannelAction::Noop);
}
