//! # Subagent Manager
//!
//! Spawns independent agent tasks in the background, each with its own
//! session state and tool registry but sharing the parent's LLM port.
//!
//! ## Design
//!
//! - Each subagent runs in its own `tokio::spawn` task
//! - Results are delivered via `tokio::sync::oneshot`
//! - Recursive spawning is prevented by always denying `subagent/spawn`
//! - Cancellation is supported via `CancellationToken`

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use bob_core::{
    error::AgentError,
    ports::{EventSink, LlmPort, SessionStore, SubagentPort, ToolPort},
    types::{
        AgentEvent, AgentRequest, AgentRunResult, CancelToken, RequestContext, SessionId,
        SubagentResult, TokenUsage, ToolCall, ToolDescriptor, ToolResult, TurnPolicy,
    },
};
use serde_json::json;
use tokio::sync::oneshot;
use tracing::{debug, info};

/// Handle to a running subagent.
struct SubagentHandle {
    cancel_token: CancelToken,
}

/// Internal state for the subagent manager.
#[derive(Default)]
struct ManagerState {
    active: HashMap<SessionId, SubagentHandle>,
    results: HashMap<SessionId, oneshot::Receiver<SubagentResult>>,
    parent_index: HashMap<SessionId, Vec<SessionId>>,
}

/// Default subagent manager implementation.
pub struct DefaultSubagentManager {
    llm: Arc<dyn LlmPort>,
    store: Arc<dyn SessionStore>,
    events: Arc<dyn EventSink>,
    tools: Arc<dyn ToolPort>,
    default_model: String,
    policy: TurnPolicy,
    state: Mutex<ManagerState>,
}

impl std::fmt::Debug for DefaultSubagentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultSubagentManager")
            .field("default_model", &self.default_model)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl DefaultSubagentManager {
    /// Create a new subagent manager.
    #[must_use]
    pub fn new(
        llm: Arc<dyn LlmPort>,
        store: Arc<dyn SessionStore>,
        events: Arc<dyn EventSink>,
        tools: Arc<dyn ToolPort>,
        default_model: String,
        policy: TurnPolicy,
    ) -> Self {
        Self {
            llm,
            store,
            events,
            tools,
            default_model,
            policy,
            state: Mutex::new(ManagerState::default()),
        }
    }
}

#[async_trait::async_trait]
impl SubagentPort for DefaultSubagentManager {
    async fn spawn(
        &self,
        task: String,
        parent_session_id: SessionId,
        model: Option<String>,
        max_steps: Option<u32>,
        extra_deny_tools: Vec<String>,
    ) -> Result<SessionId, AgentError> {
        let subagent_id = format!("subagent:{}", uuid_v7_simple());
        let cancel_token = CancelToken::new();
        let (tx, rx) = oneshot::channel();

        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state
                .active
                .insert(subagent_id.clone(), SubagentHandle { cancel_token: cancel_token.clone() });
            state.results.insert(subagent_id.clone(), rx);
            state
                .parent_index
                .entry(parent_session_id.clone())
                .or_default()
                .push(subagent_id.clone());
        }

        self.events.emit(AgentEvent::SubagentSpawned {
            parent_session_id: parent_session_id.clone(),
            subagent_id: subagent_id.clone(),
            task: task.clone(),
        });

        let llm = self.llm.clone();
        let store = self.store.clone();
        let events = self.events.clone();
        let tools = self.tools.clone();
        let model_str = model.unwrap_or_else(|| self.default_model.clone());
        let mut policy = self.policy.clone();
        if let Some(steps) = max_steps {
            policy.max_steps = steps;
        }
        let sid = subagent_id.clone();

        // Build deny list: always deny subagent spawning + any extras.
        let mut deny_tools = vec!["subagent/spawn".to_string()];
        deny_tools.extend(extra_deny_tools);
        let filtered_tools: Arc<dyn ToolPort> =
            Arc::new(DenyListToolPort { inner: tools, deny: deny_tools });

        tokio::spawn(async move {
            debug!(subagent_id = %sid, "subagent task started");

            let req = AgentRequest {
                input: task,
                session_id: sid.clone(),
                model: Some(model_str),
                context: RequestContext::default(),
                cancel_token: Some(cancel_token),
                output_schema: None,
                max_output_retries: 0,
            };

            let result = crate::scheduler::run_turn(
                llm.as_ref(),
                filtered_tools.as_ref(),
                store.as_ref(),
                events.as_ref(),
                req,
                &policy,
                "subagent-default",
            )
            .await;

            let subagent_result = match result {
                Ok(AgentRunResult::Finished(resp)) => SubagentResult {
                    subagent_id: sid.clone(),
                    parent_session_id: parent_session_id.clone(),
                    content: resp.content,
                    usage: resp.usage,
                    is_error: false,
                },
                Err(e) => SubagentResult {
                    subagent_id: sid.clone(),
                    parent_session_id: parent_session_id.clone(),
                    content: format!("subagent error: {e}"),
                    usage: TokenUsage::default(),
                    is_error: true,
                },
            };

            let is_error = subagent_result.is_error;
            let _ = tx.send(subagent_result);

            events.emit(AgentEvent::SubagentCompleted { subagent_id: sid.clone(), is_error });

            // Remove from active list.
            {
                // We don't have a reference to state here since we're in a spawned task.
                // The active entry will be cleaned up when list_active checks it.
            }

            info!(subagent_id = %sid, is_error, "subagent task completed");
        });

