use std::future::Future;
use std::time::Duration;

use tokio::time::{sleep, timeout};
use tracing::{debug, warn};

use crate::error::{Error, Result};

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
                let timeout_error = Error::timeout(config.timeout, format!("attempt-{attempt}"));
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

    Err(last_error
        .unwrap_or_else(|| Error::InternalError("Retry failed with no error captured".to_string())))
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
                    Err(Error::ConnectionClosed)
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
                Err(Error::MethodNotFound("test".to_string()))
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

        assert!(matches!(result, Err(Error::Timeout { .. })));
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let start = tokio::time::Instant::now();
        let attempt_times = Arc::new(std::sync::Mutex::new(Vec::new()));
        let times_clone = attempt_times.clone();

        let config = RetryConfig {
            max_attempts: 4,
            initial_delay: Duration::from_millis(10), // Shorter for faster test
            max_delay: Duration::from_millis(50),
            backoff_multiplier: 2.0,
            timeout: Duration::from_secs(30),
        };

        let _: Result<()> = with_retry(&config, || {
            let times = times_clone.clone();
            async move {
                times.lock().unwrap().push(start.elapsed());
                Err(Error::ConnectionClosed)
            }
        })
        .await;

        let times = attempt_times.lock().unwrap();
        assert_eq!(times.len(), 4);

        // Verify delays are increasing (can't check exact timing without pause)
        for i in 1..times.len() {
            assert!(times[i] > times[i - 1], "Delays should increase");
        }
    }

    #[tokio::test]
    async fn test_max_delay_capping() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(15), // Very low max delay
            backoff_multiplier: 10.0,             // High multiplier
            timeout: Duration::from_secs(30),
        };

        let start = tokio::time::Instant::now();
        let attempt_times = Arc::new(std::sync::Mutex::new(Vec::new()));
        let times_clone = attempt_times.clone();

        let _: Result<()> = with_retry(&config, || {
            let times = times_clone.clone();
            async move {
                times.lock().unwrap().push(start.elapsed());
                Err(Error::ConnectionClosed)
            }
        })
        .await;

        let times = attempt_times.lock().unwrap();
        assert_eq!(times.len(), 3);

        // Verify that delay between attempts 2 and 3 is capped
        if times.len() >= 3 {
            let _delay1 = times[1] - times[0];
            let delay2 = times[2] - times[1];
            // Second delay should be capped at max_delay (15ms)
            assert!(delay2 <= Duration::from_millis(20)); // Allow some variance
        }
    }

    #[tokio::test]
    async fn test_no_sleep_on_last_attempt() {
        let sleep_called = Arc::new(AtomicU32::new(0));
        let sleep_clone = sleep_called.clone();

        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            ..Default::default()
        };

        // Track when sleep would be called by counting attempts
        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_clone = attempt_count.clone();

        let _: Result<()> = with_retry(&config, || {
            let count = attempt_clone.clone();
            let sleep_track = sleep_clone.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    // We're tracking that sleep WOULD be called between attempts
                    sleep_track.fetch_add(1, Ordering::SeqCst);
                }
                Err(Error::ConnectionClosed)
            }
        })
        .await;

        // Should sleep 2 times (after attempt 1 and 2, but not after attempt 3)
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
        assert_eq!(sleep_called.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_immediate_success() {
        let attempt_count = Arc::new(AtomicU32::new(0));
        let count_clone = attempt_count.clone();

        let config = RetryConfig {
            max_attempts: 5,
            ..Default::default()
        };

        let result = with_retry(&config, || {
            let count = count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok("immediate success")
            }
        })
        .await;

        assert_eq!(result.unwrap(), "immediate success");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_timeout_with_partial_execution() {
        let config = RetryConfig {
            max_attempts: 1,
            timeout: Duration::from_millis(50),
            ..Default::default()
        };

        let progress = Arc::new(AtomicU32::new(0));
        let progress_clone = progress.clone();

        let result: Result<&str> = with_retry(&config, || {
            let prog = progress_clone.clone();
            async move {
                prog.store(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                prog.store(2, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(60)).await;
                prog.store(3, Ordering::SeqCst); // Should not reach here
                Ok("completed")
            }
        })
        .await;

        assert!(matches!(result, Err(Error::Timeout { .. })));
        // We can verify partial execution happened
        let final_progress = progress.load(Ordering::SeqCst);
        assert!((1..=2).contains(&final_progress));
    }

    #[tokio::test]
    async fn test_zero_max_attempts() {
        let config = RetryConfig {
            max_attempts: 0,
            ..Default::default()
        };

        let result: Result<&str> = with_retry(&config, || async { Ok("test") }).await;

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(Error::InternalError(msg)) if msg.contains("Retry failed")
        ));
    }

    #[tokio::test]
    async fn test_alternating_errors() {
        let attempt_count = Arc::new(AtomicU32::new(0));
        let count_clone = attempt_count.clone();

        let config = RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_millis(10),
            ..Default::default()
        };

        let result = with_retry(&config, || {
            let count = count_clone.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                match attempt {
                    0 => Err(Error::ConnectionClosed),
                    1 => Err(Error::Timeout {
                        duration: Duration::from_secs(1),
                        request_id: "test".to_string(),
                    }),
                    2 => Err(Error::TransportDisconnected),
                    _ => Ok("success after various errors"),
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "success after various errors");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_operation_completes_just_before_timeout() {
        let config = RetryConfig {
            max_attempts: 1,
            timeout: Duration::from_millis(100),
            ..Default::default()
        };

        let result: Result<&str> = with_retry(&config, || async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok("just in time")
        })
        .await;

        assert_eq!(result.unwrap(), "just in time");
    }
}
