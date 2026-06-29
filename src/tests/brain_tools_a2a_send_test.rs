use super::*;
use crate::brain::tools::{Tool, ToolExecutionContext};
use tokio;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(uuid::Uuid::new_v4())
}

#[test]
fn test_tool_name_and_schema() {
    let tool = A2aSendTool::new();
    assert_eq!(tool.name(), "a2a_send");
    let schema = tool.input_schema();
    let props = schema["properties"].as_object().unwrap();
    assert!(props.contains_key("action"));
    assert!(props.contains_key("url"));
    assert!(props.contains_key("message"));
    assert!(props.contains_key("task_id"));
    assert!(props.contains_key("api_key"));
}

#[test]
fn test_discover_does_not_require_approval() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "discover", "url": "http://localhost:18790"});
    assert!(!tool.requires_approval_for_input(&input));
}

#[test]
fn test_send_requires_approval() {
    let tool = A2aSendTool::new();
    let input =
        serde_json::json!({"action": "send", "url": "http://localhost:18790", "message": "hi"});
    assert!(tool.requires_approval_for_input(&input));
}

#[test]
fn test_cancel_requires_approval() {
    let tool = A2aSendTool::new();
    let input =
        serde_json::json!({"action": "cancel", "url": "http://localhost:18790", "task_id": "abc"});
    assert!(tool.requires_approval_for_input(&input));
}

#[tokio::test]
async fn test_missing_url() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "discover"});
    let result = tool.execute(input, &ctx()).await.unwrap();
    assert!(!result.success);
    assert!(result.error.as_deref().unwrap_or("").contains("url"));
}

#[tokio::test]
async fn test_missing_message_for_send() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "send", "url": "http://localhost:18790"});
    let result = tool.execute(input, &ctx()).await.unwrap();
    assert!(!result.success);
    assert!(result.error.as_deref().unwrap_or("").contains("message"));
}

#[tokio::test]
async fn test_missing_task_id_for_get() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "get", "url": "http://localhost:18790"});
    let result = tool.execute(input, &ctx()).await.unwrap();
    assert!(!result.success);
    assert!(result.error.as_deref().unwrap_or("").contains("task_id"));
}

#[tokio::test]
async fn test_missing_task_id_for_cancel() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "cancel", "url": "http://localhost:18790"});
    let result = tool.execute(input, &ctx()).await.unwrap();
    assert!(!result.success);
    assert!(result.error.as_deref().unwrap_or("").contains("task_id"));
}

#[tokio::test]
async fn test_unknown_action() {
    let tool = A2aSendTool::new();
    let input = serde_json::json!({"action": "invalid", "url": "http://localhost:18790"});
    let result = tool.execute(input, &ctx()).await.unwrap();
    assert!(!result.success);
    assert!(
        result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("Unknown action")
    );
}

#[test]
fn test_extract_response_text_from_status_message() {
    let task = serde_json::json!({
        "status": {
            "state": "completed",
            "message": {
                "role": "agent",
                "parts": [{"text": "Hello from agent"}]
            }
        }
    });
    assert_eq!(extract_response_text(&task), "Hello from agent");
}

#[test]
fn test_extract_response_text_from_artifacts() {
    let task = serde_json::json!({
        "status": {"state": "completed"},
        "artifacts": [{
            "parts": [{"text": "Result A"}, {"text": "Result B"}]
        }]
    });
    assert_eq!(extract_response_text(&task), "Result A\nResult B");
}

#[test]
fn test_extract_response_text_combined() {
    let task = serde_json::json!({
        "status": {
            "state": "completed",
            "message": {"role": "agent", "parts": [{"text": "Status msg"}]}
        },
        "artifacts": [{"parts": [{"text": "Artifact text"}]}]
    });
    assert_eq!(extract_response_text(&task), "Status msg\nArtifact text");
}

#[test]
fn test_extract_response_text_empty() {
    let task = serde_json::json!({"status": {"state": "working"}});
    assert_eq!(extract_response_text(&task), "");
}

#[test]
fn test_auth_headers_with_key() {
    let headers = auth_headers(Some("my-secret"));
    let auth = headers.get(reqwest::header::AUTHORIZATION).unwrap();
    assert_eq!(auth.to_str().unwrap(), "Bearer my-secret");
}

#[test]
fn test_auth_headers_without_key() {
    let headers = auth_headers(None);
    assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());
}

#[test]
fn test_default_impl() {
    let tool = A2aSendTool;
    assert_eq!(tool.name(), "a2a_send");
}
