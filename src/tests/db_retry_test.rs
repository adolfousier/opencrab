use super::*;

#[test]
fn test_retry_config_defaults() {
    let config = DbRetryConfig::default();
    assert_eq!(config.max_attempts, 5);
    assert_eq!(config.initial_delay, Duration::from_millis(50));
    assert_eq!(config.max_delay, Duration::from_secs(5));
}

#[test]
fn test_retry_config_aggressive() {
    let config = DbRetryConfig::aggressive();
    assert_eq!(config.max_attempts, 10);
    assert_eq!(config.initial_delay, Duration::from_millis(100));
}

#[test]
fn test_calculate_delay() {
    let config = DbRetryConfig {
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_secs(5),
        backoff_multiplier: 2.0,
        max_attempts: 5,
    };

    let delay0 = config.calculate_delay(0);
    assert_eq!(delay0, Duration::from_millis(50));

    let delay1 = config.calculate_delay(1);
    assert_eq!(delay1, Duration::from_millis(100));

    let delay2 = config.calculate_delay(2);
    assert_eq!(delay2, Duration::from_millis(200));

    // Should cap at max_delay
    let delay10 = config.calculate_delay(10);
    assert_eq!(delay10, Duration::from_secs(5));
}

#[test]
fn test_is_database_locked() {
    // Non-lock error should return false
    let err = rusqlite::Error::QueryReturnedNoRows;
    assert!(!is_database_locked(&err));
}

#[tokio::test]
async fn test_retry_success_immediate() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let config = DbRetryConfig::default();
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    let result = retry_db_operation(
        move || {
            let count = call_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>(42)
            }
        },
        &config,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_retry_success_after_retries() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let config = DbRetryConfig::new(3, Duration::from_millis(10));
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    let result = retry_db_operation(
        move || {
            let count = call_count_clone.clone();
            async move {
                let current = count.fetch_add(1, Ordering::SeqCst) + 1;
                if current < 3 {
                    Err("database is locked".to_string())
                } else {
                    Ok::<_, String>(42)
                }
            }
        },
        &config,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_retry_max_attempts_exceeded() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let config = DbRetryConfig::new(2, Duration::from_millis(10));
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    let result = retry_db_operation(
        move || {
            let count = call_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>("database is locked".to_string())
            }
        },
        &config,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 3); // Initial + 2 retries
}

#[tokio::test]
async fn test_retry_non_retryable_error() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let config = DbRetryConfig::default();
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    let result = retry_db_operation(
        move || {
            let count = call_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>("constraint violation".to_string())
            }
        },
        &config,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 1); // Should not retry
}
