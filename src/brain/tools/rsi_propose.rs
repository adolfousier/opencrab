//! RSI-side proposal authoring tool.
//!
//! Called only by the autonomous RSI agent. Writes a tool or command
//! proposal to the inbox in `~/.opencrabs/rsi/`. Never installs anything
//! live — that's the user-facing `rsi_proposals` tool's job.
//!
//! This split is deliberate: RSI's restricted tool whitelist must not
//! include `tool_manage` / `config_manager` directly, because a
//! hallucinated shell tool installed at 3am has a much larger blast
//! radius than a hallucinated paragraph of prose. By forcing every RSI
//! creation through an inbox, the user always gets to see (and the
//! agent always gets a chance to triage) what RSI wants to add.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::commands::UserCommand;
use crate::brain::rsi_proposals::{ProposalsStore, ProposedSkill};
use crate::brain::tools::dynamic::tool::{DynamicToolDef, ExecutorType, ParamDef};
use async_trait::async_trait;
use serde_json::Value;

pub struct RsiProposeTool;

#[async_trait]
impl Tool for RsiProposeTool {
    fn name(&self) -> &str {
        "rsi_propose"
    }

    fn description(&self) -> &str {
        "Propose a new dynamic tool, slash command, or skill for the user to install. \
         Proposals are written to your `rsi/` inbox — they DO NOT \
         install live. The user (or the user-facing agent on their behalf) reviews \
         and applies proposals via rsi_proposals. \
         \n\nWHEN TO USE EACH KIND: \
         \n- `tool` for a well-scoped, parameterised invocation worth wrapping with a schema \
         (e.g. an HTTP API call, a shell command with named params like `docker logs <container>`). \
         \n- `command` for a slash-trigger that runs a fixed prompt (e.g. `/standup` that asks \
         the agent for a 3-bullet summary of yesterday's commits). \
         \n- `skill` for a multi-step workflow the agent should follow (e.g. \"release pipeline\" \
         that covers branch check → changelog draft → tag → publish). Skills are cheaper to \
         author than tools (no schema, no executor wiring) and a natural fit when the pattern \
         RSI observed is a SEQUENCE of existing tool calls rather than one missing primitive."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["tool", "command", "skill"],
                    "description": "What to propose: a dynamic tool (tools.toml), a slash command (commands.toml), or a skill (skills/<name>/SKILL.md in your OpenCrabs home)"
                },
                "rationale": {
                    "type": "string",
                    "description": "Why this proposal exists: cite the feedback evidence (failure rate, recurring user pattern) that motivated it. Shown to the user before they apply."
                },
                "name": {
                    "type": "string",
                    "description": "Tool name (snake_case, no spaces) for kind=tool, or slash command name (with leading /) for kind=command"
                },
                "description": {
                    "type": "string",
                    "description": "Short user-facing description"
                },
                "executor_type": {
                    "type": "string",
                    "enum": ["http", "shell"],
                    "description": "(tool only) Executor: http for API calls, shell for CLI invocations"
                },
                "command": {
                    "type": "string",
                    "description": "(tool, executor_type=shell) The command line to run. Template variables {{param}} are substituted."
                },
                "method": {
                    "type": "string",
                    "description": "(tool, executor_type=http) HTTP method, e.g. GET / POST"
                },
                "url": {
                    "type": "string",
                    "description": "(tool, executor_type=http) Target URL. Template variables {{param}} are substituted."
                },
                "headers": {
                    "type": "object",
                    "description": "(tool, executor_type=http) HTTP headers as a key/value object"
                },
                "params": {
                    "type": "array",
                    "description": "(tool) Tool input parameters. Each entry: {name, type, description, required, default}",
                    "items": { "type": "object" }
                },
                "requires_approval": {
                    "type": "boolean",
                    "description": "(tool) Whether the user-facing agent should be prompted before each invocation. Defaults to true — keep it true for shell tools.",
                    "default": true
                },
                "enabled": {
                    "type": "boolean",
                    "description": "(tool) Whether the tool is enabled when applied. Defaults to true.",
                    "default": true
                },
                "prompt": {
                    "type": "string",
                    "description": "(command only) The prompt sent to the LLM when the user types /name"
                },
                "action": {
                    "type": "string",
                    "enum": ["prompt", "system"],
                    "description": "(command only) Command kind: 'prompt' sends to LLM, 'system' displays inline. Defaults to 'prompt'.",
                    "default": "prompt"
                },
                "body": {
                    "type": "string",
                    "description": "(skill only) Full markdown body of the SKILL.md file. Goes verbatim below the YAML frontmatter the user writes on apply. May span multiple steps, include shell snippets, code blocks, and explicit references to other tools (e.g. 'call bash with X', 'call parse_document with the saved path')."
                }
            },
            "required": ["kind", "rationale", "name", "description"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Pure file-write into the inbox under ~/.opencrabs/rsi/.
        // No shell exec, no network, no installation.
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let kind = input
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let rationale = input
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if rationale.is_empty() {
            return Ok(ToolResult::error(
                "rationale is required — cite the feedback evidence that motivated this proposal"
                    .to_string(),
            ));
        }

        // Efficiency gate: rationale must explicitly state which applies.
        // Pure passthrough wrappers (same token cost, no error reduction, no new capability)
        // fail this gate and should not be proposed.
        let rationale_lower = rationale.to_lowercase();
        let has_efficiency_gate = rationale_lower.contains("token savings")
            || rationale_lower.contains("token saving")
            || rationale_lower.contains("error reduction")
            || rationale_lower.contains("capability addition")
            || rationale_lower.contains("boilerplate")
            || rationale_lower.contains("quoting")
            || rationale_lower.contains("escaping")
            || rationale_lower.contains("validation")
            || rationale_lower.contains("structured output")
            || rationale_lower.contains("protocol handling");

        if kind == "tool" && !has_efficiency_gate {
            return Ok(ToolResult::error(
                "rationale must explicitly state which efficiency gate applies: \
                 (1) TOKEN SAVINGS — eliminates boilerplate, (2) ERROR REDUCTION — prevents \
                 known failures, or (3) CAPABILITY ADDITION — enables something bash cannot do. \
                 Pure passthrough wrappers fail this gate."
                    .to_string(),
            ));
        }

        if name.is_empty() || description.is_empty() {
            return Ok(ToolResult::error(
                "name and description are required".to_string(),
            ));
        }

        let store = ProposalsStore::new();
        let proposer = "rsi-autonomous";

        match kind.as_str() {
            "tool" => {
                let executor_type_str = input
                    .get("executor_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let executor = match executor_type_str {
                    "http" => ExecutorType::Http,
                    "shell" => ExecutorType::Shell,
                    other => {
                        return Ok(ToolResult::error(format!(
                            "executor_type must be 'http' or 'shell', got '{other}'"
                        )));
                    }
                };

                let params: Vec<ParamDef> = match input.get("params") {
                    Some(v) if !v.is_null() => match serde_json::from_value(v.clone()) {
                        Ok(p) => p,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "params must be an array of {{name, type, description, required, default}}: {e}"
                            )));
                        }
                    },
                    _ => Vec::new(),
                };

                let headers = input
                    .get("headers")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();

                let def = DynamicToolDef {
                    name: name.clone(),
                    description: description.clone(),
                    executor,
                    enabled: input
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    requires_approval: input
                        .get("requires_approval")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    method: input
                        .get("method")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    url: input
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    headers,
                    timeout_secs: 30,
                    command: input
                        .get("command")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    params,
                };

                // Sanity: shell tools need a command, http tools need a url.
                if matches!(def.executor, ExecutorType::Shell) && def.command.is_none() {
                    return Ok(ToolResult::error(
                        "shell tools require a 'command' field".to_string(),
                    ));
                }
                if matches!(def.executor, ExecutorType::Http) && def.url.is_none() {
                    return Ok(ToolResult::error(
                        "http tools require a 'url' field".to_string(),
                    ));
                }

                match store.add_tool_proposal(proposer, &rationale, def) {
                    Ok(id) => Ok(ToolResult::success(format!(
                        "Tool proposal filed: {id} (name={name}). User will see a banner on next session start, or can list with rsi_proposals."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to write proposal: {e}"))),
                }
            }
            "command" => {
                let prompt = input
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if prompt.is_empty() {
                    return Ok(ToolResult::error(
                        "prompt is required for command proposals".to_string(),
                    ));
                }

                let action = input
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("prompt")
                    .to_string();

                // Normalise the slash: commands.toml stores them with the
                // leading "/" by convention.
                let normalised_name = if name.starts_with('/') {
                    name.clone()
                } else {
                    format!("/{}", name)
                };

                let cmd = UserCommand {
                    name: normalised_name.clone(),
                    description,
                    action,
                    prompt,
                };

                match store.add_command_proposal(proposer, &rationale, cmd) {
                    Ok(id) => Ok(ToolResult::success(format!(
                        "Command proposal filed: {id} (name={normalised_name})."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to write proposal: {e}"))),
                }
            }
            "skill" => {
                let body = input
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if body.is_empty() {
                    return Ok(ToolResult::error(
                        "body is required for skill proposals — the multi-step \
                         workflow markdown that goes into SKILL.md below the \
                         YAML frontmatter"
                            .to_string(),
                    ));
                }
                // Normalise the skill slug — strip any leading slash the
                // model emitted by analogy with command names, since
                // skills are stored under a directory not a slash prefix.
                let normalised_name = name.trim_start_matches('/').to_string();
                if normalised_name.is_empty()
                    || normalised_name
                        .chars()
                        .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                {
                    return Ok(ToolResult::error(format!(
                        "skill name must be alphanumeric / dashes / underscores (got '{name}')"
                    )));
                }

                let skill = ProposedSkill {
                    name: normalised_name.clone(),
                    description: description.clone(),
                    body,
                };

                match store.add_skill_proposal(proposer, &rationale, skill) {
                    Ok(id) => Ok(ToolResult::success(format!(
                        "Skill proposal filed: {id} (name={normalised_name}). On apply it lands at skills/{normalised_name}/SKILL.md in your OpenCrabs home."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to write proposal: {e}"))),
                }
            }
            other => Ok(ToolResult::error(format!(
                "kind must be 'tool', 'command', or 'skill', got '{other}'"
            ))),
        }
    }
}
