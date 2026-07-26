//! Plan-state provider routing (#792).
//!
//! Planning and executing reward different models, so `/plan` can route to its
//! own provider/model. The mapping keys off plan STATE, not the command, which
//! is what lets the TUI approval, the channel command and the agent approving
//! its own plan in prose all route identically.
//!
//! The load-bearing case is the one nobody configures: an install that sets
//! none of these keys must compute no override at all, so existing users see
//! byte-identical behaviour.

use crate::brain::agent::service::plan_mode_provider::{ModeOverride, PlanModeSwap, override_for};
use crate::config::types::AgentConfig;
use crate::utils::plan_files::PlanModeState;

const EVERY_STATE: [PlanModeState; 4] = [
    PlanModeState::NoPlan,
    PlanModeState::PreInitEditing,
    PlanModeState::PostInitEditing,
    PlanModeState::Active,
];

fn planning(provider: Option<&str>, model: Option<&str>) -> AgentConfig {
    AgentConfig {
        plan_provider: provider.map(str::to_string),
        plan_model: model.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn an_unconfigured_install_never_overrides() {
    // The guarantee for existing users: no key set means no override in ANY
    // state, so the caller never reaches the swap path.
    let agent = AgentConfig::default();
    for state in EVERY_STATE {
        assert_eq!(
            override_for(state, &agent),
            None,
            "unset config must not override in {state:?}"
        );
    }
}

#[test]
fn drafting_uses_the_plan_pair() {
    // Both Editing sub-states are the window between /plan and approval.
    let agent = planning(Some("anthropic"), Some("claude-opus-4-6"));
    for state in [
        PlanModeState::PreInitEditing,
        PlanModeState::PostInitEditing,
    ] {
        assert_eq!(
            override_for(state, &agent),
            Some(ModeOverride {
                provider: Some("anthropic".to_string()),
                model: Some("claude-opus-4-6".to_string()),
            }),
            "{state:?} must use the plan pair"
        );
    }
}

#[test]
fn a_session_with_no_plan_is_untouched() {
    // Ordinary work must not be routed anywhere, however the keys are set.
    let agent = planning(Some("anthropic"), Some("claude-opus-4-6"));
    assert_eq!(override_for(PlanModeState::NoPlan, &agent), None);
}

#[test]
fn a_provider_alone_means_that_providers_default_model() {
    // Model absent is not "no override": the caller fills it from the
    // provider's own default.
    let agent = planning(Some("anthropic"), None);
    assert_eq!(
        override_for(PlanModeState::PostInitEditing, &agent),
        Some(ModeOverride {
            provider: Some("anthropic".to_string()),
            model: None,
        })
    );
}

#[test]
fn a_model_alone_keeps_the_current_provider() {
    // Wanting a different model on the SAME provider is a real case and must
    // not require naming the provider redundantly.
    let agent = planning(None, Some("claude-opus-4-6"));
    assert_eq!(
        override_for(PlanModeState::PostInitEditing, &agent),
        Some(ModeOverride {
            provider: None,
            model: Some("claude-opus-4-6".to_string()),
        })
    );
}

#[test]
fn plan_keys_do_not_leak_into_execution() {
    // Executing is #793's half. Until then the plan pair must not silently
    // apply to it, or approving a plan would keep drafting's model.
    let agent = planning(Some("anthropic"), Some("claude-opus-4-6"));
    assert_eq!(override_for(PlanModeState::Active, &agent), None);
}

// ── Restore bookkeeping ─────────────────────────────────────────────────────
// Without this the override is permanent: `ensure_session_provider_restored`
// early-returns once a session has a provider entry, so a swap left in the map
// is never undone by the normal restore path.

#[test]
fn an_untouched_override_is_recognised_as_still_applied() {
    let swap = PlanModeSwap {
        original_provider: "modelstudio".to_string(),
        original_model: "qwen3.8-max".to_string(),
        applied_provider: "anthropic".to_string(),
        applied_model: "claude-opus-4-6".to_string(),
    };
    assert!(swap.still_applied("anthropic", "claude-opus-4-6"));
}

#[test]
fn a_user_switch_mid_plan_is_not_still_applied() {
    // If the pair is no longer what the override installed, the user changed
    // it deliberately. Restoring would clobber their pick, so the caller must
    // be able to tell the difference.
    let swap = PlanModeSwap {
        original_provider: "modelstudio".to_string(),
        original_model: "qwen3.8-max".to_string(),
        applied_provider: "anthropic".to_string(),
        applied_model: "claude-opus-4-6".to_string(),
    };
    assert!(!swap.still_applied("openrouter", "some-other-model"));
    assert!(
        !swap.still_applied("anthropic", "claude-sonnet-4-6"),
        "same provider but a different model is still a deliberate change"
    );
}
