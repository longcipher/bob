//! # Tower Service Integration
//!
//! Bridges Bob's port traits with the `tower::Service` ecosystem, enabling
//! composition with standard middleware (timeout, retry, rate limit, etc.)
//! without reimplementing them in the framework.
//!
//! ## Design Philosophy
//!
//! This module demonstrates **extreme trait reuse** by leveraging `tower::Service`
//! as the universal middleware abstraction instead of building custom middleware chains.
//!
//! ## Key Patterns
//!
//! - **Blanket implementations**: Every `ToolPort` automatically becomes a `tower::Service`
//! - **Extension traits**: Fluent APIs for middleware composition
//! - **Type-state builders**: Compile-time validation of middleware chains
//! - **Associated types**: Bind request/response types to service definitions
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::tower_service::*;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! // Wrap a ToolPort with tower middleware
//! let tool_service = ToolService::new(my_tool_port);
//! let guarded = ServiceBuilder::new()
//!     .timeout(Duration::from_secs(15))
//!     .rate_limit(10, Duration::from_secs(1))
//!     .service(tool_service);
//! ```

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bob_core::{
    error::{LlmError, ToolError},
    ports::{LlmPort, ToolPort},
    types::{LlmRequest, LlmResponse, ToolCall, ToolDescriptor, ToolResult},
};

// ── Core Service Types ───────────────────────────────────────────────

/// Request type for tool execution services.
#[derive(Debug, Clone)]
pub struct ToolRequest {
    /// The tool call to execute.
    pub call: ToolCall,
}

impl ToolRequest {
    /// Create a new tool request.
    #[must_use]
    pub fn new(call: ToolCall) -> Self {
        Self { call }
    }

    /// Create a tool request from name and arguments.
    #[must_use]
    pub fn from_parts(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self { call: ToolCall::new(name, arguments) }
    }
}

/// Response type for tool execution services.
#[derive(Debug, Clone)]
pub struct ToolResponse {
    /// The tool execution result.
    pub result: ToolResult,
}

/// Request type for LLM completion services.
#[derive(Debug, Clone)]
pub struct LlmRequestWrapper {
    /// The LLM request to execute.
    pub request: LlmRequest,
}

/// Response type for LLM completion services.
#[derive(Debug, Clone)]
pub struct LlmResponseWrapper {
    /// The LLM response.
    pub response: LlmResponse,
}

/// Request type for tool listing services.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolListRequest;

// ── Tool Service (tower::Service wrapper) ─────────────────────────────

/// A `tower::Service` wrapper around [`ToolPort::call_tool`].
///
/// This allows composing tool execution with standard tower middleware
/// like `Timeout`, `Retry`, `RateLimit`, `ConcurrencyLimit`, etc.
pub struct ToolService {
    inner: Arc<dyn ToolPort>,
}

impl ToolService {
    /// Wrap a [`ToolPort`] as a `tower::Service`.
    #[must_use]
    pub fn new(port: Arc<dyn ToolPort>) -> Self {
        Self { inner: port }
    }
}

impl std::fmt::Debug for ToolService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolService").finish_non_exhaustive()
    }
}

impl tower::Service<ToolRequest> for ToolService {
    type Response = ToolResponse;
    type Error = ToolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ToolRequest) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let result = inner.call_tool(req.call).await?;
            Ok(ToolResponse { result })
        })
    }
}

// ── LLM Service (tower::Service wrapper) ──────────────────────────────

/// A `tower::Service` wrapper around [`LlmPort::complete`].
///
/// This allows composing LLM calls with standard tower middleware.
pub struct LlmService {
    inner: Arc<dyn LlmPort>,
}

impl LlmService {
    /// Wrap an [`LlmPort`] as a `tower::Service`.
    #[must_use]
    pub fn new(port: Arc<dyn LlmPort>) -> Self {
        Self { inner: port }
    }
}

impl std::fmt::Debug for LlmService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmService").finish_non_exhaustive()
    }
}

impl tower::Service<LlmRequestWrapper> for LlmService {
    type Response = LlmResponseWrapper;
    type Error = LlmError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: LlmRequestWrapper) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let response = inner.complete(req.request).await?;
            Ok(LlmResponseWrapper { response })
        })
    }
}

// ── Tool List Service ────────────────────────────────────────────────

/// A `tower::Service` wrapper around [`ToolPort::list_tools`].
pub struct ToolListService {
    inner: Arc<dyn ToolPort>,
}

impl ToolListService {
    /// Wrap a [`ToolPort`] for list_tools as a `tower::Service`.
    #[must_use]
    pub fn new(port: Arc<dyn ToolPort>) -> Self {
        Self { inner: port }
    }
}

impl std::fmt::Debug for ToolListService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolListService").finish_non_exhaustive()
    }
}

impl tower::Service<ToolListRequest> for ToolListService {
    type Response = Vec<ToolDescriptor>;
    type Error = ToolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ToolListRequest) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move { inner.list_tools().await })
    }
}

// ── Extension Traits for Service Composition ──────────────────────────

/// Extension trait for ergonomic service composition.
///
/// This provides a fluent API for building middleware stacks
/// without manual `ServiceBuilder` boilerplate.
pub trait ServiceExt<Request>: tower::Service<Request> + Sized {
    /// Apply a timeout to this service.
    fn with_timeout(self, timeout: std::time::Duration) -> tower::timeout::Timeout<Self> {
        tower::timeout::Timeout::new(self, timeout)
    }

