use crate::a2a::types::*;
#[test]
fn test_part_text_creation() {
    let part = Part::text("hello world");
    assert_eq!(part.text.as_deref(), Some("hello world"));
    assert!(part.data.is_none());
}

#[test]
fn test_task_state_serialization() {
    let state = TaskState::Working;
    let json = serde_json::to_string(&state).expect("serialize");
    assert_eq!(json, "\"working\"");
}

#[test]
fn test_json_rpc_success_response() {
    let resp = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"status": "ok"}));
    assert_eq!(resp.jsonrpc, "2.0");
    assert!(resp.result.is_some());
    assert!(resp.error.is_none());
}

#[test]
fn test_json_rpc_error_response() {
    let resp = JsonRpcResponse::error(
        serde_json::json!(1),
        error_codes::METHOD_NOT_FOUND,
        "Method not found",
    );
    assert_eq!(resp.error.as_ref().expect("has error").code, -32601);
}

#[test]
fn test_agent_card_serialization() {
    let card = AgentCard {
        name: "TestAgent".to_string(),
        description: Some("A test agent".to_string()),
        version: Some("0.1.0".to_string()),
        documentation_url: None,
        icon_url: None,
        supported_interfaces: vec![SupportedInterface {
            url: "http://localhost:18789/a2a/v1".to_string(),
            protocol_binding: "JSONRPC".to_string(),
            protocol_version: Some("1.0".to_string()),
        }],
        provider: Some(AgentProvider {
            organization: "OpenCrabs Contributors".to_string(),
            url: Some("https://github.com/adolfousier/opencrabs".to_string()),
        }),
        capabilities: Some(AgentCapabilities {
            streaming: false,
            push_notifications: false,
            state_transition_history: false,
        }),
        skills: vec![],
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
    };
    let json = serde_json::to_string_pretty(&card).expect("serialize");
    assert!(json.contains("TestAgent"));
    assert!(json.contains("OpenCrabs Contributors"));
}

#[test]
fn test_message_round_trip() {
    let msg = Message {
        message_id: Some("msg-1".to_string()),
        context_id: None,
        task_id: None,
        role: Role::User,
        parts: vec![Part::text("Hello, agent!")],
        metadata: None,
    };
    let json = serde_json::to_string(&msg).expect("serialize");
    let parsed: Message = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.role, Role::User);
    assert_eq!(parsed.parts.len(), 1);
}
