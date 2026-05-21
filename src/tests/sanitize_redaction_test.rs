//! Tests for `redact_secrets()` — the function that scrubs sensitive data
//! from tool output before it's displayed in the TUI and channels.
//!
//! Covers: env var assignments, piped secrets, IPv4 addresses,
//! known key prefixes, hex tokens, and mixed alphanumeric tokens.

use crate::utils::sanitize::redact_secrets;

// ── Environment variable assignments ───────────────────────────────

#[test]
fn redacts_env_var_pass_assignment() {
    let input = r#"ADMIN_PASS="fuNZEIYc2isz0txisiWTKg8A""#;
    let out = redact_secrets(input);
    assert!(
        !out.contains("fuNZEIYc2isz0txisiWTKg8A"),
        "password leaked: {out}"
    );
    assert!(
        out.contains("ADMIN_PASS="),
        "variable name should be preserved: {out}"
    );
}

#[test]
fn redacts_env_var_secret_assignment() {
    let input = "NEW_SECRET=mgd4EjM8oTrmvWPEbqKys7q2c5H6N7";
    let out = redact_secrets(input);
    assert!(
        !out.contains("mgd4EjM8oTrmvWPEbqKys7q2c5H6N7"),
        "secret leaked: {out}"
    );
}

#[test]
fn redacts_env_var_token_assignment() {
    let input = "API_TOKEN=abc123def456ghi789jkl012mno345";
    let out = redact_secrets(input);
    assert!(
        !out.contains("abc123def456ghi789jkl012mno345"),
        "token leaked: {out}"
    );
}

#[test]
fn redacts_env_var_apikey_assignment() {
    let input = "MY_APIKEY=sk_live_abcdef1234567890abcdef";
    let out = redact_secrets(input);
    assert!(
        !out.contains("abcdef1234567890abcdef"),
        "api key leaked: {out}"
    );
}

#[test]
fn redacts_env_var_credential_assignment() {
    let input = "DB_CREDENTIAL=super_secret_password_12345";
    let out = redact_secrets(input);
    assert!(
        !out.contains("super_secret_password_12345"),
        "credential leaked: {out}"
    );
}

#[test]
fn redacts_env_var_auth_assignment() {
    let input = "SERVICE_AUTH=bearer_token_abcdefghijklmnop";
    let out = redact_secrets(input);
    assert!(
        !out.contains("bearer_token_abcdefghijklmnop"),
        "auth value leaked: {out}"
    );
}

// ── Piped secrets ──────────────────────────────────────────────────

#[test]
fn redacts_piped_secret_double_quotes() {
    let input =
        r#"echo "mgd4EjM8oTrmvWPEbqKys7q2c5H6N7" | docker login -u robot$harbor --password-stdin"#;
    let out = redact_secrets(input);
    assert!(
        !out.contains("mgd4EjM8oTrmvWPEbqKys7q2c5H6N7"),
        "piped secret leaked: {out}"
    );
    assert!(out.contains("echo"), "echo command preserved: {out}");
    assert!(
        out.contains("docker login"),
        "docker command preserved: {out}"
    );
}

#[test]
fn redacts_piped_secret_single_quotes() {
    let input = "echo 'superSecretToken1234567890ab' | kubectl apply -f -";
    let out = redact_secrets(input);
    assert!(
        !out.contains("superSecretToken1234567890ab"),
        "piped secret leaked: {out}"
    );
}

#[test]
fn does_not_redact_short_echo_values() {
    let input = r#"echo "hello" | cat"#;
    let out = redact_secrets(input);
    assert!(
        out.contains("hello"),
        "short non-secret should not be redacted: {out}"
    );
}

// ── IPv4 addresses ─────────────────────────────────────────────────

#[test]
fn redacts_server_ip() {
    let input = "Connected to 138.68.166.23 on port 443";
    let out = redact_secrets(input);
    assert!(!out.contains("138.68.166.23"), "server IP leaked: {out}");
    assert!(
        out.contains("[IP_REDACTED]"),
        "IP should be replaced with [IP_REDACTED]: {out}"
    );
}

#[test]
fn redacts_multiple_ips() {
    let input = "Primary: 10.0.1.5, Secondary: 192.168.1.100";
    let out = redact_secrets(input);
    assert!(!out.contains("10.0.1.5"), "first IP leaked: {out}");
    assert!(!out.contains("192.168.1.100"), "second IP leaked: {out}");
}

#[test]
fn preserves_localhost() {
    let input = "Listening on 127.0.0.1:8080";
    let out = redact_secrets(input);
    assert!(
        out.contains("127.0.0.1"),
        "localhost should be preserved: {out}"
    );
}

#[test]
fn preserves_zero_address() {
    let input = "Binding to 0.0.0.0:3000";
    let out = redact_secrets(input);
    assert!(
        out.contains("0.0.0.0"),
        "0.0.0.0 should be preserved: {out}"
    );
}

// ── Existing patterns (regression) ─────────────────────────────────

#[test]
fn redacts_bearer_token() {
    let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test";
    let out = redact_secrets(input);
    assert!(
        !out.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
        "JWT leaked: {out}"
    );
}

#[test]
fn redacts_api_key_assignment() {
    let input = "api_key=sk-proj-abcdef1234567890abcdef1234567890";
    let out = redact_secrets(input);
    assert!(
        !out.contains("abcdef1234567890abcdef1234567890"),
        "API key leaked: {out}"
    );
}

#[test]
fn redacts_github_pat() {
    let input = "Token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef12";
    let out = redact_secrets(input);
    assert!(
        !out.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef12"),
        "GitHub PAT leaked: {out}"
    );
}

#[test]
fn redacts_long_hex_token() {
    let input = "secret: a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let out = redact_secrets(input);
    assert!(
        !out.contains("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"),
        "hex token leaked: {out}"
    );
}

// ── Combined patterns ──────────────────────────────────────────────

#[test]
fn redacts_mixed_output_with_ips_and_secrets() {
    let input = r#"Deploying to 209.97.180.4 with ADMIN_PASS="fuNZEIYc2isz0txisiWTKg8A" and token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef12"#;
    let out = redact_secrets(input);
    assert!(!out.contains("209.97.180.4"), "IP leaked: {out}");
    assert!(
        !out.contains("fuNZEIYc2isz0txisiWTKg8A"),
        "password leaked: {out}"
    );
    assert!(
        !out.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef12"),
        "GitHub PAT leaked: {out}"
    );
}

#[test]
fn preserves_non_sensitive_output() {
    let input = "Build completed successfully in 42 seconds with 0 errors";
    let out = redact_secrets(input);
    assert_eq!(out, input, "non-sensitive output should be unchanged");
}