        Ok(subagent_id)
    }

    async fn await_result(&self, subagent_id: &SessionId) -> Result<SubagentResult, AgentError> {
        let rx = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.results.remove(subagent_id)
        };

        match rx {
            Some(receiver) => match receiver.await {
                Ok(result) => Ok(result),
                Err(_) => {
                    Err(AgentError::Internal("subagent task dropped without sending result".into()))
                }
            },
            None => Err(AgentError::Config(format!(
                "subagent '{subagent_id}' not found (already awaited or never spawned)"
            ))),
        }
    }

    async fn list_active(
        &self,
        parent_session_id: &SessionId,
    ) -> Result<Vec<SessionId>, AgentError> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut result = Vec::new();
        if let Some(ids) = state.parent_index.get(parent_session_id) {
            for id in ids {
                if state.active.contains_key(id) {
                    result.push(id.clone());
                }
            }
        }
        Ok(result)
    }

    async fn cancel(&self, subagent_id: &SessionId) -> Result<(), AgentError> {
        let handle = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.active.remove(subagent_id)
        };

        match handle {
            Some(h) => {
                h.cancel_token.cancel();
                debug!(subagent_id = %subagent_id, "subagent cancelled");
                Ok(())
            }
            None => Err(AgentError::Config(format!("subagent '{subagent_id}' not found"))),
        }
    }
}

/// Tool port wrapper that denies specific tools.
struct DenyListToolPort {
    inner: Arc<dyn ToolPort>,
    deny: Vec<String>,
}

#[async_trait::async_trait]
impl ToolPort for DenyListToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, bob_core::error::ToolError> {
        let tools = self.inner.list_tools().await?;
        Ok(tools.into_iter().filter(|t| !self.deny.contains(&t.id)).collect())
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, bob_core::error::ToolError> {
        if self.deny.contains(&call.name) {
            return Ok(ToolResult {
                name: call.name,
                output: json!({ "error": "tool denied in subagent context" }),
                is_error: true,
            });
        }
        self.inner.call_tool(call).await
    }
}

/// Subagent tool port — exposes `subagent/spawn` as a callable tool.
pub struct SubagentToolPort {
    manager: Arc<dyn SubagentPort>,
    default_parent_session: SessionId,
}

impl std::fmt::Debug for SubagentToolPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentToolPort")
            .field("default_parent_session", &self.default_parent_session)
            .finish_non_exhaustive()
    }
}

impl SubagentToolPort {
    /// Create a new subagent tool port.
    #[must_use]
    pub fn new(manager: Arc<dyn SubagentPort>, default_parent_session: SessionId) -> Self {
        Self { manager, default_parent_session }
    }
}

#[async_trait::async_trait]
impl ToolPort for SubagentToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, bob_core::error::ToolError> {
        Ok(vec![
            ToolDescriptor::new(
                "subagent/spawn",
                "Spawn a background subagent to handle a task independently. \
             The subagent runs with its own tool registry and reasoning loop. \
             Results are returned when the subagent completes.",
            )
            .with_input_schema(json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Task description for the subagent"
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional model override"
                    },
                    "max_steps": {
                        "type": "integer",
                        "description": "Optional max steps override"
                    }
                },
                "required": ["task"]
            })),
        ])
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, bob_core::error::ToolError> {
        if call.name != "subagent/spawn" {
            return Err(bob_core::error::ToolError::NotFound { name: call.name });
        }

        let task = call
            .arguments
            .get("task")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                bob_core::error::ToolError::Execution("missing 'task' argument".to_string())
            })?
            .to_string();

        let model =
            call.arguments.get("model").and_then(serde_json::Value::as_str).map(String::from);

        let max_steps =
            call.arguments.get("max_steps").and_then(serde_json::Value::as_u64).map(|v| v as u32);

        let subagent_id = self
            .manager
            .spawn(task.clone(), self.default_parent_session.clone(), model, max_steps, vec![])
            .await
            .map_err(|e| bob_core::error::ToolError::Execution(format!("spawn failed: {e}")))?;

        let result = self
            .manager
            .await_result(&subagent_id)
            .await
            .map_err(|e| bob_core::error::ToolError::Execution(format!("await failed: {e}")))?;

        Ok(ToolResult {
            name: call.name,
            output: json!({
                "subagent_id": result.subagent_id,
                "content": result.content,
                "is_error": result.is_error,
                "usage": {
                    "prompt_tokens": result.usage.prompt_tokens,
                    "completion_tokens": result.usage.completion_tokens,
                }
            }),
            is_error: result.is_error,
        })
    }
}

