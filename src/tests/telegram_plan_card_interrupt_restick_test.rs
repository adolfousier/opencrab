//! #69 regression tests: interrupt-delivered turns must re-stick the plan
//! card through the SAME settle-path tail as normal settles.
//!
//! Before #69, a preempted turn early-returned through the cancel_token
//! teardown (`return Ok(())`) before the #62 re-stick block ran, so during an
//! interrupt chain every reply skipped the restick and the card stayed buried
//! at a stale position until a normal settle finally happened. The fix routes
//! the teardown through `plan_card::restick_plan_card_after_turn` — the exact
//! block the normal settle runs.
//!
//! These tests pin the three seams the interrupt tail must satisfy, without
//! touching the network or the Bot:
//! 1. The interrupt restick draws from the SAME shared sticky-stack budget
//!    (#1150) as normal settles — no independent gate, no bypass.
//! 2. A cardless turn spends NOTHING from that budget (the tracked-card claim
//!    gates it) — the flow-block restick must not starve for 15s after every
//!    interrupt-delivered cardless reply (#62 lesson).
//! 3. The keyboard the interrupt path passes is recomputed for a FINISHED
//!    turn (`turn_active == false` via `load_plan_state_section`), so the
//!    Approve/Discard keyboard materializes on the resticked card exactly as
//!    it does on the normal settle path (#571 gate).
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::flow_chrome::{PlanKb, plan_state_chrome};
use crate::channels::telegram::state::TelegramState;
use crate::utils::plan_files::PlanModeState;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Seam 1 + 2: the interrupt tail calls `claim_sticky_action` with the same
/// shared `STICKY_STACK_MIN_INTERVAL` the settle path uses, gated on a tracked
/// card. Pinned against the real state object: a chat whose sticky action was
/// just spent must refuse the claim within the interval (budget is shared,
/// regardless of which path spent it), and a session with no tracked card
/// never reaches the claim at all (`plan_card_cached` is the gate).
#[tokio::test]
async fn interrupt_restick_shares_the_sticky_budget_and_spends_nothing_cardless() {
    let state = Arc::new(TelegramState::new());
    let session = Uuid::new_v4();
    let chat = teloxide::types::ChatId(690_001);

    // Seam 2: cardless — the exact gate the tail evaluates before claiming.
    assert!(
        state.plan_card_cached(session).await.is_none(),
        "cardless session must not reach the sticky claim at all"
    );

    // Simulate a settle-path restick having just spent the budget (the normal
    // path claims via the same helper): the shared budget is now hot.
    assert!(
        state.claim_sticky_action(chat.0, TelegramState::STICKY_STACK_MIN_INTERVAL),
        "first claim (simulated normal settle) must be admitted"
    );

    // The interrupt tail's claim, moments later: REFUSED — the two paths draw
    // from ONE budget, this is the flood-safety contract (#1150, #814 stays
    // deleted: no cooldown is recorded on a skip, but a real burst refuses).
    assert!(
        !state.claim_sticky_action(chat.0, TelegramState::STICKY_STACK_MIN_INTERVAL),
        "interrupt-path restick must share the settle-path sticky budget"
    );

    // After the interval elapses the budget frees up for the next restick,
    // whichever path needs it. (Same seam the settle path uses; pinned here so
    // a future per-path budget split fails loudly.)
    std::thread::sleep(TelegramState::STICKY_STACK_MIN_INTERVAL + Duration::from_millis(5));
    assert!(
        state.claim_sticky_action(chat.0, TelegramState::STICKY_STACK_MIN_INTERVAL),
        "budget must free up after the interval for the next restick"
    );
}

/// Seam 3: the interrupt path recomputes the keyboard for a finished turn.
/// The teardown calls `load_plan_state_section(session_id, false)` — the
/// turn is over, so `turn_active == false` and the PostInitEditing state
/// yields the Approve/Discard keyboard, mirroring the settle path's
/// `refresh_sections` recompute (#571). Pinned at the pure seam
/// (`plan_state_chrome`) the loader delegates to, with the live plan files
/// absent (NoPlan is the state any CI environment sees).
#[test]
fn interrupt_tail_recomputes_keyboard_for_a_finished_turn() {
    // What the teardown computes: turn finished (turn_active == false).
    let (label, kb) = plan_state_chrome(PlanModeState::PostInitEditing, false, false);
    assert_eq!(label, Some("✍️ Editing plan".to_string()));
    assert_eq!(
        kb,
        PlanKb::ApproveDiscard,
        "the resticked card must carry the Approve/Discard keyboard after an \
         interrupt-delivered turn ends, exactly like a normal settle (#571)"
    );

    // Contrast: the same state mid-turn (a concurrent in-flight turn elsewhere
    // recomputed during the interrupt window) strips the keyboard.
    assert_eq!(
        plan_state_chrome(PlanModeState::PostInitEditing, true, false).1,
        PlanKb::None,
        "an in-flight turn must still render keyboardless chrome"
    );
}
