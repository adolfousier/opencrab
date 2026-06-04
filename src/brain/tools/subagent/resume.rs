//! resume_agent tool — resumes a completed/failed child agent with new input.

use super::manager::{SubAgentManager, SubAgentState};
use crate::brain::tools::error::{Result, ToolError};
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Tool that resumes a previously completed or failed sub-agent.
pub struct ResumeAgentTool {
    manager: Arc<SubAgentManager>,
    parent_registry: Arc<crate::brain::tools::ToolRegistry>,
}

impl ResumeAgentTool {
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
impl Tool for ResumeAgentTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn description(&self) -> &str {
        "Resume a completed or failed sub-agent with a new prompt. \
         The agent continues in the same session, preserving its prior context. \
         \n\nProvider and model resolution follows the same precedence as spawn_agent: \
         (1) the optional `provider` / `model` parameters on THIS call, \
         (2) the user's config.toml `[agent]` keys `subagent_provider` / `subagent_model`, \
         (3) the parent session's provider. Resuming with a different model is useful when \
         the original spawn used a cheap/fast model for a draft and the resume should \
         escalate to a stronger model for a fix-up pass."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "The ID of the sub-agent to resume"
                },
                "prompt": {
                    "type": "string",
                    "description": "New instruction/prompt for the resumed agent"
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider override for THIS resume (e.g., 'zhipu', 'openrouter', 'custom:my-provider'). Highest precedence — overrides config.agent.subagent_provider."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for THIS resume (model id as the chosen provider accepts it). Highest precedence — overrides config.agent.subagent_model."
                }
            },
            "required": ["agent_id", "prompt"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let agent_id = input
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'agent_id' is required".into()))?;

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'prompt' is required".into()))?
            .to_string();

        // Check agent exists and is in a resumable state
        match self.manager.get_state(agent_id) {
            None => {
                return Ok(ToolResult::error(format!(
                    "No sub-agent found with id: {}",
                    agent_id
                )));
            }
            Some(SubAgentState::Running) | Some(SubAgentState::AwaitingInput) => {
                return Ok(ToolResult::error(format!(
                    "Sub-agent {} is still running. Use wait_agent first or close_agent to cancel.",
                    agent_id
                )));
            }
            Some(SubAgentState::Completed) | Some(SubAgentState::Failed(_)) => {}
            Some(SubAgentState::Cancelled) => {
                return Ok(ToolResult::error(format!(
                    "Sub-agent {} was cancelled and cannot be resumed.",
                    agent_id
                )));
            }
        }

        let session_id = self.manager.get_session_id(agent_id).ok_or_else(|| {
            ToolError::Execution(format!("No session found for sub-agent {}", agent_id))
        })?;

        let service_context = context
            .service_context
            .as_ref()
            .ok_or_else(|| ToolError::Execution("No service context available".into()))?
            .clone();

        // Create new cancel token and input channel
        let cancel_token = CancellationToken::new();
        let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();

        // Prepare the agent for resumption
        let agent_id_str = agent_id.to_string();
        if !self
            .manager
            .prepare_resume(&agent_id_str, cancel_token.clone(), input_tx)
        {
            return Ok(ToolResult::error(format!(
                "Failed to prepare sub-agent {} for resumption",
                agent_id
            )));
        }

        // Per-call provider / model overrides (issue #152). Same
        // precedence as spawn_agent: per-call > config > parent.
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

        // Build a new AgentService for the resumed run
        let config = crate::config::Config::load()
            .map_err(|e| ToolError::Execution(format!("Config load failed: {}", e)))?;
        let subagent_model = call_model
            .clone()
            .or_else(|| config.agent.subagent_model.clone());
        let effective_provider_name = call_provider
            .clone()
            .or_else(|| config.agent.subagent_provider.clone());

        let child_service = {
            let provider = if let Some(ref provider_name) = effective_provider_name {
                match crate::brain::provider::create_provider_by_name(&config, provider_name).await
                {
                    Ok(p) => {
                        let source =
                            if call_provider.is_some() { "per-call" } else { "config" };
                        tracing::info!(
                            "Resumed sub-agent using {source} provider '{provider_name}'"
                        );
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

            // Resumed agents get General type (full parent tools minus recursive/dangerous)
            let child_registry =
                super::agent_type::AgentType::General.build_registry(&self.parent_registry);

            Arc::new(
                crate::brain::agent::AgentService::new(provider, service_context, &config)
                    .await
                    .with_tool_registry(Arc::new(child_registry))
                    .with_auto_approve_tools(true)
                    .with_working_directory(context.working_dir()),
            )
        };

        // Spawn resumed task with input loop
        let cancel_clone = cancel_token.clone();
        let manager = self.manager.clone();
        let agent_id_clone = agent_id_str.clone();
        let prompt_clone = prompt.clone();
        let model_override = subagent_model;
        let mut input_rx = input_rx;

        let handle = tokio::spawn(async move {
            tracing::info!("Sub-agent {} resuming: {}", agent_id_clone, prompt_clone);

            let mut current_prompt = prompt_clone;

            // Run prompt → wait for input → run again loop
            let final_output = loop {
                let result = child_service
                    .send_message_with_tools_and_mode(
                        session_id,
                        current_prompt,
                        model_override.clone(),
                        Some(cancel_clone.clone()),
                    )
                    .await;

                match result {
                    Ok(response) => {
                        manager.update_output(&agent_id_clone, response.content.clone());
                        // Flip to AwaitingInput so wait_agent can observe
                        // round-boundary progress — same pattern as
                        // spawn.rs / team/create.rs. Without this the
                        // resumed agent's task never terminates at a
                        // round (only on input/cancel), so the old
                        // `handle.await` in wait.rs always hit its
                        // timeout_secs and the LLM gave up the turn.
                        manager.mark_awaiting_input(&agent_id_clone);
                        tracing::info!(
                            "Sub-agent {} round complete, waiting for input",
                            agent_id_clone
                        );

                        let next = tokio::select! {
                            msg = input_rx.recv() => msg,
                            _ = cancel_clone.cancelled() => {
                                tracing::info!(
                                    "Sub-agent {} cancelled while waiting for input",
                                    agent_id_clone
                                );
                                None
                            }
                        };

                        match next {
                            Some(text) => {
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
                        tracing::error!("Sub-agent {} resumed and failed: {}", agent_id_clone, e);
                        manager.mark_failed(&agent_id_clone, e.to_string());
                        return;
                    }
                }
            };

            manager.mark_completed(&agent_id_clone, final_output);
        });

        self.manager.set_join_handle(&agent_id_str, handle);

        Ok(ToolResult::success(format!(
            "Resumed sub-agent {} with new prompt:\n{}",
            agent_id, prompt
        )))
    }
}
