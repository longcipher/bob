//! # Resilience Patterns
//!
//! Circuit breaker and retry configuration for preventing cascading failures.
//!
//! The [`CircuitBreaker`] is a pure state machine that tracks call outcomes
//! and transitions between `Closed` → `Open` → `HalfOpen` → `Closed`.
//! Adapters wrap their port implementations with [`CircuitBreaker::call`]
//! to add automatic failure detection and recovery probing.
//!
//! [`RetryConfig`] provides exponential backoff parameters for adapters
//! that wish to retry transient failures before surfacing errors.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

// ── Circuit Breaker ──────────────────────────────────────────────────

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Requests flow normally.
    Closed,
    /// Requests fail fast without calling the underlying service.
    Open,
    /// A single probe request is allowed to test recovery.
    HalfOpen,
}

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening the circuit.
    pub failure_threshold: u32,
    /// Number of consecutive successes in `HalfOpen` before closing the circuit.
    pub success_threshold: u32,
    /// How long to wait in `Open` before transitioning to `HalfOpen`.
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self { failure_threshold: 5, success_threshold: 2, cooldown: Duration::from_secs(30) }
    }
}

/// Inner state protected by a mutex.
#[derive(Debug)]
struct CircuitBreakerInner {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_at: Option<Instant>,
}

/// Circuit breaker for preventing cascading failures.
///
/// ## States
///
/// ```text
/// Closed ──(failures ≥ threshold)──▶ Open
///   ▲                                    │
///   │                                    │ cooldown elapsed
///   │                                    ▼
///   └──(successes ≥ threshold)── HalfOpen ◀┘
///                                      │
///          (any failure) ──────────────▶ Open
/// ```
///
/// ## Example
///
/// ```rust,ignore
/// use bob_core::resilience::{CircuitBreaker, CircuitBreakerConfig};
/// use std::time::Duration;
///
/// let cb = CircuitBreaker::new(CircuitBreakerConfig {
///     failure_threshold: 3,
///     success_threshold: 1,
///     cooldown: Duration::from_secs(10),
/// });
///
/// let result = cb.call(|| async { Ok::<_, &str>(42) }).await;
/// assert_eq!(result, Ok(42));
/// ```
#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    inner: Mutex<CircuitBreakerInner>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    #[must_use]
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(CircuitBreakerInner {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure_at: None,
            }),
        }
    }

    /// Returns the current circuit state.
    ///
    /// If the circuit is `Open` and the cooldown has elapsed, this
    /// transitions to `HalfOpen` before returning.
    pub fn state(&self) -> CircuitState {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Self::maybe_transition_to_half_open(&mut inner, &self.config);
        inner.state
    }

    /// Execute an async operation through the circuit breaker.
    ///
    /// Returns `Err(CircuitBreakerError::CircuitOpen)` immediately if the
    /// circuit is open and the cooldown has not elapsed.
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        // Pre-check: is the circuit open?
        {
            let mut inner = match self.inner.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            Self::maybe_transition_to_half_open(&mut inner, &self.config);
            if inner.state == CircuitState::Open {
                return Err(CircuitBreakerError::CircuitOpen);
            }
        }

        // Execute the operation.
        match f().await {
            Ok(value) => {
                self.on_success();
                Ok(value)
            }
            Err(err) => {
                self.on_failure();
                Err(CircuitBreakerError::Inner(err))
            }
        }
    }

    /// Manually reset the circuit breaker to the closed state.
    pub fn reset(&self) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.state = CircuitState::Closed;
        inner.failure_count = 0;
        inner.success_count = 0;
        inner.last_failure_at = None;
    }

    fn maybe_transition_to_half_open(
        inner: &mut CircuitBreakerInner,
        config: &CircuitBreakerConfig,
    ) {
        if inner.state == CircuitState::Open &&
            let Some(last_failure) = inner.last_failure_at &&
            last_failure.elapsed() >= config.cooldown
        {
            inner.state = CircuitState::HalfOpen;
            inner.success_count = 0;
        }
    }

    fn on_success(&self) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match inner.state {
            CircuitState::HalfOpen => {
                inner.success_count = inner.success_count.saturating_add(1);
                if inner.success_count >= self.config.success_threshold {
                    inner.state = CircuitState::Closed;
                    inner.failure_count = 0;
                    inner.success_count = 0;
                }
            }
            CircuitState::Closed => {
                inner.failure_count = 0;
            }
            CircuitState::Open => {}
        }
    }

    fn on_failure(&self) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match inner.state {
            CircuitState::Closed | CircuitState::HalfOpen => {
                inner.failure_count = inner.failure_count.saturating_add(1);
                if inner.failure_count >= self.config.failure_threshold {
                    inner.state = CircuitState::Open;
                    inner.last_failure_at = Some(Instant::now());
                }
            }
            CircuitState::Open => {}
        }
    }
}

// ── Circuit Breaker Error ────────────────────────────────────────────

/// Error returned by [`CircuitBreaker::call`].
pub enum CircuitBreakerError<E> {
    /// The circuit is open; the call was not attempted.
    CircuitOpen,
    /// The underlying operation returned an error.
    Inner(E),
}

impl<E> std::fmt::Debug for CircuitBreakerError<E>
where
    E: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircuitOpen => f.write_str("CircuitOpen"),
            Self::Inner(e) => f.debug_tuple("Inner").field(e).finish(),
        }
    }
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircuitOpen => f.write_str("circuit breaker is open"),
            Self::Inner(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for CircuitBreakerError<E> {}

impl<E> CircuitBreakerError<E> {
    /// Stable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::CircuitOpen => "BOB_CIRCUIT_OPEN",
            Self::Inner(_) => "BOB_CIRCUIT_INNER",
        }
    }
}

