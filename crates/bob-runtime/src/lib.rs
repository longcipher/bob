//! # Bob Runtime
//!
//! Runtime orchestration layer for the [Bob Agent Framework](https://github.com/longcipher/bob).
//!
//! ## Overview
//!
//! This crate provides the orchestration layer that coordinates agent execution:
//!
//! - **Scheduler**: Finite state machine for agent turn execution
//! - **Action Parser**: Parses LLM responses into structured actions
//! - **Prompt Builder**: Constructs prompts with tool definitions and context
//! - **Composite Tool Port**: Aggregates multiple tool sources
//!
//! This crate depends **only** on [`bob_core`] port traits — never on concrete adapters.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         AgentRuntime (trait)            │
//! ├─────────────────────────────────────────┤
//! │  ┌──────────┐  ┌──────────┐  ┌───────┐ │
//! │  │Scheduler │→ │Prompt    │→ │Action │ │
//! │  │  FSM     │  │Builder   │  │Parser │ │
//! │  └──────────┘  └──────────┘  └───────┘ │
//! └─────────────────────────────────────────┘
//!          ↓ uses ports from bob_core
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::{AgentBootstrap, AgentRuntime, RuntimeBuilder};
//! use bob_core::{
//!     ports::{LlmPort, ToolPort, SessionStore, EventSink},
//!     types::TurnPolicy,
//! };
//! use std::sync::Arc;
//!
//! fn create_runtime(
//!     llm: Arc<dyn LlmPort>,
//!     tools: Arc<dyn ToolPort>,
//!     store: Arc<dyn SessionStore>,
//!     events: Arc<dyn EventSink>,
//! ) -> Result<Arc<dyn AgentRuntime>, bob_core::error::AgentError> {
//!     RuntimeBuilder::new()
//!         .with_llm(llm)
//!         .with_tools(tools)
//!         .with_store(store)
//!         .with_events(events)
//!         .with_default_model("openai:gpt-4o-mini")
//!         .with_policy(TurnPolicy::default())
//!         .build()
//! }
//! ```
//!
//! ## Features
//!
//! - **Finite State Machine**: Robust turn execution with state tracking
//! - **Streaming Support**: Real-time event streaming via `run_stream()`
//! - **Tool Composition**: Aggregate multiple MCP servers or tool sources
//! - **Turn Policies**: Configurable limits for steps, timeouts, and retries
//! - **Health Monitoring**: Built-in health check endpoints
//!
//! ## Modules
//!
//! - [`scheduler`] - Core FSM implementation for agent execution
//! - [`action`] - Action types and parser for LLM responses
//! - [`prompt`] - Prompt construction and tool definition formatting
//! - [`composite`] - Multi-source tool aggregation
//!
//! ## Related Crates
//!
//! - [`bob_core`] - Domain types and ports
//! - [`bob_adapters`] - Concrete implementations
//!
//! [`bob_core`]: https://docs.rs/bob-core
//! [`bob_adapters`]: https://docs.rs/bob-adapters

#![forbid(unsafe_code)]

pub mod action;
pub mod agent_loop;
pub mod composite;
pub mod output_validation;
pub mod progressive_tools;
pub mod prompt;
pub mod router;
pub mod scheduler;
pub mod tooling;
pub mod typed_builder;

use std::sync::Arc;

pub use bob_core as core;
use bob_core::{
    error::{AgentError, CostError, StoreError, ToolError},
    journal::{JournalEntry, ToolJournalPort},
    ports::{
        ApprovalPort, ArtifactStorePort, ContextCompactorPort, CostMeterPort, EventSink, LlmPort,
        SessionStore, ToolPolicyPort, ToolPort, TurnCheckpointStorePort,
    },
    types::{
        AgentEventStream, AgentRequest, AgentRunResult, ApprovalContext, ApprovalDecision,
        ArtifactRecord, HealthStatus, RuntimeHealth, SessionId, ToolCall, ToolResult,
        TurnCheckpoint, TurnPolicy,
    },
};
pub use tooling::{NoOpToolPort, TimeoutToolLayer, ToolLayer};

/// Action dispatch mode for model responses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DispatchMode {
    /// Use prompt-guided JSON action protocol only.
    PromptGuided,
    /// Prefer native provider tool calls when available, fallback to prompt-guided parsing.
    #[default]
    NativePreferred,
}

/// Default static policy implementation backed by `bob-core` tool matching helpers.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DefaultToolPolicyPort;

