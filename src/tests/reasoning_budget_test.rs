//! Per-turn reasoning token budget (#970).
//!
//! A turn was observed burning 900+ seconds and 30k+ reasoning tokens while
//! `thinking_loop_timeout_secs` was armed at its 600s default. That guard is
//! per provider request, so a turn reasoning a little under the ceiling on each
//! of several iterations never trips it. This budget is scoped to the TURN
//! precisely so that shape cannot slip through.

use crate::brain::agent::service::reasoning_budget::{arm, charge, remaining};
use uuid::Uuid;

#[test]
fn charging_below_the_budget_leaves_headroom() {
    let s = Uuid::new_v4();
    let _g = arm(s, 1000);
    assert!(!charge(s, 400));
    assert_eq!(remaining(s), Some(600));
    assert!(!charge(s, 500));
    assert_eq!(remaining(s), Some(100));
}

#[test]
fn exhausting_the_budget_reports_stop() {
    let s = Uuid::new_v4();
    let _g = arm(s, 1000);
    assert!(!charge(s, 999));
    // The charge that lands exactly on zero is the one that stops it.
    assert!(charge(s, 1));
    assert_eq!(remaining(s), Some(0));
}

#[test]
fn an_oversized_single_charge_saturates_rather_than_wrapping() {
    let s = Uuid::new_v4();
    let _g = arm(s, 100);
    assert!(charge(s, usize::MAX));
    assert_eq!(remaining(s), Some(0));
}

#[test]
fn once_exhausted_it_stays_exhausted() {
    let s = Uuid::new_v4();
    let _g = arm(s, 10);
    assert!(charge(s, 10));
    // A stream that ignores the first signal must be cut on the next chunk.
    assert!(charge(s, 1));
    assert!(charge(s, 0));
}

#[test]
fn a_zero_budget_disables_enforcement() {
    let s = Uuid::new_v4();
    let _g = arm(s, 0);
    assert!(!charge(s, 1_000_000));
    assert_eq!(remaining(s), None);
}

#[test]
fn an_unarmed_session_has_no_budget() {
    // Streams outside a tool loop (compaction, title generation) must not be
    // cut by a budget nobody armed.
    let s = Uuid::new_v4();
    assert!(!charge(s, 1_000_000));
    assert_eq!(remaining(s), None);
}

#[test]
fn budgets_are_isolated_per_session() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let _ga = arm(a, 100);
    let _gb = arm(b, 100);
    assert!(charge(a, 100));
    // Draining one session must not touch a concurrent one.
    assert_eq!(remaining(b), Some(100));
    assert!(!charge(b, 1));
}

#[test]
fn the_guard_releases_the_budget_on_drop() {
    let s = Uuid::new_v4();
    {
        let _g = arm(s, 100);
        assert_eq!(remaining(s), Some(100));
    }
    // Otherwise a cancelled turn would throttle the session's next one.
    assert_eq!(remaining(s), None);
}

#[test]
fn a_fresh_turn_starts_whole() {
    let s = Uuid::new_v4();
    {
        let _g = arm(s, 100);
        assert!(charge(s, 100));
    }
    let _g = arm(s, 100);
    assert!(!charge(s, 50));
    assert_eq!(remaining(s), Some(50));
}

#[test]
fn the_default_budget_is_in_force_without_configuration() {
    // The observed runaway had no value set, so an unset config must still
    // enforce something. 16k is generous next to healthy turns and tight next
    // to the 30k+ that was observed.
    let cfg = crate::config::types::AgentConfig::default();
    assert_eq!(cfg.reasoning_token_budget, 16_000);
}

#[test]
fn a_provider_override_is_absent_by_default_so_the_global_applies() {
    let provider = crate::config::types::ProviderConfig::default();
    assert_eq!(provider.reasoning_token_budget, None);
}
