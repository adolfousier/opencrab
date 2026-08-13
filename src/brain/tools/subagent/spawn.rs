//! spawn_agent tool — creates a child agent with forked context.
//!
//! Sub-agent progress is streamed to `~/.opencrabs/tmp/subagents/<agent_id>.json`
//! so the main orchestrator can track status without session_search.

use super::manager::{SubAgent, SubAgentManager, SubAgentState};
use super::status::AgentStatus;
use crate::brain::tools::error::{Result, ToolError};
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Tool that spawns a child agent to handle a sub-task.
pub struct SpawnAgentTool {
    manager: Arc<SubAgentManager>,
    parent_registry: Arc<crate::brain::tools::ToolRegistry>,
}

impl SpawnAgentTool {
    pub fn new(
        manager: Arc<SubAgentManager>,
        parent_registry: Arc<crate::brain::tools::ToolRegistry>,
    ) -> Self {
        Self {
            manager,
            parent_registry,
        }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Spawn a child agent to handle a sub-task autonomously. The child gets its own session \
         and runs in the background. Returns an agent_id you can use with wait_agent, send_input, \
         close_agent, or resume_agent. Use this to delegate independent work items. \
         \n\nProvider and model resolution (highest priority first): \
         (1) the optional `provider` / `model` parameters on THIS call, \
         (2) the user's config.toml `[agent]` keys `subagent_provider` / `subagent_model`, \
         (3) the parent session's provider with that provider's default model. \
         Use the per-call params when a single skill orchestrates multiple steps that each \
         want a different model (for example: plan with one model, code with another, review \
         with a third). Use the config keys when every sub-agent in the session should share \
         the same routing. Use no override to let the child inherit the parent."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task/instruction for the child agent to execute"
                },
                "label": {
                    "type": "string",
                    "description": "Short human-readable label for this sub-agent (e.g., 'refactor-auth', 'test-runner')"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Agent specialization: 'general' (full tools), 'explore' (read-only), 'plan' (read+bash), 'code' (full write), 'research' (web+read). Default: general",
                    "enum": ["general", "explore", "architect", "plan", "code", "research"]
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider override for THIS spawn (e.g., 'zhipu', 'openrouter', 'custom:my-provider'). Highest precedence — overrides config.agent.subagent_provider and parent inheritance. Use to route this single sub-agent differently from the global subagent config."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for THIS spawn (model id as the chosen provider accepts it, e.g., 'glm-5', 'deepseek-coder'). Highest precedence — overrides config.agent.subagent_model. Pair with `provider` when the model lives on a provider other than the parent session's."
                },
                "plan_session": {
                    "type": "string",
                    "description": "Optional session UUID whose plan state this child operates on (#908). When set, the child's plan tool resolves that session's plan (JSON, design .md, markers, task goal) instead of its own. Plan-driven execution passes the parent session id here so a task worker sees the parent's checklist; the child's own session stays fresh. Omit for normal sub-agents."
                }
            },
            "required": ["prompt"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'prompt' is required".into()))?
            .to_string();

        let label = input
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("sub-agent")
            .to_string();

        let agent_type = super::AgentType::parse(
            input
                .get("agent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("general"),
        );

        // Optional plan-state override (#908 option A): plan-driven
        // execution hands the child the PARENT's session id so the worker's
        // plan tool resolves the parent's checklist while the worker session
        // itself stays fresh. A malformed UUID is a hard error — silently
        // falling back to the child's own session would let the worker run
        // against an empty plan and report success on nothing.
        let plan_session_override = match input
            .get("plan_session")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(raw) => Some(uuid::Uuid::parse_str(raw).map_err(|e| {
                ToolError::InvalidInput(format!("'plan_session' must be a valid UUID: {e}"))
            })?),
            None => None,
        };

        // We need a ServiceContext to create a session for the child
        let service_context = context
            .service_context
            .as_ref()
            .ok_or_else(|| ToolError::Execution("No service context available".into()))?
            .clone();

        // Create a new session for the child agent
        let session_service = crate::services::SessionService::new(service_context.clone());
        let child_session = session_service
            .create_session(Some(format!("subagent: {}", label)))
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to create child session: {}", e)))?;

        let child_session_id = child_session.id;
        let agent_id = SubAgentManager::generate_id();

        // Create cancel token and input channel for the child
        let cancel_token = CancellationToken::new();
        let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();

        // Per-call provider / model overrides, read from the tool
        // call's input. Precedence (issue #152): per-call > config >
        // parent inheritance. Empty strings are treated as unset so an
        // optional schema field passed as "" doesn't accidentally
        // resolve to an invalid provider name.
        let call_provider = input
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let call_model = input
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // Load config and extract model override before entering block scope
        let config = crate::config::Config::load()
            .map_err(|e| ToolError::Execution(format!("Config load failed: {}", e)))?;
        // Precedence: per-call model > config.subagent_model > None
        // (when None, the child uses its provider's default model).
        let model_override = call_model
            .clone()
            .or_else(|| config.agent.subagent_model.clone());

        // Resolve the effective provider name with the same precedence:
        // per-call provider > config.subagent_provider > parent default.
        // Captured for the log line so users picking a model on a
        // different provider can see which one was actually used.
        let effective_provider_name = call_provider
            .clone()
            .or_else(|| config.agent.subagent_provider.clone());

        // Build a minimal AgentService for the child
        let child_service = {
            // Use the resolved per-call/config provider if any,
            // otherwise inherit parent's. The fallback-on-failure
            // path keeps a typo in the override from breaking the
            // spawn entirely — same shape as the prior config-only
            // resolution.
            let provider = if let Some(ref provider_name) = effective_provider_name {
                match crate::brain::provider::create_provider_by_name(&config, provider_name).await
                {
                    Ok(p) => {
                        let source = if call_provider.is_some() {
                            "per-call"
                        } else {
                            "config"
                        };
                        tracing::info!("Sub-agent using {source} provider '{provider_name}'");
                        p
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Sub-agent provider '{}' failed: {e}, falling back to parent",
                            provider_name
                        );
                        crate::brain::provider::create_provider(&config)
                            .await
                            .map_err(|e| {
                                ToolError::Execution(format!("Failed to create provider: {}", e))
                            })?
                    }
                }
            } else {
                crate::brain::provider::create_provider(&config)
                    .await
                    .map_err(|e| {
                        ToolError::Execution(format!("Failed to create provider: {}", e))
                    })?
            };

            // Build filtered tool registry based on agent type
            let child_registry = agent_type.build_registry(&self.parent_registry);
            // #649: a child spawned while the PARENT session is in Plan-mode
            // Editing must be read-only. The child runs under a fresh session
            // that resolves to NoPlan, so the per-call plan gate never fires
            // for it; strip the mutating tools from its registry instead so it
            // can read and review the design but cannot write the project, run
            // bash, or spawn further agents (which would escape the parent's
            // write-freeze). Currently reachable only after spawn_agent leaves
            // EDITING_DENIED_NAMES; landing this filter first keeps that
            // removal safe.
            if matches!(
                crate::utils::plan_files::plan_mode_state(context.session_id).await,
                crate::utils::plan_files::PlanModeState::PreInitEditing
                    | crate::utils::plan_files::PlanModeState::PostInitEditing
            ) {
                crate::brain::tools::plan_gate::restrict_registry_to_read_only(&child_registry);
                tracing::info!(
                    "Sub-agent spawned under a Plan-mode Editing parent: \
                     child registry restricted to read-only (#649)"
                );
            }

            let agent =
                crate::brain::agent::AgentService::new(provider, service_context.clone(), &config)
                    .await
                    .with_tool_registry(Arc::new(child_registry))
                    .with_auto_approve_tools(true) // children auto-approve (parent already approved spawn)
                    .with_working_directory(context.working_dir())
                    .with_plan_session_override(plan_session_override);

            Arc::new(agent)
        };

        // Prepend agent type system prompt to the user's task
        let full_prompt = format!("{}\n\n{}", agent_type.system_prompt(), prompt);

        // Create the status file in Pending state before spawning. new()
        // writes the file; we don't need the returned handle, but we do
        // propagate any write error.
        let _ = AgentStatus::new(
            &agent_id,
            &label,
            &child_session_id.to_string(),
            &full_prompt,
        )
        .map_err(|e| ToolError::Execution(format!("Failed to create status file: {e}")))?;

        // Spawn background task with input loop
        let cancel_clone = cancel_token.clone();
        let manager = self.manager.clone();
        let agent_id_clone = agent_id.clone();
        let prompt_clone = full_prompt;
        let label_clone = label.clone();
        let mut input_rx = input_rx;

        let handle = tokio::spawn(async move {
            tracing::info!("Sub-agent {} starting: {}", agent_id_clone, prompt_clone);

            // Transition to Running state.
            let mut status = AgentStatus::read(&agent_id_clone).unwrap_or_else(|| {
                AgentStatus::new(
                    &agent_id_clone,
                    &label_clone,
                    &child_session_id.to_string(),
                    &prompt_clone,
                )
                .expect("status file")
            });
            if !matches!(
                status.state,
                super::status::AgentState::Completed | super::status::AgentState::Failed
            ) && let Err(e) = status.mark_running()
            {
                tracing::warn!("Failed to write running status: {e}");
            }

            // Reload with correct state.

            let mut current_prompt = prompt_clone;
            let mut iteration: usize = 0;

            // Run prompt → wait for input → run again loop
            let final_output = loop {
                iteration += 1;
                let result = child_service
                    .send_message_with_tools_and_mode(
                        child_session_id,
                        current_prompt,
                        model_override.clone(),
                        Some(cancel_clone.clone()),
                    )
                    .await;

                match result {
                    Ok(response) => {
                        // Extract a short summary of what the agent did this turn.
                        let summary = if response.stop_reason
                            == Some(crate::brain::provider::types::StopReason::ToolUse)
                        {
                            "tool call(s) completed".to_string()
                        } else {
                            response.content.chars().take(120).collect::<String>()
                        };

                        status
                            .update_progress(iteration, None, Some(summary))
                            .unwrap_or_else(|e| tracing::warn!("status write failed: {e}"));

                        manager.update_output(&agent_id_clone, response.content.clone());
                        // Flip to AwaitingInput so wait_agent can observe
                        // round-boundary progress instead of blocking on
                        // task-join semantics (the task never terminates
                        // at a round — only on input/cancel — so the old
                        // `handle.await` in wait.rs always hit its
                        // timeout_secs and the LLM gave up the turn).
                        manager.mark_awaiting_input(&agent_id_clone);
                        tracing::info!(
                            "Sub-agent {} round {} complete, waiting for input",
                            agent_id_clone,
                            iteration
                        );

                        // Wait for follow-up input or shutdown
                        let next = tokio::select! {
                            msg = input_rx.recv() => msg,
                            _ = cancel_clone.cancelled() => {
                                tracing::info!("Sub-agent {} cancelled while waiting for input", agent_id_clone);
                                None
                            }
                        };

                        match next {
                            Some(text) => {
                                manager.mark_running_again(&agent_id_clone);
                                tracing::info!(
                                    "Sub-agent {} received follow-up input",
                                    agent_id_clone
                                );
                                current_prompt = text;
                            }
                            None => break response.content,
                        }
                    }
                    Err(e) => {
                        tracing::error!("Sub-agent {} failed: {}", agent_id_clone, e);
                        // A dropped write here loses the record of a failure
                        // that already happened, and leaves the file reading
                        // `Running` forever. Proceed either way, but say so.
                        if let Err(write_err) = status.mark_failed(e.to_string()) {
                            tracing::error!(
                                "Sub-agent {} failed and its failure status could not be written, \
                                 so it will keep reading as running: {write_err}",
                                agent_id_clone
                            );
                        }
                        manager.mark_failed(&agent_id_clone, e.to_string());
                        return;
                    }
                }
            };

            if let Err(write_err) = status.mark_completed(final_output.chars().take(200).collect())
            {
                tracing::error!(
                    "Sub-agent {} completed but its status could not be written, so it will keep \
                     reading as running: {write_err}",
                    agent_id_clone
                );
            }
            manager.mark_completed(&agent_id_clone, final_output);
        });

        // Register in manager
        self.manager.insert(SubAgent {
            id: agent_id.clone(),
            label: label.clone(),
            session_id: child_session_id,
            state: SubAgentState::Running,
            cancel_token,
            join_handle: Some(handle),
            input_tx: Some(input_tx),
            output: None,
            spawned_at: chrono::Utc::now(),
        });

        Ok(ToolResult::success(format!(
            "Spawned sub-agent '{}' with id: {}\nSession: {}\nPrompt: {}",
            label, agent_id, child_session_id, prompt
        )))
    }
}