impl ToolPolicyPort for DefaultToolPolicyPort {
    fn is_tool_allowed(
        &self,
        tool: &str,
        deny_tools: &[String],
        allow_tools: Option<&[String]>,
    ) -> bool {
        bob_core::is_tool_allowed(tool, deny_tools, allow_tools)
    }
}

/// Default approval implementation that allows every tool call.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AllowAllApprovalPort;

#[async_trait::async_trait]
impl ApprovalPort for AllowAllApprovalPort {
    async fn approve_tool_call(
        &self,
        _call: &ToolCall,
        _context: &ApprovalContext,
    ) -> Result<ApprovalDecision, ToolError> {
        Ok(ApprovalDecision::Approved)
    }
}

/// Default checkpoint store that drops all checkpoints.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoOpCheckpointStorePort;

#[async_trait::async_trait]
impl TurnCheckpointStorePort for NoOpCheckpointStorePort {
    async fn save_checkpoint(&self, _checkpoint: &TurnCheckpoint) -> Result<(), StoreError> {
        Ok(())
    }

    async fn load_latest(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<TurnCheckpoint>, StoreError> {
        Ok(None)
    }
}

/// Default artifact store that drops all artifacts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoOpArtifactStorePort;

#[async_trait::async_trait]
impl ArtifactStorePort for NoOpArtifactStorePort {
    async fn put(&self, _artifact: ArtifactRecord) -> Result<(), StoreError> {
        Ok(())
    }

    async fn list_by_session(
        &self,
        _session_id: &SessionId,
    ) -> Result<Vec<ArtifactRecord>, StoreError> {
        Ok(Vec::new())
    }
}

/// Default cost meter that never blocks and records nothing.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoOpCostMeterPort;

#[async_trait::async_trait]
impl CostMeterPort for NoOpCostMeterPort {
    async fn check_budget(&self, _session_id: &SessionId) -> Result<(), CostError> {
        Ok(())
    }

    async fn record_llm_usage(
        &self,
        _session_id: &SessionId,
        _model: &str,
        _usage: &bob_core::types::TokenUsage,
    ) -> Result<(), CostError> {
        Ok(())
    }

    async fn record_tool_result(
        &self,
        _session_id: &SessionId,
        _tool_result: &ToolResult,
    ) -> Result<(), CostError> {
        Ok(())
    }
}

/// Default journal that never records or replays.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoOpToolJournalPort;

#[async_trait::async_trait]
impl ToolJournalPort for NoOpToolJournalPort {
    async fn append(&self, _entry: JournalEntry) -> Result<(), StoreError> {
        Ok(())
    }

    async fn lookup(
        &self,
        _session_id: &SessionId,
        _fingerprint: &str,
    ) -> Result<Option<JournalEntry>, StoreError> {
        Ok(None)
    }

    async fn entries(&self, _session_id: &SessionId) -> Result<Vec<JournalEntry>, StoreError> {
        Ok(Vec::new())
    }
}

// ── Bootstrap / Builder ───────────────────────────────────────────────

/// Bootstrap contract for producing an [`AgentRuntime`].
pub trait AgentBootstrap: Send {
    /// Consume the builder and produce a ready-to-use runtime.
    fn build(self) -> Result<Arc<dyn AgentRuntime>, AgentError>
    where
        Self: Sized;
}

/// Trait-first runtime builder used by composition roots.
///
/// This keeps wiring explicit while avoiding a monolithic `main.rs`.
#[derive(Default)]
pub struct RuntimeBuilder {
    llm: Option<Arc<dyn LlmPort>>,
    tools: Option<Arc<dyn ToolPort>>,
    store: Option<Arc<dyn SessionStore>>,
    events: Option<Arc<dyn EventSink>>,
    default_model: Option<String>,
    policy: TurnPolicy,
    tool_layers: Vec<Arc<dyn ToolLayer>>,
    tool_policy: Option<Arc<dyn ToolPolicyPort>>,
    approval: Option<Arc<dyn ApprovalPort>>,
    dispatch_mode: DispatchMode,
    checkpoint_store: Option<Arc<dyn TurnCheckpointStorePort>>,
    artifact_store: Option<Arc<dyn ArtifactStorePort>>,
    cost_meter: Option<Arc<dyn CostMeterPort>>,
    context_compactor: Option<Arc<dyn ContextCompactorPort>>,
    journal: Option<Arc<dyn ToolJournalPort>>,
}

impl std::fmt::Debug for RuntimeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeBuilder")
            .field("has_llm", &self.llm.is_some())
            .field("has_tools", &self.tools.is_some())
            .field("has_store", &self.store.is_some())
            .field("has_events", &self.events.is_some())
            .field("default_model", &self.default_model)
            .field("policy", &self.policy)
            .field("tool_layers", &self.tool_layers.len())
            .field("has_tool_policy", &self.tool_policy.is_some())
            .field("has_approval", &self.approval.is_some())
            .field("dispatch_mode", &self.dispatch_mode)
            .field("has_checkpoint_store", &self.checkpoint_store.is_some())
            .field("has_artifact_store", &self.artifact_store.is_some())
            .field("has_cost_meter", &self.cost_meter.is_some())
            .field("has_context_compactor", &self.context_compactor.is_some())
            .field("has_journal", &self.journal.is_some())
            .finish()
    }
}

