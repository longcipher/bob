//! # Provider Router
//!
//! Routes LLM requests across multiple providers with fallback,
//! priority ordering, and circuit-breaker-aware health checks.
//!
//! ## Strategies
//!
//! - [`RoutingStrategy::Priority`]: Try providers in order; fall back to the next on failure.
//! - [`RoutingStrategy::RoundRobin`]: Distribute requests across healthy providers in round-robin
//!   order.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::provider_router::{ProviderRouter, RoutingStrategy, ProviderEntry};
//! use std::sync::Arc;
//!
//! let router = ProviderRouter::new(RoutingStrategy::Priority)
//!     .with_provider(ProviderEntry::new("openai", Arc::new(openai_adapter)))
//!     .with_provider(ProviderEntry::new("anthropic", Arc::new(anthropic_adapter)));
//!
//! let response = router.complete(request).await?;
//! ```

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use bob_core::{
    error::LlmError,
    ports::LlmPort,
    resilience::{CircuitBreaker, CircuitState},
    types::{LlmCapabilities, LlmRequest, LlmResponse, LlmStream},
};

// ── Routing Strategy ─────────────────────────────────────────────────

/// How the router selects which provider to use.
#[derive(Debug, Clone)]
pub enum RoutingStrategy {
    /// Try each provider in the order they were added; stop on first success.
    Priority,
    /// Distribute across healthy providers in round-robin fashion.
    RoundRobin,
}

// ── Provider Entry ───────────────────────────────────────────────────

/// A named LLM provider with an optional circuit breaker.
pub struct ProviderEntry {
    /// Human-readable name (e.g. `"openai"`, `"anthropic"`).
    pub name: String,
    /// The underlying LLM adapter.
    pub adapter: Arc<dyn LlmPort>,
    /// Optional circuit breaker for this provider.
    pub circuit_breaker: Option<Arc<CircuitBreaker>>,
}

impl std::fmt::Debug for ProviderEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderEntry")
            .field("name", &self.name)
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .finish_non_exhaustive()
    }
}

impl ProviderEntry {
    /// Create a new provider entry without a circuit breaker.
    #[must_use]
    pub fn new(name: impl Into<String>, adapter: Arc<dyn LlmPort>) -> Self {
        Self { name: name.into(), adapter, circuit_breaker: None }
    }

    /// Attach a circuit breaker to this provider entry.
    #[must_use]
    pub fn with_circuit_breaker(mut self, cb: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = Some(cb);
        self
    }

    /// Returns `true` if the circuit breaker allows a call.
    fn is_available(&self) -> bool {
        match &self.circuit_breaker {
            Some(cb) => cb.state() != CircuitState::Open,
            None => true,
        }
    }
}

// ── Provider Router ──────────────────────────────────────────────────

/// Routes LLM requests across multiple providers.
pub struct ProviderRouter {
    strategy: RoutingStrategy,
    providers: Vec<ProviderEntry>,
    round_robin_index: AtomicUsize,
}

impl std::fmt::Debug for ProviderRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRouter")
            .field("strategy", &self.strategy)
            .field("provider_count", &self.providers.len())
            .finish_non_exhaustive()
    }
}

