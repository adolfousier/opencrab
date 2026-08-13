//! team_create tool — spawn a named team of agents from a single command.

use super::manager::TeamManager;
use crate::brain::tools::error::{Result, ToolError};
use crate::brain::tools::subagent::AgentType;
use crate::brain::tools::subagent::manager::{SubAgent, SubAgentManager, SubAgentState};
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Tool that spawns a named team of sub-agents from a list of tasks.
pub struct TeamCreateTool {
    subagent_manager: Arc<SubAgentManager>,
    team_manager: Arc<TeamManager>,
    parent_registry: Arc<crate::brain::tools::ToolRegistry>,
}

impl TeamCreateTool {
    pub fn new(
        subagent_manager: Arc<SubAgentManager>,
        team_manager: Arc<TeamManager>,
        parent_registry: Arc<crate::brain::tools::ToolRegistry>,
    ) -> Self {
        Self {
            subagent_manager,
            team_manager,
            parent_registry,
        }
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a named team by spawning multiple sub-agents at once. Each agent gets its own \
         task and optional type. Returns team name and all agent IDs. \
         \n\nProvider and model resolution is PER MEMBER. Each entry in the `agents` array \
         can carry its own `provider` and `model` fields, with the same precedence as \
         spawn_agent: per-member > config.agent.subagent_* > parent. This lets one \
         team_create call spawn agents that each use a different model — useful when a \
         skill orchestrates a plan-with-GLM / code-with-Deepseek / review-with-Kimi flow \
         as one atomic team rather than chained spawn_agent calls."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Unique name for this team (e.g., 'backend-refactor', 'test-suite')"
                },
                "agents": {
                    "type": "array",
                    "description": "List of agents to spawn",
                    "items": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "Task for this agent"
                            },
                            "label": {
                                "type": "string",
                                "description": "Short label for this agent"
                            },
                            "agent_type": {
                                "type": "string",
                                "enum": ["general", "explore", "architect", "plan", "code", "research"]
                            },
                            "provider": {
                                "type": "string",
                                "description": "Optional per-member provider override (e.g., 'zhipu', 'openrouter', 'custom:my-provider'). Highest precedence — overrides config.agent.subagent_provider for THIS member only."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional per-member model override. Highest precedence — overrides config.agent.subagent_model for THIS member only."
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            },
            "required": ["team_name", "agents"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let team_name = input
            .get("team_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'team_name' is required".into()))?
            .to_string();