impl RuntimeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_llm(mut self, llm: Arc<dyn LlmPort>) -> Self {
        self.llm = Some(llm);
        self
    }

    #[must_use]
    pub fn with_tools(mut self, tools: Arc<dyn ToolPort>) -> Self {
        self.tools = Some(tools);
        self
    }

    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = Some(events);
        self
    }

    #[must_use]
    pub fn with_default_model(mut self, default_model: impl Into<String>) -> Self {
        self.default_model = Some(default_model.into());
        self
    }

    #[must_use]
    pub fn with_policy(mut self, policy: TurnPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[must_use]
    pub fn with_tool_policy(mut self, tool_policy: Arc<dyn ToolPolicyPort>) -> Self {
        self.tool_policy = Some(tool_policy);
        self
    }

    #[must_use]
    pub fn with_approval(mut self, approval: Arc<dyn ApprovalPort>) -> Self {
        self.approval = Some(approval);
        self
    }

    #[must_use]
    pub fn with_dispatch_mode(mut self, dispatch_mode: DispatchMode) -> Self {
        self.dispatch_mode = dispatch_mode;
        self
    }

    #[must_use]
    pub fn with_checkpoint_store(
        mut self,
        checkpoint_store: Arc<dyn TurnCheckpointStorePort>,
    ) -> Self {
        self.checkpoint_store = Some(checkpoint_store);
        self
    }

    #[must_use]
    pub fn with_artifact_store(mut self, artifact_store: Arc<dyn ArtifactStorePort>) -> Self {
        self.artifact_store = Some(artifact_store);
        self
    }

    #[must_use]
    pub fn with_cost_meter(mut self, cost_meter: Arc<dyn CostMeterPort>) -> Self {
        self.cost_meter = Some(cost_meter);
        self
    }

    #[must_use]
    pub fn with_context_compactor(
        mut self,
        context_compactor: Arc<dyn ContextCompactorPort>,
    ) -> Self {
        self.context_compactor = Some(context_compactor);
        self
    }

    #[must_use]
    pub fn with_journal(mut self, journal: Arc<dyn ToolJournalPort>) -> Self {
        self.journal = Some(journal);
        self
    }

    #[must_use]
    pub fn add_tool_layer(mut self, layer: Arc<dyn ToolLayer>) -> Self {
        self.tool_layers.push(layer);
        self
    }

    fn into_runtime(self) -> Result<Arc<dyn AgentRuntime>, AgentError> {
        let llm = self.llm.ok_or_else(|| AgentError::Config("missing LLM port".to_string()))?;
        let store =
            self.store.ok_or_else(|| AgentError::Config("missing session store".to_string()))?;
        let events =
            self.events.ok_or_else(|| AgentError::Config("missing event sink".to_string()))?;
        let default_model = self
            .default_model
            .ok_or_else(|| AgentError::Config("missing default model".to_string()))?;
        let tool_policy: Arc<dyn ToolPolicyPort> = self
            .tool_policy
            .unwrap_or_else(|| Arc::new(DefaultToolPolicyPort) as Arc<dyn ToolPolicyPort>);
        let approval: Arc<dyn ApprovalPort> = self
            .approval
            .unwrap_or_else(|| Arc::new(AllowAllApprovalPort) as Arc<dyn ApprovalPort>);
        let checkpoint_store: Arc<dyn TurnCheckpointStorePort> =
            self.checkpoint_store.unwrap_or_else(|| {
                Arc::new(NoOpCheckpointStorePort) as Arc<dyn TurnCheckpointStorePort>
            });
        let artifact_store: Arc<dyn ArtifactStorePort> = self
            .artifact_store
            .unwrap_or_else(|| Arc::new(NoOpArtifactStorePort) as Arc<dyn ArtifactStorePort>);
        let cost_meter: Arc<dyn CostMeterPort> = self
            .cost_meter
            .unwrap_or_else(|| Arc::new(NoOpCostMeterPort) as Arc<dyn CostMeterPort>);
        let context_compactor: Arc<dyn ContextCompactorPort> =
            self.context_compactor.unwrap_or_else(|| {
                Arc::new(crate::prompt::WindowContextCompactor::default())
                    as Arc<dyn ContextCompactorPort>
            });
        let journal: Arc<dyn ToolJournalPort> = self
            .journal
            .unwrap_or_else(|| Arc::new(NoOpToolJournalPort) as Arc<dyn ToolJournalPort>);

        let mut tools: Arc<dyn ToolPort> =
            self.tools.unwrap_or_else(|| Arc::new(NoOpToolPort) as Arc<dyn ToolPort>);
        for layer in self.tool_layers {
            tools = layer.wrap(tools);
        }

        let rt = DefaultAgentRuntime {
            llm,
            tools,
            store,
            events,
            default_model,
            policy: self.policy,
            tool_policy,
            approval,
            dispatch_mode: self.dispatch_mode,
            checkpoint_store,
            artifact_store,
            cost_meter,
            context_compactor,
            journal,
        };
        Ok(Arc::new(rt))
    }
}

