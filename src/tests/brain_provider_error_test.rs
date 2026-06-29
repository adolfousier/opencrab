use super::*;

#[test]
fn test_error_retryable() {
    let rate_limit = ProviderError::RateLimitExceeded("Try again later".to_string());
    assert!(rate_limit.is_retryable());

    let invalid_key = ProviderError::InvalidApiKey;
    assert!(!invalid_key.is_retryable());

    let server_error = ProviderError::ApiError {
        status: 500,
        message: "Internal Server Error".to_string(),
        error_type: None,
    };
    assert!(server_error.is_retryable());

    let client_error = ProviderError::ApiError {
        status: 400,
        message: "Bad Request".to_string(),
        error_type: None,
    };
    assert!(!client_error.is_retryable());
}

#[test]
fn test_status_code() {
    let error = ProviderError::ApiError {
        status: 429,
        message: "Too many requests".to_string(),
        error_type: Some("rate_limit_error".to_string()),
    };
    assert_eq!(error.status_code(), Some(429));

    let invalid_key = ProviderError::InvalidApiKey;
    assert_eq!(invalid_key.status_code(), None);
}