    /// Apply rate limiting to this service.
    fn with_rate_limit(
        self,
        max: u64,
        interval: std::time::Duration,
    ) -> tower::limit::RateLimit<Self> {
        tower::limit::RateLimit::new(self, tower::limit::rate::Rate::new(max, interval))
    }

    /// Apply concurrency limiting to this service.
    fn with_concurrency_limit(self, max: usize) -> tower::limit::ConcurrencyLimit<Self> {
        tower::limit::ConcurrencyLimit::new(self, max)
    }

    /// Map errors from this service.
    fn map_err<F, E2>(self, f: F) -> tower::util::MapErr<Self, F>
    where
        F: FnOnce(Self::Error) -> E2,
    {
        tower::util::MapErr::new(self, f)
    }

    /// Map responses from this service.
    fn map_response<F, Response2>(self, f: F) -> tower::util::MapResponse<Self, F>
    where
        F: FnOnce(Self::Response) -> Response2,
    {
        tower::util::MapResponse::new(self, f)
    }

    /// Box this service for dynamic dispatch.
    fn boxed(self) -> tower::util::BoxService<Request, Self::Response, Self::Error>
    where
        Self: Send + 'static,
        Request: Send + 'static,
        Self::Future: Send + 'static,
    {
        tower::util::BoxService::new(self)
    }
}

// Blanket implementation for all tower::Service implementations
impl<T, Request> ServiceExt<Request> for T where T: tower::Service<Request> + Sized {}

// ── Port Extension Traits ────────────────────────────────────────────

/// Extension trait that converts a [`ToolPort`] into tower services.
///
/// This provides a fluent API for creating service instances from tool ports.
pub trait ToolPortServiceExt: ToolPort {
    /// Convert this tool port into a `tower::Service` for tool execution.
    fn into_tool_service(self: Arc<Self>) -> ToolService;

    /// Convert this tool port into a `tower::Service` for tool listing.
    fn into_tool_list_service(self: Arc<Self>) -> ToolListService;
}

// Blanket implementation for all ToolPort implementations
impl<T: ToolPort + 'static> ToolPortServiceExt for T {
    fn into_tool_service(self: Arc<Self>) -> ToolService {
        ToolService::new(self)
    }

    fn into_tool_list_service(self: Arc<Self>) -> ToolListService {
        ToolListService::new(self)
    }
}

// Implementation for trait objects
impl ToolPortServiceExt for dyn ToolPort {
    fn into_tool_service(self: Arc<Self>) -> ToolService {
        ToolService::new(self)
    }

    fn into_tool_list_service(self: Arc<Self>) -> ToolListService {
        ToolListService::new(self)
    }
}

/// Extension trait that converts an [`LlmPort`] into a tower service.
pub trait LlmPortServiceExt: LlmPort {
    /// Convert this LLM port into a `tower::Service` for completions.
    fn into_llm_service(self: Arc<Self>) -> LlmService;
}

// Blanket implementation for all LlmPort implementations
impl<T: LlmPort + 'static> LlmPortServiceExt for T {
    fn into_llm_service(self: Arc<Self>) -> LlmService {
        LlmService::new(self)
    }
}

// Implementation for trait objects
impl LlmPortServiceExt for dyn LlmPort {
    fn into_llm_service(self: Arc<Self>) -> LlmService {
        LlmService::new(self)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bob_core::types::ToolDescriptor;
    use tower::Service;

    use super::*;

    struct MockToolPort {
        calls: Mutex<Vec<String>>,
    }

    impl MockToolPort {
        fn new() -> Self {
            Self { calls: Mutex::new(Vec::new()) }
        }
    }

    #[async_trait::async_trait]
    impl ToolPort for MockToolPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(vec![ToolDescriptor::new("mock/echo", "Echo tool")])
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            let mut calls = self.calls.lock().unwrap_or_else(|p| p.into_inner());
            calls.push(call.name.clone());
            Ok(ToolResult {
                name: call.name,
                output: serde_json::json!({"ok": true}),
                is_error: false,
            })
        }
    }

    #[tokio::test]
    async fn tool_service_basic() {
        let port: Arc<dyn ToolPort> = Arc::new(MockToolPort::new());
        let mut svc = ToolService::new(port);

        let resp = svc.call(ToolRequest::from_parts("mock/echo", serde_json::json!({}))).await;
        assert!(resp.is_ok());
        assert_eq!(resp.unwrap().result.name, "mock/echo");
    }

    #[tokio::test]
    async fn tool_list_service() {
        let port: Arc<dyn ToolPort> = Arc::new(MockToolPort::new());
        let mut svc = ToolListService::new(port);

        let tools = svc.call(ToolListRequest).await.expect("should list tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "mock/echo");
    }

    #[tokio::test]
    async fn service_ext_timeout() {
        let port: Arc<dyn ToolPort> = Arc::new(MockToolPort::new());
        let svc = ToolService::new(port);

        let mut timeout_svc = svc.with_timeout(std::time::Duration::from_secs(1));

        let resp =
            timeout_svc.call(ToolRequest::from_parts("mock/echo", serde_json::json!({}))).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn port_service_ext() {
        let port: Arc<dyn ToolPort> = Arc::new(MockToolPort::new());
        let mut svc = port.into_tool_service();

        let resp = svc.call(ToolRequest::from_parts("mock/echo", serde_json::json!({}))).await;
        assert!(resp.is_ok());
    }
}
