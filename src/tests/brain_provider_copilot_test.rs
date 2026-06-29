use super::*;

#[test]
fn copilot_client_id_is_well_known() {
    assert_eq!(COPILOT_CLIENT_ID, "Iv1.b507a08c87ecfe98");
}

#[test]
fn copilot_urls_are_correct() {
    assert!(COPILOT_CHAT_URL.starts_with("https://api.githubcopilot.com"));
    assert!(COPILOT_MODELS_URL.starts_with("https://api.githubcopilot.com"));
    assert!(COPILOT_TOKEN_URL.contains("copilot_internal"));
    assert!(DEVICE_CODE_URL.contains("login/device"));
    assert!(OAUTH_TOKEN_URL.contains("login/oauth"));
}

#[test]
fn copilot_extra_headers_include_required_fields() {
    let headers = copilot_extra_headers();
    let keys: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"copilot-integration-id"));
    assert!(keys.contains(&"editor-version"));
    assert!(keys.contains(&"user-agent"));
}

#[test]
fn token_manager_starts_with_empty_token() {
    let manager = CopilotTokenManager::new("gho_test".to_string());
    assert!(manager.get_cached_token().is_empty());
}

#[test]
fn device_flow_response_deserializes() {
    let json = r#"{
            "device_code": "abc123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;
    let resp: DeviceFlowResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.user_code, "ABCD-1234");
    assert_eq!(resp.interval, 5);
    assert_eq!(resp.expires_in, 900);
}