impl AgentBootstrap for RuntimeBuilder {
    fn build(self) -> Result<Arc<dyn AgentRuntime>, AgentError>
    where
        Self: Sized,
    {
        self.into_runtime()
    }
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
    pub default_model: String,
    pub policy: TurnPolicy,
    pub tool_policy: Arc<dyn ToolPolicyPort>,
    pub approval: Arc<dyn ApprovalPort>,
    pub dispatch_mode: DispatchMode,
    pub checkpoint_store: Arc<dyn TurnCheckpointStorePort>,
    pub artifact_store: Arc<dyn ArtifactStorePort>,
    pub cost_meter: Arc<dyn CostMeterPort>,
    pub context_compactor: Arc<dyn ContextCompactorPort>,
    pub journal: Arc<dyn ToolJournalPort>,
}

impl std::fmt::Debug for DefaultAgentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultAgentRuntime").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl AgentRuntime for DefaultAgentRuntime {
    async fn run(&self, req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        scheduler::run_turn_with_extensions(
            self.llm.as_ref(),
            self.tools.as_ref(),
            self.store.as_ref(),
            self.events.as_ref(),
            req,
            &self.policy,
            &self.default_model,
            self.tool_policy.as_ref(),
            self.approval.as_ref(),
            self.dispatch_mode,
            self.checkpoint_store.as_ref(),
            self.artifact_store.as_ref(),
            self.cost_meter.as_ref(),
            self.context_compactor.as_ref(),
            self.journal.as_ref(),
        )
        .await
    }

    async fn run_stream(&self, req: AgentRequest) -> Result<AgentEventStream, AgentError> {
        scheduler::run_turn_stream_with_controls(
            self.llm.clone(),
            self.tools.clone(),
            self.store.clone(),
            self.events.clone(),
            req,
            self.policy.clone(),
            self.default_model.clone(),
            self.tool_policy.clone(),
            self.approval.clone(),
            self.dispatch_mode,
            self.checkpoint_store.clone(),
            self.artifact_store.clone(),
            self.cost_meter.clone(),
            self.context_compactor.clone(),
            self.journal.clone(),
        )
        .await
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use bob_core::{
        error::{LlmError, StoreError, ToolError},
        ports::ContextCompactorPort,
        types::*,
    };

    use super::*;

    // Minimal mock implementations for testing the runtime wiring.

    struct StubLlm;

    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "stub response"}"#.into(),
                usage: TokenUsage::default(),
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
            let mut count = self.count.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            *count += 1;
        }
    }

    #[derive(Default)]
    struct RecordingLlm {
        requests: Mutex<Vec<LlmRequest>>,
    }

    #[async_trait::async_trait]
    impl LlmPort for RecordingLlm {
        async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
            let mut requests =
                self.requests.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            requests.push(req);
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "recorded"}"#.into(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
                tool_calls: Vec::new(),
            })
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
        }
    }

    struct SessionFixtureStore {
        state: SessionState,
    }

    #[async_trait::async_trait]
    impl SessionStore for SessionFixtureStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(Some(self.state.clone()))
        }

        async fn save(&self, _id: &SessionId, _state: &SessionState) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct OverrideCompactor {
        invocations: AtomicUsize,
        compacted: Vec<Message>,
    }

    #[async_trait::async_trait]
    impl ContextCompactorPort for OverrideCompactor {
        async fn compact(&self, _session: &SessionState) -> Vec<Message> {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            self.compacted.clone()
        }
    }

    #[tokio::test]
    async fn default_runtime_run() {
        let rt: Arc<dyn AgentRuntime> = Arc::new(DefaultAgentRuntime {
            llm: Arc::new(StubLlm),
            tools: Arc::new(StubTools),
            store: Arc::new(StubStore),
            events: Arc::new(StubSink { count: Mutex::new(0) }),
            default_model: "test-model".into(),
            policy: TurnPolicy::default(),
            tool_policy: Arc::new(DefaultToolPolicyPort),
            approval: Arc::new(AllowAllApprovalPort),
            dispatch_mode: DispatchMode::PromptGuided,
            checkpoint_store: Arc::new(NoOpCheckpointStorePort),
            artifact_store: Arc::new(NoOpArtifactStorePort),
            cost_meter: Arc::new(NoOpCostMeterPort),
            context_compactor: Arc::new(crate::prompt::WindowContextCompactor::default()),
            journal: Arc::new(NoOpToolJournalPort),
        });

        let req = AgentRequest {
            input: "hello".into(),
            session_id: "test".into(),
            model: None,
            context: RequestContext::default(),
            cancel_token: None,
            output_schema: None,
            max_output_retries: 0,
        };

        let result = rt.run(req).await;
        assert!(
            matches!(result, Ok(AgentRunResult::Finished(_))),
            "run should finish successfully"
        );
        if let Ok(AgentRunResult::Finished(resp)) = result {
            assert_eq!(resp.finish_reason, FinishReason::Stop);
            assert_eq!(resp.content, "stub response");
        }
    }

    #[tokio::test]
    async fn default_runtime_health() {
        let rt = DefaultAgentRuntime {
            llm: Arc::new(StubLlm),
            tools: Arc::new(StubTools),
            store: Arc::new(StubStore),
            events: Arc::new(StubSink { count: Mutex::new(0) }),
            default_model: "test-model".into(),
            policy: TurnPolicy::default(),
            tool_policy: Arc::new(DefaultToolPolicyPort),
            approval: Arc::new(AllowAllApprovalPort),
            dispatch_mode: DispatchMode::PromptGuided,
            checkpoint_store: Arc::new(NoOpCheckpointStorePort),
            artifact_store: Arc::new(NoOpArtifactStorePort),
            cost_meter: Arc::new(NoOpCostMeterPort),
            context_compactor: Arc::new(crate::prompt::WindowContextCompactor::default()),
            journal: Arc::new(NoOpToolJournalPort),
        };

        let health = rt.health().await;
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn runtime_builder_requires_core_dependencies() {
        let result = RuntimeBuilder::new().build();
        assert!(
            matches!(result, Err(AgentError::Config(msg)) if msg.contains("missing LLM")),
            "missing llm should return config error"
        );
    }

    #[tokio::test]
    async fn runtime_builder_uses_custom_context_compactor() {
        let llm = Arc::new(RecordingLlm::default());
        let compactor = Arc::new(OverrideCompactor {
            invocations: AtomicUsize::new(0),
            compacted: vec![Message::text(Role::Assistant, "compacted-history")],
        });

        let runtime = RuntimeBuilder::new()
            .with_llm(llm.clone())
            .with_tools(Arc::new(StubTools))
            .with_store(Arc::new(SessionFixtureStore {
                state: SessionState {
                    messages: vec![
                        Message::text(Role::User, "original-user"),
                        Message::text(Role::Assistant, "original-assistant"),
                    ],
                    ..Default::default()
                },
            }))
            .with_events(Arc::new(StubSink { count: Mutex::new(0) }))
            .with_default_model("test-model")
            .with_context_compactor(compactor.clone())
            .build()
            .expect("runtime should build");

        let req = AgentRequest {
            input: "hello".into(),
            session_id: "test".into(),
            model: None,
            context: RequestContext::default(),
            cancel_token: None,
            output_schema: None,
            max_output_retries: 0,
        };

        let result = runtime.run(req).await;
        assert!(matches!(result, Ok(AgentRunResult::Finished(_))), "unexpected result: {result:?}");
        assert_eq!(compactor.invocations.load(Ordering::SeqCst), 1);

        let requests = llm.requests.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let request = requests.last().expect("llm should receive one request");
        assert!(request.messages.iter().any(|message| message.content == "compacted-history"));
        assert!(!request.messages.iter().any(|message| message.content == "original-assistant"));
    }
}
