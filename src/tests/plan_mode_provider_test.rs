//! Plan-state provider routing (#792, #793).
//!
//! Planning and executing reward different models, so `/plan` and `/execute`
//! each route to their own provider/model. The mapping keys off plan STATE,
//! not the command, which
//! is what lets the TUI approval, the channel command and the agent approving
//! its own plan in prose all route identically.
//!
//! The load-bearing case is the one nobody configures: an install that sets
//! none of these keys must compute no override at all, so existing users see
//! byte-identical behaviour.

use crate::brain::agent::service::plan_mode_provider::{
    ModeOverride, PlanModeSwap, normalized_override_for, override_for,
};
use crate::config::Config;
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
    // Routing only the drafting half must leave execution on the session's own
    // model; otherwise approving a plan would silently keep drafting's model.
    let agent = planning(Some("anthropic"), Some("claude-opus-4-6"));
    assert_eq!(override_for(PlanModeState::Active, &agent), None);
}

// ── Executing (#793) ────────────────────────────────────────────────────────

fn executing(provider: Option<&str>, model: Option<&str>) -> AgentConfig {
    AgentConfig {
        execute_provider: provider.map(str::to_string),
        execute_model: model.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn executing_uses_the_execute_pair() {
    let agent = executing(Some("openrouter"), Some("qwen3.8-max"));
    assert_eq!(
        override_for(PlanModeState::Active, &agent),
        Some(ModeOverride {
            provider: Some("openrouter".to_string()),
            model: Some("qwen3.8-max".to_string()),
        })
    );
}

#[test]
fn execute_keys_do_not_leak_into_drafting() {
    // The mirror of plan_keys_do_not_leak_into_execution: configuring only the
    // execution pair must leave planning on the session's own model.
    let agent = executing(Some("openrouter"), Some("qwen3.8-max"));
    for state in [
        PlanModeState::PreInitEditing,
        PlanModeState::PostInitEditing,
    ] {
        assert_eq!(
            override_for(state, &agent),
            None,
            "{state:?} must not use the execute pair"
        );
    }
}

#[test]
fn drafting_and_executing_route_independently() {
    // The whole point: two different pairs across one plan's life.
    let agent = AgentConfig {
        plan_provider: Some("anthropic".to_string()),
        plan_model: Some("claude-opus-4-6".to_string()),
        execute_provider: Some("openrouter".to_string()),
        execute_model: Some("qwen3.8-max".to_string()),
        ..Default::default()
    };
    assert_eq!(
        override_for(PlanModeState::PostInitEditing, &agent)
            .and_then(|o| o.provider)
            .as_deref(),
        Some("anthropic")
    );
    assert_eq!(
        override_for(PlanModeState::Active, &agent)
            .and_then(|o| o.provider)
            .as_deref(),
        Some("openrouter")
    );
    // And the session returns to its own pair once the plan archives.
    assert_eq!(override_for(PlanModeState::NoPlan, &agent), None);
}

#[test]
fn only_one_half_configured_leaves_the_other_alone() {
    // Routing just execution is a legitimate setup and must not drag planning
    // along with it, nor vice versa.
    let plan_only = planning(Some("anthropic"), None);
    assert_eq!(override_for(PlanModeState::Active, &plan_only), None);

    let exec_only = executing(Some("openrouter"), None);
    assert_eq!(
        override_for(PlanModeState::PostInitEditing, &exec_only),
        None
    );
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

// ------------------------------------------------------------- #1316

fn full_config(json: &str) -> Config {
    serde_json::from_str(json).expect("fixture config")
}

#[test]
fn a_custom_prefixed_plan_provider_resolves_to_the_section_name() {
    let cfg = full_config(
        r#"{
            "agent": { "plan_provider": "custom:myprovider", "plan_model": "some-model" },
            "providers": {
                "custom": { "myprovider": { "base_url": "http://localhost:1/v1", "api_key": "k" } }
            }
        }"#,
    );
    assert_eq!(
        normalized_override_for(PlanModeState::PreInitEditing, &cfg),
        Some(ModeOverride {
            provider: Some("myprovider".into()),
            model: Some("some-model".into()),
        })
    );
}

#[test]
fn an_execute_provider_slash_model_lands_the_model_in_the_override() {
    let cfg = full_config(
        r#"{
            "agent": { "execute_provider": "zhipu/glm-5.3" },
            "providers": { "zhipu": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    assert_eq!(
        normalized_override_for(PlanModeState::Active, &cfg),
        Some(ModeOverride {
            provider: Some("zhipu".into()),
            model: Some("glm-5.3".into()),
        })
    );
    // Editing states read the plan pair, which is unset here.
    assert_eq!(
        normalized_override_for(PlanModeState::PreInitEditing, &cfg),
        None
    );
}

#[test]
fn a_model_only_override_and_the_no_op_pass_through_unnormalised() {
    let model_only = full_config(r#"{ "agent": { "plan_model": "some-model" }, "providers": {} }"#);
    assert_eq!(
        normalized_override_for(PlanModeState::PostInitEditing, &model_only),
        Some(ModeOverride {
            provider: None,
            model: Some("some-model".into()),
        })
    );
    let unset = full_config(r#"{ "agent": {}, "providers": {} }"#);
    for state in EVERY_STATE {
        assert_eq!(normalized_override_for(state, &unset), None, "{state:?}");
    }
}
