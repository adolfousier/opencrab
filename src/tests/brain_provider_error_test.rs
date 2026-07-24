use crate::brain::provider::error::*;

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
fn flaky_404_is_retryable_but_model_not_found_404_is_not() {
    // #748: a flaky provider's transient 404 must retry with backoff...
    let flaky = ProviderError::ApiError {
        status: 404,
        message: "Not Found".to_string(),
        error_type: None,
    };
    assert!(
        flaky.is_retryable(),
        "a non-model 404 is a transient infra hiccup and must retry"
    );

    // ...but a genuine model-not-found 404 stays permanent.
    let model_404 = ProviderError::ApiError {
        status: 404,
        message: "The model gpt-x is not found".to_string(),
        error_type: Some("model_not_found".to_string()),
    };
    assert!(
        !model_404.is_retryable(),
        "a model-not-found 404 is permanent, not retryable"
    );
}

#[test]
fn repetitive_tool_guardrail_500_is_not_retryable() {
    // #740: the repetition guardrail 500 will 500 again on retry with the same
    // poisoned history — surface it fast so the tool loop can prune and retry.
    let poison = ProviderError::ApiError {
        status: 500,
        message: "Repetitive tool calls detected in the conversation history. The same tool call \
                  with identical name and arguments has been repeated across multiple consecutive \
                  rounds."
            .to_string(),
        error_type: Some("invalid_request_error".to_string()),
    };
    assert!(
        !poison.is_retryable(),
        "the repetition guardrail 500 must not be retried in place"
    );

    // A plain 500 is still retryable.
    let plain = ProviderError::ApiError {
        status: 500,
        message: "Internal Server Error".to_string(),
        error_type: None,
    };
    assert!(plain.is_retryable());
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
