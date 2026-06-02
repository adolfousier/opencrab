//! Dynamic tool types and executor.
//!
//! `DynamicToolDef` is the TOML-serializable definition.
//! `DynamicTool` wraps a definition and implements the `Tool` trait.

use crate::brain::tools::error::Result;
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Executor type for a dynamic tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorType {
    Http,
    Shell,
}

/// What to do with a parameter value when it lands in one of the
/// edge-case shapes (`null`, empty array, empty string). Configured
/// per-param in `tools.toml` so the same call can hand off cleanly to
/// servers that disagree on what "absent" means (issue #95).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoerceAction {
    /// Pass the value through as-is. Default; preserves the
    /// pre-coercion behaviour for every existing tools.toml entry.
    #[default]
    Keep,
    /// Drop the key entirely. For HTTP this means it does not appear
    /// in the JSON body. For shell, the `{{#name}}…{{/name}}` block
    /// that wraps the parameter is collapsed away.
    Omit,
    /// Replace the value with an explicit JSON `null`. Useful when the
    /// downstream server expects `null` rather than an empty array.
    Null,
    /// Reject the call before it leaves the tool. Returns an error to
    /// the agent so it can adjust.
    Error,
}

/// Parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    #[serde(rename = "type", default = "default_string_type")]
    pub param_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    /// What to do when the resolved value is an empty container
    /// (empty array, empty object, empty string). Default `Keep`.
    #[serde(default)]
    pub coerce_empty_to: CoerceAction,
    /// What to do when the resolved value is JSON `null`. Default
    /// `Keep`.
    #[serde(default)]
    pub coerce_null_to: CoerceAction,
}

/// A single dynamic tool definition as parsed from tools.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolDef {
    pub name: String,
    pub description: String,
    pub executor: ExecutorType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub requires_approval: bool,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamDef>,
}

fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    30
}
fn default_string_type() -> String {
    "string".to_string()
}

impl DynamicToolDef {
    pub fn input_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for param in &self.params {
            let mut prop = serde_json::Map::new();
            prop.insert("type".into(), Value::String(param.param_type.clone()));
            if !param.description.is_empty() {
                prop.insert(
                    "description".into(),
                    Value::String(param.description.clone()),
                );
            }
            if let Some(ref default) = param.default {
                prop.insert("default".into(), default.clone());
            }
            properties.insert(param.name.clone(), Value::Object(prop));
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    /// Render `template` by substituting `{{name}}` placeholders with
    /// the matching value from `params`. Also expands mustache-style
    /// conditional sections `{{#name}}…{{/name}}`: when `name` is
    /// present in `params` the section's body is rendered (with its
    /// inner `{{name}}` substituted); when it's absent the entire
    /// section, including its enclosing tags, is dropped. This lets
    /// `coerce_empty_to = "omit"` cleanly remove a CLI flag like
    /// `{{#ids}}--ids {{ids}}{{/ids}}` instead of leaving a dangling
    /// `--ids ` with no value.
    pub fn render_template(template: &str, params: &Value) -> String {
        let obj = params.as_object();

        // Section pass first. Single-pass left-to-right scan so nested
        // tags collapse predictably even if a future caller writes
        // overlapping sections (which we still reject by silently
        // leaving the malformed bytes alone).
        let mut after_sections = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open_at) = rest.find("{{#") {
            after_sections.push_str(&rest[..open_at]);
            let after_open = &rest[open_at + 3..];
            let Some(name_end) = after_open.find("}}") else {
                // Malformed: no closing `}}` for the open tag. Bail
                // out and let the rest of the template pass through
                // untouched so the error is at least visible.
                after_sections.push_str(&rest[open_at..]);
                rest = "";
                break;
            };
            let name = &after_open[..name_end];
            let body_start = name_end + 2;
            let close_tag = format!("{{{{/{}}}}}", name);
            let after_name = &after_open[body_start..];
            let Some(close_at) = after_name.find(&close_tag) else {
                // Malformed section. Emit as-is.
                after_sections.push_str(&rest[open_at..]);
                rest = "";
                break;
            };
            let body = &after_name[..close_at];
            let present = obj.is_some_and(|o| o.contains_key(name));
            if present {
                after_sections.push_str(body);
            }
            rest = &after_name[close_at + close_tag.len()..];
        }
        after_sections.push_str(rest);

        // Then the standard `{{name}}` substitution pass over the
        // section-resolved template.
        let mut result = after_sections;
        if let Some(obj) = obj {
            for (key, value) in obj {
                let placeholder = format!("{{{{{}}}}}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }
        result
    }

    /// Escape string values in params for use in single-quoted shell
    /// arguments. Replaces each `'` with `'\''` (end single-quote,
    /// escaped single-quote, resume single-quote) — the standard POSIX
    /// shell idiom for embedding a single quote inside a single-quoted
    /// string.
    ///
    /// Non-string values (numbers, booleans, arrays, objects) pass
    /// through unchanged — they are already safe for shell usage since
    /// `render_template` converts them with `to_string()`.
    pub fn shell_escape_params(params: &Value) -> Value {
        match params {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), Self::shell_escape_params(v));
                }
                Value::Object(out)
            }
            Value::String(s) => Value::String(s.replace('\'', "'\\''")),
            other => other.clone(),
        }
    }
}