impl ProviderRouter {
    /// Create a new router with the given strategy.
    #[must_use]
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self { strategy, providers: Vec::new(), round_robin_index: AtomicUsize::new(0) }
    }

    /// Add a provider to the router.
    #[must_use]
    pub fn with_provider(mut self, entry: ProviderEntry) -> Self {
        self.providers.push(entry);
        self
    }

    /// Returns the number of registered providers.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Execute a request using the configured routing strategy.
    async fn route_request<F, Fut>(&self, make_call: F) -> Result<LlmResponse, LlmError>
    where
        F: Fn(Arc<dyn LlmPort>) -> Fut,
        Fut: std::future::Future<Output = Result<LlmResponse, LlmError>>,
    {
        match &self.strategy {
            RoutingStrategy::Priority => self.route_priority(&make_call).await,
            RoutingStrategy::RoundRobin => self.route_round_robin(&make_call).await,
        }
    }

    async fn route_priority<F, Fut>(&self, make_call: &F) -> Result<LlmResponse, LlmError>
    where
        F: Fn(Arc<dyn LlmPort>) -> Fut,
        Fut: std::future::Future<Output = Result<LlmResponse, LlmError>>,
    {
        let mut last_error = None;

        for entry in &self.providers {
            if !entry.is_available() {
                continue;
            }

            let result = if let Some(cb) = &entry.circuit_breaker {
                cb.call(|| make_call(entry.adapter.clone())).await.map_err(|cb_err| match cb_err {
                    bob_core::resilience::CircuitBreakerError::CircuitOpen => {
                        LlmError::Provider(format!("{}: circuit open", entry.name))
                    }
                    bob_core::resilience::CircuitBreakerError::Inner(e) => e,
                })
            } else {
                make_call(entry.adapter.clone()).await
            };

            match result {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    tracing::warn!(provider = %entry.name, error = %err, "provider failed, trying next");
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LlmError::Provider("no providers available".into())))
    }

    async fn route_round_robin<F, Fut>(&self, make_call: &F) -> Result<LlmResponse, LlmError>
    where
        F: Fn(Arc<dyn LlmPort>) -> Fut,
        Fut: std::future::Future<Output = Result<LlmResponse, LlmError>>,
    {
        let healthy: Vec<&ProviderEntry> =
            self.providers.iter().filter(|p| p.is_available()).collect();

        if healthy.is_empty() {
            return Err(LlmError::Provider("no healthy providers available".into()));
        }

        let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed) % healthy.len();

        // Try starting from the round-robin index, then wrap around.
        let mut last_error = None;
        for offset in 0..healthy.len() {
            let entry = healthy[(index + offset) % healthy.len()];

            let result = if let Some(cb) = &entry.circuit_breaker {
                cb.call(|| make_call(entry.adapter.clone())).await.map_err(|cb_err| match cb_err {
                    bob_core::resilience::CircuitBreakerError::CircuitOpen => {
                        LlmError::Provider(format!("{}: circuit open", entry.name))
                    }
                    bob_core::resilience::CircuitBreakerError::Inner(e) => e,
                })
            } else {
                make_call(entry.adapter.clone()).await
            };

            match result {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    tracing::warn!(provider = %entry.name, error = %err, "provider failed in round-robin");
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LlmError::Provider("all providers failed".into())))
    }
}

// ── LlmPort Implementation ───────────────────────────────────────────

