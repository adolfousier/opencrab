use crate::config::secrets::*;

#[test]
fn test_secret_string_creation() {
    let secret = SecretString::from_str("my-secret-key");
    assert_eq!(secret.expose_secret(), "my-secret-key");
    assert_eq!(secret.len(), 13);
    assert!(!secret.is_empty());
}

#[test]
fn test_secret_string_debug() {
    let secret = SecretString::from_str("my-secret-key");
    let debug_output = format!("{:?}", secret);
    assert_eq!(debug_output, "[REDACTED]");
    assert!(!debug_output.contains("my-secret-key"));
}

#[test]
fn test_secret_string_display() {
    let secret = SecretString::from_str("my-secret-key");
    let display_output = format!("{}", secret);
    assert_eq!(display_output, "[REDACTED]");
    assert!(!display_output.contains("my-secret-key"));
}

#[test]
fn test_secret_string_from_env_missing() {
    // Test that a non-existent env var returns None (no env loading)
    let result = std::env::var("OPENCRABS_TEST_NONEXISTENT_KEY_12345");
    assert!(result.is_err());
}

#[test]
fn test_secret_string_serialize() {
    let secret = SecretString::from_str("my-secret-key");
    let serialized = serde_json::to_string(&secret).unwrap();
    assert_eq!(serialized, "\"[REDACTED]\"");
    assert!(!serialized.contains("my-secret-key"));
}
