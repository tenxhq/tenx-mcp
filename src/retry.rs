use std::future::Future;
use std::time::Duration;

use tokio::time::{sleep, timeout};
use tracing::{debug, warn};

use crate::error::{MCPError, Result};

/// Configuration for retry behavior
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Request timeout duration
    pub timeout: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Execute a future with retry logic
pub async fn with_retry<F, Fut, T>(config: &RetryConfig, mut operation: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut delay = config.initial_delay;
    let mut last_error = None;

    for attempt in 1..=config.max_attempts {
        debug!("Attempt {} of {}", attempt, config.max_attempts);

        // Execute with timeout
        match timeout(config.timeout, operation()).await {
            Ok(Ok(result)) => return Ok(result),
            Ok(Err(e)) => {
                last_error = Some(e);

                // Check if error is retryable
                if let Some(ref error) = last_error {
                    if !error.is_retryable() {
                        debug!("Error is not retryable: {}", error);
                        return Err(error.clone());
                    }
                }

                // Don't sleep on the last attempt
                if attempt < config.max_attempts {
                    warn!(
                        "Attempt {} failed, retrying in {:?}: {:?}",
                        attempt, delay, last_error
                    );
                    sleep(delay).await;

                    // Calculate next delay with exponential backoff
                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * config.backoff_multiplier)
                            .min(config.max_delay.as_secs_f64()),
                    );
                }
            }
            Err(_) => {
                let timeout_error =
                    MCPError::timeout(config.timeout, format!("attempt-{}", attempt));
                last_error = Some(timeout_error);

                // Don't sleep on the last attempt
                if attempt < config.max_attempts {
                    warn!(
                        "Attempt {} timed out after {:?}, retrying",
                        attempt, config.timeout
                    );
                    sleep(delay).await;
                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * config.backoff_multiplier)
                            .min(config.max_delay.as_secs_f64()),
                    );
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        MCPError::InternalError("Retry failed with no error captured".to_string())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_retry_success_on_second_attempt() {
        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = attempt_count.clone();

        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            ..Default::default()
        };

        let result = with_retry(&config, || {
            let count = attempt_count_clone.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(MCPError::ConnectionClosed)
                } else {
                    Ok("success")
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = attempt_count.clone();

        let config = RetryConfig {
            max_attempts: 3,
            ..Default::default()
        };

        let result: Result<&str> = with_retry(&config, || {
            let count = attempt_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err(MCPError::MethodNotFound("test".to_string()))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1); // Should not retry
    }

    #[tokio::test]
    async fn test_retry_timeout() {
        let config = RetryConfig {
            max_attempts: 2,
            timeout: Duration::from_millis(50),
            initial_delay: Duration::from_millis(10),
            ..Default::default()
        };

        let result: Result<&str> = with_retry(&config, || async {
            sleep(Duration::from_millis(100)).await;
            Ok("should timeout")
        })
        .await;

        assert!(matches!(result, Err(MCPError::Timeout { .. })));
    }
}
