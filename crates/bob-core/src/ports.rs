//! Hexagonal port traits for the Bob Agent Framework.
//!
//! These are the 4 v1 boundaries that adapters must implement.
//! All async traits use `async_trait` for dyn-compatibility.

use crate::{
    error::{LlmError, StoreError, ToolError},
    types::{
        AgentEvent, LlmRequest, LlmResponse, LlmStream, SessionId, SessionState, ToolCall,
        ToolDescriptor, ToolResult,
    },
};

// ── LLM Port ─────────────────────────────────────────────────────────

/// Port for LLM inference (complete and stream).
#[async_trait::async_trait]
pub trait LlmPort: Send + Sync {
    /// Run a non-streaming inference call.
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Run a streaming inference call.
    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError>;
}

// ── Tool Port ────────────────────────────────────────────────────────

/// Port for tool discovery and execution.
#[async_trait::async_trait]
pub trait ToolPort: Send + Sync {
    /// List all available tools.
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError>;

    /// Execute a tool call and return its result.
    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError>;
}

// ── Session Store ────────────────────────────────────────────────────

/// Port for session state persistence.
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    /// Load a session by ID. Returns `None` if not found.
    async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError>;

    /// Persist a session by ID.
    async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError>;
}

// ── Event Sink ───────────────────────────────────────────────────────

/// Port for emitting observability events (fire-and-forget).
pub trait EventSink: Send + Sync {
    /// Emit an agent event. Must not block.
    fn emit(&self, event: AgentEvent);
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    // ── Mock LLM ─────────────────────────────────────────────────

    struct MockLlm {
        response: LlmResponse,
    }

    #[async_trait::async_trait]
    impl LlmPort for MockLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(self.response.clone())
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("streaming not implemented in mock".into()))
        }
    }

    // ── Mock Tool Port ───────────────────────────────────────────

    struct MockToolPort {
        tools: Vec<ToolDescriptor>,
    }

    #[async_trait::async_trait]
    impl ToolPort for MockToolPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                name: call.name,
                output: serde_json::json!({"result": "mock"}),
                is_error: false,
            })
        }
    }

    // ── Mock Session Store ───────────────────────────────────────

    struct MockSessionStore {
        inner: Mutex<std::collections::HashMap<SessionId, SessionState>>,
    }

    impl MockSessionStore {
        fn new() -> Self {
            Self { inner: Mutex::new(std::collections::HashMap::new()) }
        }
    }

    #[async_trait::async_trait]
    impl SessionStore for MockSessionStore {
        async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(self.inner.lock().unwrap().get(id).cloned())
        }

        async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError> {
            self.inner.lock().unwrap().insert(id.clone(), state.clone());
            Ok(())
        }
    }

    // ── Mock Event Sink ──────────────────────────────────────────

    struct MockEventSink {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl MockEventSink {
        fn new() -> Self {
            Self { events: Mutex::new(Vec::new()) }
        }
    }

    impl EventSink for MockEventSink {
        fn emit(&self, event: AgentEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    // ── Tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn llm_port_complete() {
        let llm: Arc<dyn LlmPort> = Arc::new(MockLlm {
            response: LlmResponse {
                content: "Hello!".into(),
                usage: crate::types::TokenUsage { prompt_tokens: 10, completion_tokens: 5 },
                finish_reason: crate::types::FinishReason::Stop,
            },
        });

        let req = LlmRequest { model: "test-model".into(), messages: vec![], tools: vec![] };

        let resp = llm.complete(req).await.unwrap();
        assert_eq!(resp.content, "Hello!");
        assert_eq!(resp.usage.total(), 15);
    }

    #[tokio::test]
    async fn tool_port_list_and_call() {
        let tools: Arc<dyn ToolPort> = Arc::new(MockToolPort {
            tools: vec![ToolDescriptor {
                id: "test/echo".into(),
                description: "Echo tool".into(),
                input_schema: serde_json::json!({}),
                source: crate::types::ToolSource::Local,
            }],
        });

        let listed = tools.list_tools().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "test/echo");

        let result = tools
            .call_tool(ToolCall {
                name: "test/echo".into(),
                arguments: serde_json::json!({"msg": "hi"}),
            })
            .await
            .unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn session_store_roundtrip() {
        let store: Arc<dyn SessionStore> = Arc::new(MockSessionStore::new());

        let id = "session-1".to_string();
        assert!(store.load(&id).await.unwrap().is_none());

        let state = SessionState {
            messages: vec![crate::types::Message {
                role: crate::types::Role::User,
                content: "hello".into(),
            }],
            ..Default::default()
        };

        store.save(&id, &state).await.unwrap();
        let loaded = store.load(&id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "hello");
    }

    #[test]
    fn event_sink_collect() {
        let sink = MockEventSink::new();
        sink.emit(AgentEvent::TurnStarted { session_id: "s1".into() });
        sink.emit(AgentEvent::Error { error: "boom".into() });

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn turn_policy_defaults() {
        let policy = crate::types::TurnPolicy::default();
        assert_eq!(policy.max_steps, 12);
        assert_eq!(policy.max_tool_calls, 8);
        assert_eq!(policy.max_consecutive_errors, 2);
        assert_eq!(policy.turn_timeout_ms, 90_000);
        assert_eq!(policy.tool_timeout_ms, 15_000);
    }
}