/// Top-level tools.toml structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DynamicToolsConfig {
    #[serde(default)]
    pub tools: Vec<DynamicToolDef>,
}

/// Runtime tool wrapping a TOML definition.
pub struct DynamicTool {
    def: DynamicToolDef,
}

impl DynamicTool {
    pub fn new(def: DynamicToolDef) -> Self {
        Self { def }
    }

    fn extract_params(&self, input: &Value) -> Value {
        let mut out = serde_json::Map::new();
        let obj = input.as_object();
        for p in &self.def.params {
            let val = obj
                .and_then(|o| o.get(&p.name))
                .cloned()
                .or_else(|| p.default.clone());
            if let Some(v) = val {
                out.insert(p.name.clone(), v);
            }
        }
        Value::Object(out)
    }

    /// Apply per-parameter `coerce_empty_to` / `coerce_null_to` rules
    /// (issue #95) to the extracted params. Returns `Ok(params)` with
    /// the coerced map, or `Err(message)` when any param had its
    /// `Error` rule triggered. `Omit` removes the key; `Null` replaces
    /// the value with `Value::Null`; `Keep` is the no-op default.
    fn coerce_params(&self, params: Value) -> std::result::Result<Value, String> {
        let mut map = match params {
            Value::Object(m) => m,
            other => return Ok(other),
        };

        // Drop the action enum into a small Vec of decisions so we
        // mutate the map after walking the param defs.
        for p in &self.def.params {
            let Some(v) = map.get(&p.name) else { continue };

            let is_null = matches!(v, Value::Null);
            let is_empty = match v {
                Value::String(s) => s.is_empty(),
                Value::Array(a) => a.is_empty(),
                Value::Object(o) => o.is_empty(),
                _ => false,
            };

            let action = if is_null {
                p.coerce_null_to
            } else if is_empty {
                p.coerce_empty_to
            } else {
                CoerceAction::Keep
            };

            match action {
                CoerceAction::Keep => {}
                CoerceAction::Omit => {
                    map.remove(&p.name);
                }
                CoerceAction::Null => {
                    map.insert(p.name.clone(), Value::Null);
                }
                CoerceAction::Error => {
                    let shape = if is_null { "null" } else { "empty" };
                    return Err(format!(
                        "Parameter '{}' is {} and the tool config rejects this shape \
                         (coerce_{}_to = \"error\"). Adjust the call or change the rule.",
                        p.name,
                        shape,
                        if is_null { "null" } else { "empty" }
                    ));
                }
            }
        }

        Ok(Value::Object(map))
    }

    async fn execute_http(&self, params: &Value) -> Result<ToolResult> {
        let url = match &self.def.url {
            Some(u) => DynamicToolDef::render_template(u, params),
            None => return Ok(ToolResult::error("HTTP tool missing 'url' field".into())),
        };
        let method = self.def.method.as_deref().unwrap_or("GET").to_uppercase();
        let client = reqwest::Client::new();
        let mut req = match method.as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            "DELETE" => client.delete(&url),
            _ => client.get(&url),
        };
        for (k, v) in &self.def.headers {
            let rendered = DynamicToolDef::render_template(v, params);
            req = req.header(k.as_str(), rendered);
        }
        let timeout = std::time::Duration::from_secs(self.def.timeout_secs);
        match req.timeout(timeout).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    Ok(ToolResult::success(body))
                } else {
                    Ok(ToolResult::error(format!(
                        "HTTP {} {}: {}",
                        status.as_u16(),
                        status.canonical_reason().unwrap_or(""),
                        body
                    )))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("HTTP request failed: {e}"))),
        }
    }

    async fn execute_shell(
        &self,
        params: &Value,
        context: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        // Shell-escape string parameter values so single quotes in
        // values don't break single-quoted shell arguments like
        // `--string 'message={{message}}'`.
        let escaped_params = DynamicToolDef::shell_escape_params(params);
        let cmd = match &self.def.command {
            Some(c) => DynamicToolDef::render_template(c, &escaped_params),
            None => {
                return Ok(ToolResult::error(
                    "Shell tool missing 'command' field".into(),
                ));
            }
        };
        // Detach stdin from the parent TTY so mouse-capture bytes don't
        // leak into captured stdout (same TUI-bleed issue as bash.rs).
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(context.working_dir())
            .stdin(std::process::Stdio::null())
            .output()
            .await;
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    let mut result = stdout;
                    if !stderr.is_empty() {
                        result.push_str("\n[stderr] ");
                        result.push_str(&stderr);
                    }
                    Ok(ToolResult::success(result))
                } else {
                    Ok(ToolResult::error(format!(
                        "Exit code {}: {}{}",
                        out.status.code().unwrap_or(-1),
                        stdout,
                        if stderr.is_empty() {
                            String::new()
                        } else {
                            format!("\n[stderr] {stderr}")
                        }
                    )))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to spawn shell: {e}"))),
        }
    }
}