#[async_trait]
impl LlmPort for ProviderRouter {
    fn capabilities(&self) -> LlmCapabilities {
        // Union of all provider capabilities.
        let mut native_tool_calling = false;
        let mut streaming = false;
        for entry in &self.providers {
            let caps = entry.adapter.capabilities();
            native_tool_calling |= caps.native_tool_calling;
            streaming |= caps.streaming;
        }
        LlmCapabilities { native_tool_calling, streaming }
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let req = Arc::new(req);
        self.route_request(|adapter| {
            let req = Arc::clone(&req);
            async move { adapter.complete((*req).clone()).await }
        })
        .await
    }

    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
        // For streaming, try each provider in priority order.
        for entry in &self.providers {
            if !entry.is_available() {
                continue;
            }
            match entry.adapter.complete_stream(req.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    tracing::warn!(provider = %entry.name, error = %err, "stream provider failed, trying next");
                }
            }
        }
        Err(LlmError::Provider("no provider available for streaming".into()))
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct MockLlm {
        _name: &'static str,
        responses: Mutex<Vec<Result<LlmResponse, LlmError>>>,
    }

    impl MockLlm {
        fn succeeds(name: &'static str, content: &'static str) -> Self {
            Self {
                _name: name,
                responses: Mutex::new(vec![Ok(LlmResponse {
                    content: content.into(),
                    usage: bob_core::types::TokenUsage::default(),
                    finish_reason: bob_core::types::FinishReason::Stop,
                    tool_calls: vec![],
                })]),
            }
        }

        fn always_fails(name: &'static str) -> Self {
            Self {
                _name: name,
                responses: Mutex::new(vec![Err(LlmError::Provider(format!(
                    "{name}: simulated failure"
                )))]),
            }
        }
    }

    #[async_trait]
    impl LlmPort for MockLlm {
        fn capabilities(&self) -> LlmCapabilities {
            LlmCapabilities { native_tool_calling: false, streaming: false }
        }

        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            let mut responses = match self.responses.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if responses.is_empty() {
                return Err(LlmError::Provider("no more mock responses".into()));
            }
            responses.remove(0)
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("streaming not supported in mock".into()))
        }
    }

    fn test_request() -> LlmRequest {
        LlmRequest {
            model: "test-model".into(),
            messages: vec![bob_core::types::Message::text(bob_core::types::Role::User, "hello")],
            tools: vec![],
            output_schema: None,
        }
    }

    #[tokio::test]
    async fn priority_routes_to_first_available() {
        let router = ProviderRouter::new(RoutingStrategy::Priority)
            .with_provider(ProviderEntry::new(
                "primary",
                Arc::new(MockLlm::succeeds("primary", "ok")),
            ))
            .with_provider(ProviderEntry::new(
                "backup",
                Arc::new(MockLlm::succeeds("backup", "fallback")),
            ));

        let resp = router.complete(test_request()).await.expect("should succeed");
        assert_eq!(resp.content, "ok");
    }

    #[tokio::test]
    async fn priority_falls_back_on_failure() {
        let router = ProviderRouter::new(RoutingStrategy::Priority)
            .with_provider(ProviderEntry::new(
                "primary",
                Arc::new(MockLlm::always_fails("primary")),
            ))
            .with_provider(ProviderEntry::new(
                "backup",
                Arc::new(MockLlm::succeeds("backup", "fallback")),
            ));

        let resp = router.complete(test_request()).await.expect("should succeed via fallback");
        assert_eq!(resp.content, "fallback");
    }

    #[tokio::test]
    async fn priority_fails_when_all_providers_fail() {
        let router = ProviderRouter::new(RoutingStrategy::Priority)
            .with_provider(ProviderEntry::new("p1", Arc::new(MockLlm::always_fails("p1"))))
            .with_provider(ProviderEntry::new("p2", Arc::new(MockLlm::always_fails("p2"))));

        let result = router.complete(test_request()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn round_robin_distributes_requests() {
        let router = ProviderRouter::new(RoutingStrategy::RoundRobin)
            .with_provider(ProviderEntry::new("a", Arc::new(MockLlm::succeeds("a", "from-a"))))
            .with_provider(ProviderEntry::new("b", Arc::new(MockLlm::succeeds("b", "from-b"))));

        // Both should succeed; the order depends on the round-robin index.
        let _ = router.complete(test_request()).await.expect("first call should succeed");
        let _ = router.complete(test_request()).await.expect("second call should succeed");
    }

    #[tokio::test]
    async fn circuit_breaker_skips_open_provider() {
        let cb = Arc::new(CircuitBreaker::new(bob_core::resilience::CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            cooldown: std::time::Duration::from_secs(60),
        }));

        // Trip the circuit breaker by calling it directly.
        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        let router = ProviderRouter::new(RoutingStrategy::Priority)
            .with_provider(
                ProviderEntry::new("primary", Arc::new(MockLlm::succeeds("primary", "ok")))
                    .with_circuit_breaker(cb),
            )
            .with_provider(ProviderEntry::new(
                "backup",
                Arc::new(MockLlm::succeeds("backup", "fallback")),
            ));

        let resp = router.complete(test_request()).await.expect("should fall back to backup");
        assert_eq!(resp.content, "fallback");
    }

    #[tokio::test]
    async fn no_providers_returns_error() {
        let router = ProviderRouter::new(RoutingStrategy::Priority);
        let result = router.complete(test_request()).await;
        assert!(result.is_err());
    }
}
