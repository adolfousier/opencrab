use crate::brain::provider::codex_oauth::*;

#[test]
fn codex_client_id_is_correct() {
    assert_eq!(CODEX_CLIENT_ID, "app_EMoamEEZ73f0CkXaXp7hrann");
}

#[test]
fn codex_urls_are_correct() {
    assert!(DEVICE_CODE_URL.contains("deviceauth/usercode"));
    assert!(DEVICE_TOKEN_URL.contains("deviceauth/token"));
    assert!(OAUTH_TOKEN_URL.contains("oauth/token"));
    assert!(OPENAI_CHAT_URL.contains("chat/completions"));
}

#[test]
fn token_response_deserializes() {
    let json = r#"{
        "access_token": "at_abc123",
        "refresh_token": "rt__xyz789",
        "id_token": "eyJ...",
        "account_id": "8e1f627a-...",
        "expires_in": 864000
    }"#;
    let resp: TokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.access_token, "at_abc123");
    assert_eq!(resp.refresh_token, "rt__xyz789");
    assert_eq!(resp.expires_in, 864000);
}

#[test]
fn device_flow_response_deserializes() {
    let json = r#"{
        "device_code": "dc_abc123",
        "user_code": "ABCD-1234",
        "verification_uri": "https://auth.openai.com/verify",
        "expires_in": 900,
        "interval": 5
    }"#;
    let resp: DeviceFlowResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.user_code, "ABCD-1234");
    assert_eq!(resp.interval, 5);
}

#[test]
fn codex_tokens_serializes_and_deserializes() {
    let tokens = CodexTokens {
        access_token: "at_test".to_string(),
        refresh_token: "rt_test".to_string(),
        id_token: Some("id_test".to_string()),
        account_id: Some("acc_test".to_string()),
        expires_at: 9999999999,
    };
    let json = serde_json::to_string(&tokens).unwrap();
    let restored: CodexTokens = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.access_token, "at_test");
    assert_eq!(restored.account_id.as_deref(), Some("acc_test"));
}

#[test]
fn token_manager_starts_with_loaded_tokens() {
    let mgr = CodexTokenManager::new();
    let _ = mgr.get_cached_token();
}