        let agents_array = input
            .get("agents")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::InvalidInput("'agents' must be an array".into()))?;

        if agents_array.is_empty() {
            return Err(ToolError::InvalidInput(
                "'agents' array cannot be empty".into(),
            ));
        }

        if self.team_manager.exists(&team_name) {
            return Err(ToolError::InvalidInput(format!(
                "Team '{}' already exists",
                team_name
            )));
        }

        let service_context = context
            .service_context
            .as_ref()
            .ok_or_else(|| ToolError::Execution("No service context available".into()))?
            .clone();

        let config = crate::config::Config::load()
            .map_err(|e| ToolError::Execution(format!("Config load failed: {}", e)))?;

        let mut spawned_ids = Vec::new();
        let mut spawn_results = Vec::new();

        for agent_def in agents_array {
            let prompt = agent_def
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidInput("Each agent needs a 'prompt'".into()))?
                .to_string();

            let label = agent_def
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("team-member")
                .to_string();

            let agent_type = AgentType::parse(
                agent_def
                    .get("agent_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("general"),
            );

            // Per-member provider / model overrides (issue #152). Same
            // precedence as spawn_agent: per-member > config > parent.
            // Resolved here inside the loop so each team member can
            // route to a different model in a single team_create call
            // — that's the orchestration shape the issue requested.
            let member_provider = agent_def
                .get("provider")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let member_model = agent_def
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let model_override = member_model
                .clone()
                .or_else(|| config.agent.subagent_model.clone());
            let effective_provider_name = member_provider
                .clone()
                .or_else(|| config.agent.subagent_provider.clone());

            // Create session for this agent
            let session_service = crate::services::SessionService::new(service_context.clone());
            let child_session = session_service
                .create_session(Some(format!("team:{}/{}", team_name, label)))
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to create session: {}", e)))?;

            let child_session_id = child_session.id;
            let agent_id = SubAgentManager::generate_id();

            let cancel_token = CancellationToken::new();
            let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();

            // Create provider — per-member override wins over config.
            let provider = if let Some(ref provider_name) = effective_provider_name {
                match crate::brain::provider::create_provider_by_name(&config, provider_name).await
                {
                    Ok(p) => {
                        let source = if member_provider.is_some() {
                            "per-member"
                        } else {
                            "config"
                        };
                        tracing::info!(
                            "Team member '{label}' using {source} provider '{provider_name}'"
                        );
                        p
                    }
                    Err(_) => crate::brain::provider::create_provider(&config)
                        .await
                        .map_err(|e| {
                            ToolError::Execution(format!(
                                "Fallback provider creation failed: {}",
                                e
                            ))
                        })?,
                }
            } else {
                crate::brain::provider::create_provider(&config)
                    .await
                    .map_err(|e| ToolError::Execution(format!("Provider creation failed: {}", e)))?
            };

            let child_registry = agent_type.build_registry(&self.parent_registry);

            let child_service = Arc::new(
                crate::brain::agent::AgentService::new(provider, service_context.clone(), &config)
                    .await
                    .with_tool_registry(Arc::new(child_registry))
                    .with_auto_approve_tools(true)
                    .with_working_directory(context.working_dir()),
            );

            let full_prompt = format!("{}\n\n{}", agent_type.system_prompt(), prompt);

            let cancel_clone = cancel_token.clone();
            let manager = self.subagent_manager.clone();
            let agent_id_clone = agent_id.clone();
            let model_clone = model_override.clone();

            let handle = tokio::spawn(async move {
                tracing::info!("Team agent {} starting", agent_id_clone);

                let mut current_prompt = full_prompt;

                let final_output = loop {
                    let result = child_service
                        .send_message_with_tools_and_mode(
                            child_session_id,
                            current_prompt,
                            model_clone.clone(),
                            Some(cancel_clone.clone()),
                        )
                        .await;

                    match result {
                        Ok(response) => {
                            manager.update_output(&agent_id_clone, response.content.clone());
                            // Flip to AwaitingInput so wait_agent can observe
                            // round-boundary progress (same pattern as spawn.rs).
                            manager.mark_awaiting_input(&agent_id_clone);
                            tracing::info!(
                                "Team agent {} round complete, waiting for input",
                                agent_id_clone
                            );

                            let next = tokio::select! {
                                msg = input_rx.recv() => msg,
                                _ = cancel_clone.cancelled() => {
                                    tracing::info!("Team agent {} cancelled while waiting for input", agent_id_clone);
                                    None
                                }
                            };

                            match next {
                                Some(text) => current_prompt = text,
                                None => break response.content,
                            }
                        }
                        Err(e) => {
                            tracing::error!("Team agent {} failed: {}", agent_id_clone, e);
                            manager.mark_failed(&agent_id_clone, e.to_string());
                            return;
                        }
                    }
                };

                manager.mark_completed(&agent_id_clone, final_output);
            });

            // Register in subagent manager
            self.subagent_manager.insert(SubAgent {
                id: agent_id.clone(),
                label: label.clone(),
                session_id: child_session_id,
                state: SubAgentState::Running,
                cancel_token,
                join_handle: Some(handle),
                input_tx: Some(input_tx),
                output: None,
                spawned_at: chrono::Utc::now(),
                waiters: 0,
            });

            spawned_ids.push(agent_id.clone());
            spawn_results.push(format!(
                "  {} ({}) → {}",
                label,
                agent_type.label(),
                agent_id
            ));
        }

        // Register team
        self.team_manager
            .create_team(team_name.clone(), spawned_ids.clone());

        Ok(ToolResult::success(format!(
            "Created team '{}' with {} agents:\n{}",
            team_name,
            spawned_ids.len(),
            spawn_results.join("\n")
        )))
    }
}