#[async_trait]
impl Tool for DynamicTool {
    fn name(&self) -> &str {
        &self.def.name
    }
    fn description(&self) -> &str {
        &self.def.description
    }
    fn input_schema(&self) -> Value {
        self.def.input_schema()
    }
    fn capabilities(&self) -> Vec<ToolCapability> {
        match self.def.executor {
            ExecutorType::Http => vec![ToolCapability::Network],
            ExecutorType::Shell => vec![ToolCapability::ExecuteShell],
        }
    }
    fn requires_approval(&self) -> bool {
        self.def.requires_approval
    }
    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let raw_params = self.extract_params(&input);
        let params = match self.coerce_params(raw_params) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };
        tracing::info!(
            "Executing dynamic tool '{}' ({:?})",
            self.def.name,
            self.def.executor
        );
        match self.def.executor {
            ExecutorType::Http => self.execute_http(&params).await,
            ExecutorType::Shell => self.execute_shell(&params, context).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_shell(name: &str, cmd: &str, params: Vec<ParamDef>) -> DynamicTool {
        DynamicTool::new(DynamicToolDef {
            name: name.into(),
            description: format!("Test: {name}"),
            executor: ExecutorType::Shell,
            enabled: true,
            requires_approval: false,
            method: None,
            url: None,
            headers: HashMap::new(),
            timeout_secs: 10,
            command: Some(cmd.into()),
            params,
        })
    }

    fn ctx() -> ToolExecutionContext {
        ToolExecutionContext::new(Uuid::new_v4())
    }

    #[test]
    fn test_name() {
        assert_eq!(make_shell("t", "echo", vec![]).name(), "t");
    }

    #[test]
    fn test_capabilities() {
        assert_eq!(
            make_shell("s", "echo", vec![]).capabilities(),
            vec![ToolCapability::ExecuteShell]
        );
    }

    #[test]
    fn test_input_schema() {
        let tool = make_shell(
            "echo",
            "echo {{msg}}",
            vec![ParamDef {
                name: "msg".into(),
                param_type: "string".into(),
                description: "Msg".into(),
                required: true,
                default: None,
                coerce_empty_to: Default::default(),
                coerce_null_to: Default::default(),
            }],
        );
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"][0], "msg");
    }

    #[test]
    fn test_extract_params_with_defaults() {
        let tool = make_shell(
            "echo",
            "echo {{msg}} {{count}}",
            vec![
                ParamDef {
                    name: "msg".into(),
                    param_type: "string".into(),
                    description: "".into(),
                    required: true,
                    default: None,
                    coerce_empty_to: Default::default(),
                    coerce_null_to: Default::default(),
                },
                ParamDef {
                    name: "count".into(),
                    param_type: "integer".into(),
                    description: "".into(),
                    required: false,
                    default: Some(serde_json::json!(3)),
                    coerce_empty_to: Default::default(),
                    coerce_null_to: Default::default(),
                },
            ],
        );
        let params = tool.extract_params(&serde_json::json!({"msg": "hello"}));
        assert_eq!(params["msg"], "hello");
        assert_eq!(params["count"], 3);
    }

    #[test]
    fn test_template_rendering() {
        let result = DynamicToolDef::render_template(
            "deploy {{branch}} x{{count}}",
            &serde_json::json!({"branch": "main", "count": 3}),
        );
        assert_eq!(result, "deploy main x3");
    }

    #[test]
    fn test_parse_toml() {
        let config: DynamicToolsConfig = toml::from_str(
            r#"
[[tools]]
name = "check"
description = "Check health"
executor = "http"
method = "GET"
url = "https://example.com/health"
"#,
        )
        .unwrap();
        assert_eq!(config.tools.len(), 1);
        assert_eq!(config.tools[0].executor, ExecutorType::Http);
    }

    #[test]
    fn test_roundtrip_toml() {
        let config = DynamicToolsConfig {
            tools: vec![DynamicToolDef {
                name: "ping".into(),
                description: "Ping".into(),
                executor: ExecutorType::Shell,
                enabled: true,
                requires_approval: false,
                method: None,
                url: None,
                headers: HashMap::new(),
                timeout_secs: 30,
                command: Some("ping -c 1 {{host}}".into()),
                params: vec![ParamDef {
                    name: "host".into(),
                    param_type: "string".into(),
                    description: "".into(),
                    required: true,
                    default: None,
                    coerce_empty_to: Default::default(),
                    coerce_null_to: Default::default(),
                }],
            }],
        };
        let content = toml::to_string_pretty(&config).unwrap();
        let loaded: DynamicToolsConfig = toml::from_str(&content).unwrap();
        assert_eq!(loaded.tools[0].name, "ping");
    }

    #[tokio::test]
    async fn test_execute_shell_echo() {
        let tool = make_shell("echo_test", "echo hello", vec![]);
        let result = tool.execute(serde_json::json!({}), &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn test_execute_shell_failure() {
        let result = make_shell("fail", "exit 42", vec![])
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_missing_command() {
        let t = DynamicTool::new(DynamicToolDef {
            name: "b".into(),
            description: "".into(),
            executor: ExecutorType::Shell,
            enabled: true,
            requires_approval: false,
            method: None,
            url: None,
            headers: HashMap::new(),
            timeout_secs: 5,
            command: None,
            params: vec![],
        });
        let result = t.execute(serde_json::json!({}), &ctx()).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_missing_url() {
        let t = DynamicTool::new(DynamicToolDef {
            name: "h".into(),
            description: "".into(),
            executor: ExecutorType::Http,
            enabled: true,
            requires_approval: false,
            method: None,
            url: None,
            headers: HashMap::new(),
            timeout_secs: 5,
            command: None,
            params: vec![],
        });
        let result = t.execute(serde_json::json!({}), &ctx()).await.unwrap();
        assert!(!result.success);
    }

    #[test]
    fn test_shell_escape_params_noop() {
        // No single quotes — values pass through unchanged
        let params = serde_json::json!({"msg": "hello world"});
        let escaped = DynamicToolDef::shell_escape_params(&params);
        assert_eq!(escaped["msg"], "hello world");
    }

    #[test]
    fn test_shell_escape_params_single_quote() {
        // Single quote in value gets escaped
        let params = serde_json::json!({"msg": "it's nice"});
        let escaped = DynamicToolDef::shell_escape_params(&params);
        assert_eq!(escaped["msg"], "it'\\''s nice");
    }

    #[test]
    fn test_shell_escape_params_multiple_quotes() {
        // Multiple single quotes
        let params = serde_json::json!({"msg": "'a' 'b'"});
        let escaped = DynamicToolDef::shell_escape_params(&params);
        assert_eq!(escaped["msg"], "'\\''a'\\'' '\\''b'\\''");
    }

    #[test]
    fn test_shell_escape_params_nested() {
        // Nested object values are also escaped
        let params = serde_json::json!({"outer": {"inner": "it's nested"}});
        let escaped = DynamicToolDef::shell_escape_params(&params);
        assert_eq!(escaped["outer"]["inner"], "it'\\''s nested");
    }

    #[test]
    fn test_shell_escape_params_non_string() {
        // Numbers, booleans, null pass through unchanged
        let params = serde_json::json!({"n": 42, "b": true, "x": null});
        let escaped = DynamicToolDef::shell_escape_params(&params);
        assert_eq!(escaped["n"], 42);
        assert_eq!(escaped["b"], true);
        assert_eq!(escaped["x"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_execute_shell_with_single_quote() {
        // Message with single quote — escaped params prevent shell breakage
        let result = make_shell(
            "echo_test",
            "echo 'msg={{msg}}'",
            vec![ParamDef {
                name: "msg".into(),
                param_type: "string".into(),
                description: "".into(),
                required: true,
                default: None,
                coerce_empty_to: Default::default(),
                coerce_null_to: Default::default(),
            }],
        )
        .execute(serde_json::json!({"msg": "it's nice"}), &ctx())
        .await
        .unwrap();
        assert!(result.success);
        assert!(result.output.contains("msg=it's nice"));
    }

    #[tokio::test]
    async fn test_execute_shell_newlines() {
        // Multi-line message — newlines survive single-quoted shell arg
        let result = make_shell(
            "echo_test",
            "echo 'msg={{msg}}'",
            vec![ParamDef {
                name: "msg".into(),
                param_type: "string".into(),
                description: "".into(),
                required: true,
                default: None,
                coerce_empty_to: Default::default(),
                coerce_null_to: Default::default(),
            }],
        )
        .execute(
            serde_json::json!({"msg": "line1\nline2"}),
            &ctx(),
        )
        .await
        .unwrap();
        assert!(result.success);
        assert!(result.output.contains("line1\nline2") || result.output.contains("line1\nline2"));
    }
}
