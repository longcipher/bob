//! # Resilient LLM Adapter
//!
//! Wraps any [`LlmPort`] implementation with circuit breaker protection
//! and exponential backoff retry logic.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::resilient_llm::ResilientLlmAdapter;
//! use bob_core::resilience::{CircuitBreaker, CircuitBreakerConfig, RetryConfig};
//! use std::sync::Arc;
//!
//! let inner = GenAiLlmAdapter::new(client);
//! let resilient = ResilientLlmAdapter::new(inner)
//!     .with_circuit_breaker(Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())))
//!     .with_retry(RetryConfig::default());
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use bob_core::{
    error::LlmError,
    ports::LlmPort,
    resilience::{CircuitBreaker, CircuitBreakerError, RetryConfig},
    types::{LlmCapabilities, LlmRequest, LlmResponse, LlmStream},
};

/// Resilience wrapper around an [`LlmPort`] implementation.
pub struct ResilientLlmAdapter<T: LlmPort> {
    inner: T,
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    retry_config: Option<RetryConfig>,
}

impl<T: LlmPort> std::fmt::Debug for ResilientLlmAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResilientLlmAdapter")
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .field("has_retry", &self.retry_config.is_some())
            .finish_non_exhaustive()
    }
}

impl<T: LlmPort> ResilientLlmAdapter<T> {
    /// Create a new resilient adapter wrapping the given inner adapter.
    #[must_use]
    pub fn new(inner: T) -> Self {
        Self { inner, circuit_breaker: None, retry_config: None }
    }

    /// Attach a circuit breaker.
    #[must_use]
    pub fn with_circuit_breaker(mut self, cb: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = Some(cb);
        self
    }

    /// Attach retry configuration.
    #[must_use]
    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Execute with circuit breaker protection (no retry).
    async fn execute_with_cb<F, Fut, R>(&self, f: F) -> Result<R, LlmError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<R, LlmError>>,
    {
        match &self.circuit_breaker {
            Some(cb) => cb.call(f).await.map_err(|cb_err| match cb_err {
                CircuitBreakerError::CircuitOpen => {
                    LlmError::Provider("circuit breaker is open".into())
                }
                CircuitBreakerError::Inner(e) => e,
            }),
            None => f().await,
        }
    }

    /// Execute with circuit breaker + retry.
    async fn execute_with_retry<F, Fut, R>(&self, f: &F) -> Result<R, LlmError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<R, LlmError>>,
    {
        let retry_config = match &self.retry_config {
            Some(config) => config,
            None => return self.execute_with_cb(f).await,
        };

        let mut last_err = None;
        for attempt in 0..=retry_config.max_retries {
            if attempt > 0 {
                let delay = retry_config.delay_for_attempt(attempt - 1);
                tokio::time::sleep(delay).await;
            }

            match self.execute_with_cb(f).await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    // Don't retry on circuit-open or context-length errors.
                    if matches!(&err, LlmError::Provider(msg) if msg.contains("circuit breaker is open"))
                        || matches!(err, LlmError::ContextLengthExceeded)
                    {
                        return Err(err);
                    }
                    tracing::warn!(attempt, error = %err, "LLM call failed, will retry");
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| LlmError::Provider("all retry attempts exhausted".into())))
    }
}

#[async_trait]
impl<T: LlmPort> LlmPort for ResilientLlmAdapter<T> {
    fn capabilities(&self) -> LlmCapabilities {
        self.inner.capabilities()
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.execute_with_retry(&|| self.inner.complete(req.clone())).await
    }

    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
        // Streaming does not retry — partial streams cannot be replayed.
        self.execute_with_cb(|| self.inner.complete_stream(req.clone())).await
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct FlakyLlm {
        fail_count: Mutex<u32>,
    }

    impl FlakyLlm {
        fn new(fail_count: u32) -> Self {
            Self { fail_count: Mutex::new(fail_count) }
        }
    }

    #[async_trait]
    impl LlmPort for FlakyLlm {
        fn capabilities(&self) -> LlmCapabilities {
            LlmCapabilities::default()
        }

        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            let mut count = match self.fail_count.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if *count > 0 {
                *count -= 1;
                return Err(LlmError::Provider("flaky failure".into()));
            }
            Ok(LlmResponse {
                content: "success".into(),
                usage: bob_core::types::TokenUsage::default(),
                finish_reason: bob_core::types::FinishReason::Stop,
                tool_calls: vec![],
            })
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("no stream".into()))
        }
    }

    fn test_request() -> LlmRequest {
        LlmRequest {
            model: "test".into(),
            messages: vec![bob_core::types::Message::text(
                bob_core::types::Role::User,
                "hi",
            )],
            tools: vec![],
        }
    }

    #[tokio::test]
    async fn retry_succeeds_after_transient_failures() {
        let adapter = ResilientLlmAdapter::new(FlakyLlm::new(2)).with_retry(RetryConfig {
            max_retries: 3,
            initial_delay: std::time::Duration::from_millis(1),
            max_delay: std::time::Duration::from_millis(10),
            multiplier: 2.0,
        });

        let resp = adapter.complete(test_request()).await.expect("should succeed after retries");
        assert_eq!(resp.content, "success");
    }

    #[tokio::test]
    async fn retry_exhausts_and_returns_error() {
        let adapter = ResilientLlmAdapter::new(FlakyLlm::new(10)).with_retry(RetryConfig {
            max_retries: 2,
            initial_delay: std::time::Duration::from_millis(1),
            max_delay: std::time::Duration::from_millis(10),
            multiplier: 2.0,
        });

        let result = adapter.complete(test_request()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn no_retry_config_just_calls_once() {
        let adapter = ResilientLlmAdapter::new(FlakyLlm::new(1));

        let result = adapter.complete(test_request()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold() {
        let cb = Arc::new(CircuitBreaker::new(
            bob_core::resilience::CircuitBreakerConfig {
                failure_threshold: 2,
                success_threshold: 1,
                cooldown: std::time::Duration::from_secs(60),
            },
        ));

        let adapter = ResilientLlmAdapter::new(FlakyLlm::new(100))
            .with_circuit_breaker(Arc::clone(&cb))
            .with_retry(RetryConfig {
                max_retries: 0,
                initial_delay: std::time::Duration::from_millis(1),
                max_delay: std::time::Duration::from_millis(1),
                multiplier: 1.0,
            });

        // Two calls should trip the breaker.
        let _ = adapter.complete(test_request()).await;
        let _ = adapter.complete(test_request()).await;

        // Third call should fail with circuit open.
        let result = adapter.complete(test_request()).await;
        assert!(result.is_err());
        if let Err(LlmError::Provider(msg)) = result {
            assert!(msg.contains("circuit breaker is open"));
        }
    }
}
