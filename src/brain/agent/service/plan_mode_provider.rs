//! Which provider/model a turn runs on, given where the session sits in the
//! plan lifecycle (#792).
//!
//! Planning and executing are different jobs and reward different models, so
//! `/plan` can route to one pair and the execution that follows to another.
//! The mapping keys off plan STATE rather than the command that caused it,
//! which is what makes it surface-agnostic: `/execute` on Telegram, the TUI
//! approval, and the agent approving its own plan in prose all converge on
//! `try_approve` and all land in the same state, so all three route the same
//! way with no per-surface code.
//!
//! Everything here is pure. Deciding what the pair SHOULD be is separable from
//! the swap that installs it, and only the decision needs exhaustive tests.

use crate::brain::provider_spec::{ProviderKey, normalize_in};
use crate::config::Config;
use crate::config::types::AgentConfig;
use crate::utils::plan_files::PlanModeState;

/// A provider/model change a plan state asks for. Either field may be absent:
/// a provider with no model means that provider's default, and a model with no
/// provider means keep the current provider and swap the model alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeOverride {
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// The pair `state` calls for, or `None` to leave the session alone.
///
/// `None` is the important case: it is what an unconfigured install returns for
/// every state, and it must mean no computation, no swap, and behaviour
/// byte-identical to having no feature at all.
pub fn override_for(state: PlanModeState, agent: &AgentConfig) -> Option<ModeOverride> {
    let (provider, model) = if state.is_editing() {
        // Drafting: from `/plan` until the plan is approved.
        (agent.plan_provider.as_ref(), agent.plan_model.as_ref())
    } else if state == PlanModeState::Active {
        // Executing: from approval until the plan is 100% complete. The end is
        // the plan lifecycle's own, not a signal invented here: at every turn
        // settle the tool loop ticks the trailing delivery task (#737) and
        // archives a plan whose tasks are all resolved, which returns the
        // session to NoPlan and so to no override. A plan left incomplete,
        // failed or blocked never satisfies that, and deliberately keeps the
        // execute pair rather than reverting mid-work.
        (
            agent.execute_provider.as_ref(),
            agent.execute_model.as_ref(),
        )
    } else {
        // NoPlan: ordinary work is never routed.
        return None;
    };

    // Both unset is the default install. Returning None here rather than an
    // empty override is what guarantees the no-op: the caller never reaches the
    // swap path at all.
    if provider.is_none() && model.is_none() {
        return None;
    }

    Some(ModeOverride {
        provider: provider.cloned(),
        model: model.cloned(),
    })
}

/// [`override_for`] with the provider value normalised against `config`
/// (#1316): `"custom:myprovider"` or `"myprovider/some-model"` in
/// `plan_provider` / `execute_provider` resolves to the provider it names
/// instead of being handed to the swap verbatim. Logs one warning naming the
/// canonical spelling when a correction was applied. A bare name, and the
/// `None` no-op, pass through untouched.
pub fn normalized_override_for(state: PlanModeState, config: &Config) -> Option<ModeOverride> {
    let over = override_for(state, &config.agent)?;
    let Some(provider) = over.provider.as_deref() else {
        return Some(over);
    };
    let key = if state.is_editing() {
        ProviderKey::PLAN
    } else {
        ProviderKey::EXECUTE
    };
    let pair = normalize_in(config, key, provider, over.model.as_deref());
    if let Some(note) = pair.note.as_deref() {
        tracing::warn!(
            "Plan-mode routing: {} corrected to '{}': {note}",
            key.provider,
            pair.provider
        );
    }
    Some(ModeOverride {
        provider: Some(pair.provider),
        model: pair.model,
    })
}

/// The provider/model a session was on before a plan-mode override replaced it,
/// paired with what the override installed.
///
/// Both halves are needed. The original is where the session returns once the
/// plan archives. The applied pair is the evidence that the override is still
/// the thing in place: if the user switches provider themselves mid-plan, the
/// current pair no longer matches and restoring would clobber a deliberate
/// pick, so the record is dropped instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanModeSwap {
    pub original_provider: String,
    pub original_model: String,
    pub applied_provider: String,
    pub applied_model: String,
}

impl PlanModeSwap {
    /// Is `(provider, model)` still what this override installed?
    pub fn still_applied(&self, provider: &str, model: &str) -> bool {
        self.applied_provider == provider && self.applied_model == model
    }
}
