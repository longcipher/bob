//! Bob Agent Runtime
//!
//! Orchestration layer: scheduler, turn loop, and public `AgentRuntime` trait.
//! Depends only on `bob-core` port traits — never on concrete adapters.

use std::sync::Arc;

pub use bob_core as core;
use bob_core::{
    error::AgentError,
    ports::{EventSink, LlmPort, SessionStore, ToolPort},
    types::{
        AgentEventStream, AgentRequest, AgentResponse, AgentRunResult, FinishReason, HealthStatus,
        RuntimeHealth, TokenUsage,
    },
};

// ── Bootstrap Trait ──────────────────────────────────────────────────

/// Configuration for an MCP server to connect at startup.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub id: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
}

/// Builder trait: configure the runtime, then freeze it into an `AgentRuntime`.
#[async_trait::async_trait]
pub trait AgentBootstrap: Send {
    /// Register an MCP server to discover tools from.
    async fn register_mcp_server(&mut self, cfg: McpServerConfig) -> Result<(), AgentError>;

    /// Consume the builder and produce a ready-to-use runtime.
    fn build(self) -> Result<Arc<dyn AgentRuntime>, AgentError>
    where
        Self: Sized;
}

// ── Runtime Trait ────────────────────────────────────────────────────

/// The primary API for running agent turns.
#[async_trait::async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Execute a single agent turn (blocking until complete).
    async fn run(&self, req: AgentRequest) -> Result<AgentRunResult, AgentError>;

    /// Execute a single agent turn with streaming events.
    async fn run_stream(&self, req: AgentRequest) -> Result<AgentEventStream, AgentError>;

    /// Check runtime health.
    async fn health(&self) -> RuntimeHealth;
}

// ── Default Implementation ───────────────────────────────────────────

/// Default runtime that composes the 4 port traits via `Arc<dyn ...>`.
pub struct DefaultAgentRuntime {
    pub llm: Arc<dyn LlmPort>,
    pub tools: Arc<dyn ToolPort>,
    pub store: Arc<dyn SessionStore>,
    pub events: Arc<dyn EventSink>,
}

impl std::fmt::Debug for DefaultAgentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultAgentRuntime").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl AgentRuntime for DefaultAgentRuntime {
    async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        // TODO: implement scheduler turn loop
        Ok(AgentRunResult::Finished(AgentResponse {
            content: "not yet implemented".into(),
            tool_transcript: vec![],
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(&self, _req: AgentRequest) -> Result<AgentEventStream, AgentError> {
        // TODO: implement streaming scheduler
        Err(AgentError::Internal("streaming not yet implemented".into()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use bob_core::{
        error::{LlmError, StoreError, ToolError},
        types::*,
    };

    use super::*;

    // Minimal mock implementations for testing the runtime wiring.

    struct StubLlm;

    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: "stub".into(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
            })
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
        }
    }

    struct StubTools;

    #[async_trait::async_trait]
    impl ToolPort for StubTools {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(vec![])
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult { name: call.name, output: serde_json::json!(null), is_error: false })
        }
    }

    struct StubStore;

    #[async_trait::async_trait]
    impl SessionStore for StubStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(None)
        }

        async fn save(&self, _id: &SessionId, _state: &SessionState) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct StubSink {
        count: Mutex<usize>,
    }

    impl EventSink for StubSink {
        fn emit(&self, _event: AgentEvent) {
            *self.count.lock().unwrap() += 1;
        }
    }

    #[tokio::test]
    async fn default_runtime_run() {
        let rt: Arc<dyn AgentRuntime> = Arc::new(DefaultAgentRuntime {
            llm: Arc::new(StubLlm),
            tools: Arc::new(StubTools),
            store: Arc::new(StubStore),
            events: Arc::new(StubSink { count: Mutex::new(0) }),
        });

        let req = AgentRequest {
            input: "hello".into(),
            session_id: "test".into(),
            model: None,
            metadata: HashMap::new(),
            cancel_token: None,
        };

        let result = rt.run(req).await.unwrap();
        match result {
            AgentRunResult::Finished(resp) => {
                assert_eq!(resp.finish_reason, FinishReason::Stop);
            }
        }
    }

    #[tokio::test]
    async fn default_runtime_health() {
        let rt = DefaultAgentRuntime {
            llm: Arc::new(StubLlm),
            tools: Arc::new(StubTools),
            store: Arc::new(StubStore),
            events: Arc::new(StubSink { count: Mutex::new(0) }),
        };

        let health = rt.health().await;
        assert_eq!(health.status, HealthStatus::Healthy);
    }
}
