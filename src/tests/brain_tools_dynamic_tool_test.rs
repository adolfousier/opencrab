use crate::brain::tools::Tool;
use crate::brain::tools::ToolCapability;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::dynamic::tool::*;
use std::collections::HashMap;
use tokio;
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
    .execute(serde_json::json!({"msg": "line1\nline2"}), &ctx())
    .await
    .unwrap();
    assert!(result.success);
    assert!(result.output.contains("line1\nline2") || result.output.contains("line1\nline2"));
}

// ── render_shell_command: context-aware escaping (#523) ──────────────
// shell_escape_params only escaped single quotes, so a value in a DOUBLE-
// quoted argument (the command_code template `-p "{{prompt}}"`) left $,
// backtick and \ shell-interpreted (44% failure). render_shell_command
// escapes per quoting context; single-quoted/unquoted stay byte-identical.

#[test]
fn shell_double_quoted_escapes_dollar_backtick_backslash() {
    let out = DynamicToolDef::render_shell_command(
        r#"cmd-wrap -p "{{prompt}}" --yolo"#,
        &serde_json::json!({ "prompt": "run `date` and $VAR then a\\b" }),
    );
    assert_eq!(
        out,
        r#"cmd-wrap -p "run \`date\` and \$VAR then a\\b" --yolo"#
    );
}

#[test]
fn shell_double_quoted_escapes_embedded_quote() {
    let out = DynamicToolDef::render_shell_command(
        r#"echo "{{p}}""#,
        &serde_json::json!({ "p": "say \"hi\"" }),
    );
    assert_eq!(out, r#"echo "say \"hi\"""#);
}

#[test]
fn shell_single_quoted_matches_previous_behavior() {
    // Byte-for-byte the old shell_escape_params behavior: ' -> '\'' , and $ is
    // left ALONE (literal inside single quotes), so nothing regresses.
    let out = DynamicToolDef::render_shell_command(
        r#"echo 'm={{m}}'"#,
        &serde_json::json!({ "m": "it's $HOME" }),
    );
    assert_eq!(out, r#"echo 'm=it'\''s $HOME'"#);
}

#[test]
fn shell_unquoted_matches_previous_behavior() {
    // Unquoted context: same single-quote escaping as before (unchanged).
    let out = DynamicToolDef::render_shell_command("run {{x}}", &serde_json::json!({ "x": "a'b" }));
    assert_eq!(out, r#"run a'\''b"#);
}

#[test]
fn shell_non_string_and_unknown_placeholder() {
    // Numbers pass through; unknown placeholders are emitted verbatim.
    let out = DynamicToolDef::render_shell_command(
        "n={{count}} u={{missing}}",
        &serde_json::json!({ "count": 7 }),
    );
    assert_eq!(out, "n=7 u={{missing}}");
}

#[test]
fn shell_sections_still_resolve() {
    let present = DynamicToolDef::render_shell_command(
        "base{{#flag}} --on {{flag}}{{/flag}}",
        &serde_json::json!({ "flag": "yes" }),
    );
    assert_eq!(present, "base --on yes");
    let absent = DynamicToolDef::render_shell_command(
        "base{{#flag}} --on {{flag}}{{/flag}}",
        &serde_json::json!({}),
    );
    assert_eq!(absent, "base");
}

#[test]
fn shell_escaped_quote_in_template_stays_in_double_context() {
    // An author's \" inside the double-quoted span must not close it, so the
    // following {{p}} is still escaped for a double-quoted context.
    let out = DynamicToolDef::render_shell_command(
        r#"echo "a \" {{p}}""#,
        &serde_json::json!({ "p": "$X" }),
    );
    assert_eq!(out, r#"echo "a \" \$X""#);
}
