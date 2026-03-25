//! # Tower Service Integration
//!
//! Bridges Bob's port traits with the `tower::Service` ecosystem, enabling
//! composition with standard middleware (timeout, retry, rate limit, etc.)
//! without reimplementing them in the framework.
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

// ── Tool Service ─────────────────────────────────────────────────────

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

/// Request type for [`ToolService`].
#[derive(Debug, Clone)]
pub struct ToolServiceRequest {
    /// The tool call to execute.
    pub call: ToolCall,
}

/// Response type for [`ToolService`].
#[derive(Debug, Clone)]
pub struct ToolServiceResponse {
    /// The tool execution result.
    pub result: ToolResult,
}

impl tower::Service<ToolServiceRequest> for ToolService {
    type Response = ToolServiceResponse;
    type Error = ToolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ToolServiceRequest) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let result = inner.call_tool(req.call).await?;
            Ok(ToolServiceResponse { result })
        })
    }
}

// ── LLM Service ──────────────────────────────────────────────────────

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

/// Request type for [`LlmService`].
#[derive(Debug, Clone)]
pub struct LlmServiceRequest {
    /// The LLM request to execute.
    pub request: LlmRequest,
}

/// Response type for [`LlmService`].
#[derive(Debug, Clone)]
pub struct LlmServiceResponse {
    /// The LLM response.
    pub response: LlmResponse,
}

impl tower::Service<LlmServiceRequest> for LlmService {
    type Response = LlmServiceResponse;
    type Error = LlmError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: LlmServiceRequest) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let response = inner.complete(req.request).await?;
            Ok(LlmServiceResponse { response })
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

/// Request type for [`ToolListService`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolListRequest;

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

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

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

        use tower::Service;
        let resp = svc
            .call(ToolServiceRequest { call: ToolCall::new("mock/echo", serde_json::json!({})) })
            .await;
        assert!(resp.is_ok());
        assert_eq!(resp.unwrap().result.name, "mock/echo");
    }

    #[tokio::test]
    async fn tool_list_service() {
        let port: Arc<dyn ToolPort> = Arc::new(MockToolPort::new());
        let mut svc = ToolListService::new(port);

        use tower::Service;
        let tools = svc.call(ToolListRequest).await.expect("should list tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "mock/echo");
    }
}