// ── Retry Config ─────────────────────────────────────────────────────

/// Jitter strategy for retry delays to avoid thundering herd.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JitterKind {
    /// No jitter — use the computed delay as-is.
    #[default]
    None,
    /// Add uniform random jitter: delay ± (delay * jitter_ratio).
    Uniform,
    /// Full jitter: random value between 0 and computed delay.
    Full,
}

/// Exponential backoff retry parameters.
///
/// Adapters consume this configuration to drive retry loops.
/// The struct itself is pure data; the actual `sleep` call
/// is left to the adapter (which has access to `tokio::time`).
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts after the first failure.
    pub max_retries: u32,
    /// Initial delay between attempts.
    pub initial_delay: Duration,
    /// Maximum delay (caps the exponential growth).
    pub max_delay: Duration,
    /// Multiplier applied to the delay after each failure.
    pub multiplier: f64,
    /// Jitter strategy to add randomness and avoid thundering herd.
    pub jitter: JitterKind,
    /// Jitter ratio for Uniform jitter (0.0-1.0). Default 0.25 = ±25%.
    pub jitter_ratio: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
            jitter: JitterKind::None,
            jitter_ratio: 0.25,
        }
    }
}

impl RetryConfig {
    /// Compute the base delay for a given attempt number (0-indexed).
    #[must_use]
    pub fn base_delay(&self, attempt: u32) -> Duration {
        let base = self.initial_delay.as_millis() as f64;
        let scaled = base * self.multiplier.powi(attempt as i32);
        let capped = scaled.min(self.max_delay.as_millis() as f64);
        Duration::from_millis(capped as u64)
    }

    /// Compute the delay with jitter for a given attempt number (0-indexed).
    ///
    /// Uses `rand` crate when available, falls back to a simple
    /// deterministic hash-based jitter for no-std / no-rand environments.
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.base_delay(attempt);
        self.apply_jitter(base, attempt)
    }

    /// Apply jitter to a base delay.
    fn apply_jitter(&self, base: Duration, attempt: u32) -> Duration {
        match self.jitter {
            JitterKind::None => base,
            JitterKind::Uniform => {
                let jitter_range = (base.as_millis() as f64) * self.jitter_ratio;
                let offset = (Self::deterministic_jitter(attempt) * jitter_range)
                    .mul_add(2.0, -jitter_range);
                let result = (base.as_millis() as f64 + offset).max(0.0);
                Duration::from_millis(result as u64)
            }
            JitterKind::Full => {
                let max = base.as_millis() as f64;
                let factor = Self::deterministic_jitter(attempt);
                Duration::from_millis((max * factor) as u64)
            }
        }
    }

    /// Deterministic pseudo-random jitter based on attempt number.
    ///
    /// Returns a value in [0.0, 1.0). Uses a simple hash-like
    /// approach so behavior is deterministic for testing.
    fn deterministic_jitter(attempt: u32) -> f64 {
        // Simple xorshift-like hash for deterministic pseudo-random
        let mut x = attempt.wrapping_mul(0x9E37_79B9).wrapping_add(0x517C_C1B7);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        // Normalize to [0.0, 1.0)
        f64::from(x) / (f64::from(u32::MAX) + 1.0)
    }
}

impl RetryConfig {
    /// Create a config with uniform jitter (±ratio).
    #[must_use]
    pub fn with_uniform_jitter(mut self, ratio: f64) -> Self {
        self.jitter = JitterKind::Uniform;
        self.jitter_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Create a config with full jitter (random between 0 and delay).
    #[must_use]
    pub fn with_full_jitter(mut self) -> Self {
        self.jitter = JitterKind::Full;
        self
    }
}

// ── Health Check ─────────────────────────────────────────────────────

/// Simple health status for a single component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Component is operating normally.
    Healthy,
    /// Component is operational but degraded (e.g. high latency).
    Degraded,
    /// Component is not operational.
    Unhealthy,
}

/// A health check result for one component.
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    /// Component name (e.g. `"llm:openai"`, `"mcp:filesystem"`).
    pub name: String,
    /// Current health status.
    pub status: HealthStatus,
    /// Optional human-readable detail.
    pub detail: Option<String>,
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn circuit_breaker_allows_calls_when_closed() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);

        let result = cb.call(|| async { Ok::<_, &str>(42) }).await;
        assert!(result.is_ok());
        assert_eq!(result.expect("should succeed"), 42);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 1,
            cooldown: Duration::from_secs(60),
        });

        for _ in 0..3 {
            let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        }

        assert_eq!(cb.state(), CircuitState::Open);

        let result = cb.call(|| async { Ok::<_, &str>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::CircuitOpen)));
    }

    #[tokio::test]
    async fn circuit_breaker_half_open_after_cooldown() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            cooldown: Duration::from_millis(50),
        });

        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn circuit_breaker_closes_after_success_in_half_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            cooldown: Duration::from_millis(10),
        });

        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        let result = cb.call(|| async { Ok::<_, &str>(42) }).await;
        assert!(result.is_ok());
        assert_eq!(result.expect("should succeed"), 42);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_reopens_on_failure_in_half_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            cooldown: Duration::from_millis(10),
        });

        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        let _ = cb.call(|| async { Err::<(), _>("fail again") }).await;
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn circuit_breaker_reset() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            cooldown: Duration::from_secs(60),
        });

        let _ = cb.call(|| async { Err::<(), _>("fail") }).await;
        assert!(cb.call(|| async { Ok::<_, &str>(1) }).await.is_err());

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn retry_config_delay_computation() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            multiplier: 2.0,
            jitter: JitterKind::None,
            jitter_ratio: 0.25,
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(config.delay_for_attempt(10), Duration::from_secs(5)); // capped
    }

    #[test]
    fn retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(200));
    }
}
