//! # Native Async Port Traits
//!
//! Ergonomic native `async fn` trait alternatives that don't require
//! `#[async_trait]`. Developers implement these for the best IDE experience
//! and fastest compile times. The framework automatically bridges them to
//! the dyn-compatible ports via blanket implementations.
//!
//! ## Why Two Sets of Traits?
//!
//! Rust's native `async fn in trait` (RPITIT) produces opaque `impl Future`
//! return types, which are not object-safe. The framework's runtime uses
//! `Arc<dyn Port>` for flexibility, requiring `#[async_trait]` (which boxes
//! the future). These native traits let you write clean code without the
//! macro overhead, while the blanket impls handle the bridging.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_core::native::NativeLlmPort;
//!
//! struct MyLlm;
//!
//! // Native async — no #[async_trait] needed
//! impl NativeLlmPort for MyLlm {
//!     async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
//!         // ...
//!     }
//! }
//!
//! // Automatically usable as Arc<dyn LlmPort> via blanket impl
//! let port: Arc<dyn LlmPort> = Arc::new(MyLlm);
//! ```

use std::future::Future;

use crate::{
    error::{LlmError, StoreError, ToolError},
    types::{
        LlmRequest, LlmResponse, LlmStream, SessionId, SessionState, ToolCall, ToolDescriptor,
        ToolResult,
    },
};

// ── Native LLM Port ──────────────────────────────────────────────────

/// Native async LLM port — implement this for the cleanest developer experience.
///
/// The framework provides a blanket implementation that bridges any
/// `NativeLlmPort` to the dyn-compatible [`LlmPort`](crate::ports::LlmPort).
pub trait NativeLlmPort: Send + Sync {
    /// Run a non-streaming inference call.
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl Future<Output = Result<LlmResponse, LlmError>> + Send;

    /// Run a streaming inference call.
    fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> impl Future<Output = Result<LlmStream, LlmError>> + Send;
}

// ── Native Tool Port ─────────────────────────────────────────────────

/// Native async tool port — implement this for the cleanest developer experience.
///
/// The framework provides a blanket implementation that bridges any
/// `NativeToolPort` to the dyn-compatible [`ToolPort`](crate::ports::ToolPort).
pub trait NativeToolPort: Send + Sync {
    /// List all available tools.
    fn list_tools(&self) -> impl Future<Output = Result<Vec<ToolDescriptor>, ToolError>> + Send;

    /// Execute a tool call.
    fn call_tool(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolResult, ToolError>> + Send;
}

// ── Native Session Store ─────────────────────────────────────────────

/// Native async session store — implement this for the cleanest developer experience.
pub trait NativeSessionStore: Send + Sync {
    /// Load a session by ID.
    fn load(
        &self,
        id: &SessionId,
    ) -> impl Future<Output = Result<Option<SessionState>, StoreError>> + Send;

    /// Save a session by ID.
    fn save(
        &self,
        id: &SessionId,
        state: &SessionState,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the traits are implementable without #[async_trait].
    struct MyTool;

    impl NativeToolPort for MyTool {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(vec![])
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult { name: call.name, output: serde_json::json!(null), is_error: false })
        }
    }

    struct MyLlm;

    impl NativeLlmPort for MyLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
        }
    }

    struct MyStore;

    impl NativeSessionStore for MyStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(None)
        }

        async fn save(&self, _id: &SessionId, _state: &SessionState) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn native_tool_port_works() {
        let tool = MyTool;
        let tools = tool.list_tools().await.unwrap();
        assert!(tools.is_empty());

        let result = tool.call_tool(ToolCall::new("test", serde_json::json!({}))).await.unwrap();
        assert_eq!(result.name, "test");
    }

    #[tokio::test]
    async fn native_llm_port_works() {
        let llm = MyLlm;
        let req = LlmRequest {
            model: "test".into(),
            messages: vec![],
            tools: vec![],
            output_schema: None,
        };
        assert!(llm.complete(req).await.is_err());
    }

    #[tokio::test]
    async fn native_session_store_works() {
        let store = MyStore;
        let loaded = store.load(&"test".to_string()).await.unwrap();
        assert!(loaded.is_none());
    }
}
