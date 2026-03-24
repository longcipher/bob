//! # Resilient Tool Adapter
//!
//! Wraps any [`ToolPort`] implementation with circuit breaker protection
//! and exponential backoff retry logic.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::resilient_tool::ResilientToolAdapter;
//! use bob_core::resilience::{CircuitBreaker, CircuitBreakerConfig, RetryConfig};
//! use std::sync::Arc;
//!
//! let inner = McpToolAdapter::connect_stdio(...).await?;
//! let resilient = ResilientToolAdapter::new(inner)
//!     .with_circuit_breaker(Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())))
//!     .with_retry(RetryConfig::default());
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use bob_core::{
    error::ToolError,
    ports::ToolPort,
    resilience::{CircuitBreaker, CircuitBreakerError, RetryConfig},
    types::{ToolCall, ToolDescriptor, ToolResult},
};

/// Resilience wrapper around a [`ToolPort`] implementation.
pub struct ResilientToolAdapter<T: ToolPort> {
    inner: T,
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    retry_config: Option<RetryConfig>,
}

impl<T: ToolPort> std::fmt::Debug for ResilientToolAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResilientToolAdapter")
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .field("has_retry", &self.retry_config.is_some())
            .finish_non_exhaustive()
    }
}

impl<T: ToolPort> ResilientToolAdapter<T> {
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

    async fn execute_with_retry<F, Fut, R>(&self, f: &F) -> Result<R, ToolError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<R, ToolError>>,
    {
        let retry_config = match &self.retry_config {
            Some(config) => config,
            None => {
                return match &self.circuit_breaker {
                    Some(cb) => cb.call(f).await.map_err(map_cb_error),
                    None => f().await,
                };
            }
        };

        let mut last_err = None;
        for attempt in 0..=retry_config.max_retries {
            if attempt > 0 {
                let delay = retry_config.delay_for_attempt(attempt - 1);
                tokio::time::sleep(delay).await;
            }

            let result = match &self.circuit_breaker {
                Some(cb) => cb.call(f).await.map_err(map_cb_error),
                None => f().await,
            };

            match result {
                Ok(r) => return Ok(r),
                Err(err) => {
                    // Don't retry tool-not-found — it won't recover.
                    if matches!(&err, ToolError::NotFound { .. }) {
                        return Err(err);
                    }
                    tracing::warn!(attempt, error = %err, "tool call failed, will retry");
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| ToolError::Other(
            "all retry attempts exhausted".to_string().into(),
        )))
    }
}

fn map_cb_error<E: std::fmt::Display>(cb_err: CircuitBreakerError<E>) -> ToolError {
    match cb_err {
        CircuitBreakerError::CircuitOpen => {
            ToolError::Other("circuit breaker is open".to_string().into())
        }
        CircuitBreakerError::Inner(e) => ToolError::Other(e.to_string().into()),
    }
}

#[async_trait]
impl<T: ToolPort> ToolPort for ResilientToolAdapter<T> {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        self.inner.list_tools().await
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let call_name = call.name.clone();
        let inner = &self.inner;
        self.execute_with_retry(&|| {
            let c = ToolCall::new(call_name.clone(), call.arguments.clone());
            async move { inner.call_tool(c).await }
        })
        .await
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct FlakyTool {
        fail_count: Mutex<u32>,
    }

    impl FlakyTool {
        fn new(fail_count: u32) -> Self {
            Self { fail_count: Mutex::new(fail_count) }
        }
    }

    #[async_trait]
    impl ToolPort for FlakyTool {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(vec![])
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            let mut count = match self.fail_count.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if *count > 0 {
                *count -= 1;
                return Err(ToolError::Execution("flaky failure".into()));
            }
            Ok(ToolResult {
                name: call.name,
                output: serde_json::json!({"ok": true}),
                is_error: false,
            })
        }
    }

    fn test_call() -> ToolCall {
        ToolCall::new("test-tool", serde_json::json!({"arg": 1}))
    }

    #[tokio::test]
    async fn retry_succeeds_after_transient_failures() {
        let adapter = ResilientToolAdapter::new(FlakyTool::new(2)).with_retry(RetryConfig {
            max_retries: 3,
            initial_delay: std::time::Duration::from_millis(1),
            max_delay: std::time::Duration::from_millis(10),
            multiplier: 2.0,
        });

        let result = adapter.call_tool(test_call()).await.expect("should succeed after retries");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn retry_exhausts_and_returns_error() {
        let adapter = ResilientToolAdapter::new(FlakyTool::new(10)).with_retry(RetryConfig {
            max_retries: 2,
            initial_delay: std::time::Duration::from_millis(1),
            max_delay: std::time::Duration::from_millis(10),
            multiplier: 2.0,
        });

        let result = adapter.call_tool(test_call()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_tools_passes_through() {
        let adapter = ResilientToolAdapter::new(FlakyTool::new(0));
        let tools = adapter.list_tools().await.expect("list_tools should succeed");
        assert!(tools.is_empty());
    }
}