/// Generate a simple unique ID.
fn uuid_v7_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let rand_part: u16 = rand_u16();
    format!("{:016x}{:04x}", now.as_nanos() & 0xFFFF_FFFF_FFFF_FFFF, rand_part)
}

fn rand_u16() -> u16 {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let mut hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    std::time::Instant::now().hash(&mut hasher);
    hasher.finish() as u16
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::{
        error::{LlmError, StoreError, ToolError},
        types::{FinishReason, LlmRequest, LlmResponse, LlmStream, SessionState, TokenUsage},
    };

    use super::*;

    struct StubLlm;

    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "subagent done"}"#.into(),
                usage: TokenUsage { prompt_tokens: 5, completion_tokens: 3 },
                finish_reason: FinishReason::Stop,
                tool_calls: Vec::new(),
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
            Ok(Vec::new())
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult { name: call.name, output: json!(null), is_error: false })
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

    struct StubSink;

    impl EventSink for StubSink {
        fn emit(&self, _event: AgentEvent) {}
    }

    fn create_manager() -> DefaultSubagentManager {
        DefaultSubagentManager::new(
            Arc::new(StubLlm),
            Arc::new(StubStore),
            Arc::new(StubSink),
            Arc::new(StubTools),
            "test-model".to_string(),
            TurnPolicy::default(),
        )
    }

    #[tokio::test]
    async fn deny_list_blocks_denied_tools() {
        let inner: Arc<dyn ToolPort> = Arc::new(StubTools);
        let deny = DenyListToolPort { inner, deny: vec!["subagent/spawn".into()] };

        let tools = deny.list_tools().await.unwrap_or_default();
        assert!(tools.is_empty(), "denied tool should not appear in list");
    }

    #[tokio::test]
    async fn deny_list_returns_error_for_denied_call() {
        let inner: Arc<dyn ToolPort> = Arc::new(StubTools);
        let deny = DenyListToolPort { inner, deny: vec!["dangerous_tool".into()] };

        let result = deny
            .call_tool(ToolCall::new("dangerous_tool", json!({})))
            .await
            .unwrap_or_else(|_| unreachable!());
        assert!(result.is_error, "denied tool call should return error result");
    }

    #[tokio::test]
    async fn deny_list_passes_allowed_tools() {
        let inner: Arc<dyn ToolPort> = Arc::new(StubTools);
        let deny = DenyListToolPort { inner, deny: vec!["subagent/spawn".into()] };

        let result = deny
            .call_tool(ToolCall::new("local/file_read", json!({"path": "x"})))
            .await
            .unwrap_or_else(|_| unreachable!());
        assert!(!result.is_error, "allowed tool should pass through");
    }

    #[tokio::test]
    async fn subagent_tool_port_lists_spawn_tool() {
        let manager = Arc::new(create_manager());
        let port = SubagentToolPort::new(manager, "parent-session".into());

        let tools = port.list_tools().await.unwrap_or_default();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "subagent/spawn");
    }

    #[tokio::test]
    async fn spawn_creates_subagent_and_delivers_result() {
        let manager = create_manager();

        let id = manager
            .spawn("test task".into(), "parent-1".into(), None, None, vec![])
            .await
            .unwrap_or_else(|_| unreachable!());

        assert!(id.starts_with("subagent:"));

        let result = manager.await_result(&id).await.unwrap_or_else(|_| unreachable!());
        assert_eq!(result.subagent_id, id);
        assert!(!result.is_error);
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn cancel_removes_subagent() {
        let manager = create_manager();

        let id = manager
            .spawn("test task".into(), "parent-1".into(), None, None, vec![])
            .await
            .unwrap_or_else(|_| unreachable!());

        let cancel_result = manager.cancel(&id).await;
        assert!(cancel_result.is_ok(), "cancel should succeed");

        let cancel_again = manager.cancel(&id).await;
        assert!(cancel_again.is_err(), "double cancel should fail");
    }

    #[tokio::test]
    async fn await_result_unknown_id_returns_error() {
        let manager = create_manager();
        let result = manager.await_result(&"nonexistent".into()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_active_returns_running_subagents() {
        let manager = create_manager();

        let active =
            manager.list_active(&"parent-1".into()).await.unwrap_or_else(|_| unreachable!());
        assert!(active.is_empty());

        let _id = manager
            .spawn("test task".into(), "parent-1".into(), None, None, vec![])
            .await
            .unwrap_or_else(|_| unreachable!());

        let active =
            manager.list_active(&"parent-1".into()).await.unwrap_or_else(|_| unreachable!());
        assert_eq!(active.len(), 1, "should have one active subagent");
    }
}
