//! # Type-State Runtime Builder
//!
//! A compile-time safe builder for [`AgentRuntime`] that enforces required
//! fields (LLM port and default model) at the type level.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use bob_runtime::typed_builder::TypedRuntimeBuilder;
//! use std::sync::Arc;
//!
//! let runtime = TypedRuntimeBuilder::new()
//!     .with_llm(llm)              // transitions to HasLlm
//!     .with_default_model("gpt")  // transitions to Ready
//!     .with_store(store)          // optional
//!     .with_events(events)        // optional
//!     .build();                   // only available on Ready
//! ```

use std::{marker::PhantomData, sync::Arc};

use bob_core::{
    journal::ToolJournalPort,
    ports::{
        ApprovalPort, ArtifactStorePort, ContextCompactorPort, CostMeterPort, EventSink, LlmPort,
        SessionStore, ToolPolicyPort, ToolPort, TurnCheckpointStorePort,
    },
    types::TurnPolicy,
};

use crate::{AgentError, AgentRuntime, DefaultAgentRuntime, DispatchMode, tooling::ToolLayer};

// ── Type-State Markers ────────────────────────────────────────────────

/// Initial state — no required fields set.
#[derive(Debug, Clone, Copy, Default)]
pub struct Empty;

/// LLM port is set, model is not.
#[derive(Debug, Clone, Copy, Default)]
pub struct HasLlm;

/// Both LLM port and default model are set — ready to build.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ready;

// ── Type-State Builder ────────────────────────────────────────────────

/// Type-state builder that enforces required fields at compile time.
///
/// `S` tracks which required fields have been set:
/// - [`Empty`]: No required fields set
/// - [`HasLlm`]: LLM port set, default model not set
/// - [`Ready`]: All required fields set, `build()` available
#[derive(Default)]
pub struct TypedRuntimeBuilder<S = Empty> {
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
    _state: PhantomData<S>,
}

impl<S> std::fmt::Debug for TypedRuntimeBuilder<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedRuntimeBuilder")
            .field("has_llm", &self.llm.is_some())
            .field("has_default_model", &self.default_model.is_some())
            .finish_non_exhaustive()
    }
}

impl TypedRuntimeBuilder<Empty> {
    /// Create a new empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            llm: None,
            tools: None,
            store: None,
            events: None,
            default_model: None,
            policy: TurnPolicy::default(),
            tool_layers: Vec::new(),
            tool_policy: None,
            approval: None,
            dispatch_mode: DispatchMode::default(),
            checkpoint_store: None,
            artifact_store: None,
            cost_meter: None,
            context_compactor: None,
            journal: None,
            _state: PhantomData,
        }
    }

    /// Set the LLM port. Transitions to [`HasLlm`] state.
    #[must_use]
    pub fn with_llm(mut self, llm: Arc<dyn LlmPort>) -> TypedRuntimeBuilder<HasLlm> {
        self.llm = Some(llm);
        TypedRuntimeBuilder {
            llm: self.llm,
            tools: self.tools,
            store: self.store,
            events: self.events,
            default_model: self.default_model,
            policy: self.policy,
            tool_layers: self.tool_layers,
            tool_policy: self.tool_policy,
            approval: self.approval,
            dispatch_mode: self.dispatch_mode,
            checkpoint_store: self.checkpoint_store,
            artifact_store: self.artifact_store,
            cost_meter: self.cost_meter,
            context_compactor: self.context_compactor,
            journal: self.journal,
            _state: PhantomData,
        }
    }
}

impl TypedRuntimeBuilder<HasLlm> {
    /// Set the default model. Transitions to [`Ready`] state.
    #[must_use]
    pub fn with_default_model(mut self, model: impl Into<String>) -> TypedRuntimeBuilder<Ready> {
        self.default_model = Some(model.into());
        TypedRuntimeBuilder {
            llm: self.llm,
            tools: self.tools,
            store: self.store,
            events: self.events,
            default_model: self.default_model,
            policy: self.policy,
            tool_layers: self.tool_layers,
            tool_policy: self.tool_policy,
            approval: self.approval,
            dispatch_mode: self.dispatch_mode,
            checkpoint_store: self.checkpoint_store,
            artifact_store: self.artifact_store,
            cost_meter: self.cost_meter,
            context_compactor: self.context_compactor,
            journal: self.journal,
            _state: PhantomData,
        }
    }

    /// Set the session store. (Stays in `HasLlm` state.)
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the event sink. (Stays in `HasLlm` state.)
    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = Some(events);
        self
    }

    /// Set the tools port.
    #[must_use]
    pub fn with_tools(mut self, tools: Arc<dyn ToolPort>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the turn policy.
    #[must_use]
    pub fn with_policy(mut self, policy: TurnPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set the dispatch mode.
    #[must_use]
    pub fn with_dispatch_mode(mut self, mode: DispatchMode) -> Self {
        self.dispatch_mode = mode;
        self
    }

    /// Add a tool layer.
    #[must_use]
    pub fn add_tool_layer(mut self, layer: Arc<dyn ToolLayer>) -> Self {
        self.tool_layers.push(layer);
        self
    }

    /// Set the tool policy port.
    #[must_use]
    pub fn with_tool_policy(mut self, policy: Arc<dyn ToolPolicyPort>) -> Self {
        self.tool_policy = Some(policy);
        self
    }

    /// Set the approval port.
    #[must_use]
    pub fn with_approval(mut self, approval: Arc<dyn ApprovalPort>) -> Self {
        self.approval = Some(approval);
        self
    }

    /// Set the checkpoint store.
    #[must_use]
    pub fn with_checkpoint_store(mut self, store: Arc<dyn TurnCheckpointStorePort>) -> Self {
        self.checkpoint_store = Some(store);
        self
    }

    /// Set the artifact store.
    #[must_use]
    pub fn with_artifact_store(mut self, store: Arc<dyn ArtifactStorePort>) -> Self {
        self.artifact_store = Some(store);
        self
    }

    /// Set the cost meter.
    #[must_use]
    pub fn with_cost_meter(mut self, meter: Arc<dyn CostMeterPort>) -> Self {
        self.cost_meter = Some(meter);
        self
    }

    /// Set the context compactor.
    #[must_use]
    pub fn with_context_compactor(mut self, compactor: Arc<dyn ContextCompactorPort>) -> Self {
        self.context_compactor = Some(compactor);
        self
    }

    /// Set the tool journal.
    #[must_use]
    pub fn with_journal(mut self, journal: Arc<dyn ToolJournalPort>) -> Self {
        self.journal = Some(journal);
        self
    }
}

impl TypedRuntimeBuilder<Ready> {
    /// Build the runtime. Only available when all required fields are set.
    pub fn build(self) -> Result<Arc<dyn AgentRuntime>, AgentError> {
        // type-state guarantees these are Some in Ready state.
        let Some(llm) = self.llm else {
            unreachable!("type-state guarantees llm is set in Ready state")
        };
        let Some(default_model) = self.default_model else {
            unreachable!("type-state guarantees default_model is set in Ready state")
        };

        let store =
            self.store.ok_or_else(|| AgentError::Config("missing session store".to_string()))?;
        let events =
            self.events.ok_or_else(|| AgentError::Config("missing event sink".to_string()))?;

        let tool_policy: Arc<dyn ToolPolicyPort> = self
            .tool_policy
            .unwrap_or_else(|| Arc::new(crate::DefaultToolPolicyPort) as Arc<dyn ToolPolicyPort>);
        let approval: Arc<dyn ApprovalPort> = self
            .approval
            .unwrap_or_else(|| Arc::new(crate::AllowAllApprovalPort) as Arc<dyn ApprovalPort>);
        let checkpoint_store: Arc<dyn TurnCheckpointStorePort> =
            self.checkpoint_store.unwrap_or_else(|| {
                Arc::new(crate::NoOpCheckpointStorePort) as Arc<dyn TurnCheckpointStorePort>
            });
        let artifact_store: Arc<dyn ArtifactStorePort> = self.artifact_store.unwrap_or_else(|| {
            Arc::new(crate::NoOpArtifactStorePort) as Arc<dyn ArtifactStorePort>
        });
        let cost_meter: Arc<dyn CostMeterPort> = self
            .cost_meter
            .unwrap_or_else(|| Arc::new(crate::NoOpCostMeterPort) as Arc<dyn CostMeterPort>);
        let context_compactor: Arc<dyn ContextCompactorPort> =
            self.context_compactor.unwrap_or_else(|| {
                Arc::new(crate::prompt::WindowContextCompactor::default())
                    as Arc<dyn ContextCompactorPort>
            });
        let journal: Arc<dyn ToolJournalPort> = self
            .journal
            .unwrap_or_else(|| Arc::new(crate::NoOpToolJournalPort) as Arc<dyn ToolJournalPort>);

        let mut tools: Arc<dyn ToolPort> =
            self.tools.unwrap_or_else(|| Arc::new(crate::NoOpToolPort) as Arc<dyn ToolPort>);
        for layer in self.tool_layers {
            tools = layer.wrap(tools);
        }

        let rt = DefaultAgentRuntime::new(
            llm,
            tools,
            store,
            events,
            default_model,
            self.policy,
            tool_policy,
            approval,
            self.dispatch_mode,
            checkpoint_store,
            artifact_store,
            cost_meter,
            context_compactor,
            journal,
        );
        Ok(Arc::new(rt))
    }
}

// Allow optional fields on Ready state too.
impl TypedRuntimeBuilder<Ready> {
    /// Set the session store. (On Ready, overrides any previous value.)
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set the event sink. (On Ready, overrides any previous value.)
    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = Some(events);
        self
    }

    /// Set the tools port.
    #[must_use]
    pub fn with_tools(mut self, tools: Arc<dyn ToolPort>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the turn policy.
    #[must_use]
    pub fn with_policy(mut self, policy: TurnPolicy) -> Self {
        self.policy = policy;
        self
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bob_core::{
        error::{LlmError, StoreError},
        types::{
            FinishReason, LlmCapabilities, LlmRequest, LlmResponse, SessionId, SessionState,
            TokenUsage,
        },
    };

    use super::*;

    struct StubLlm;

    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        fn capabilities(&self) -> LlmCapabilities {
            LlmCapabilities::default()
        }
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "ok"}"#.into(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
                tool_calls: Vec::new(),
            })
        }
        async fn complete_stream(
            &self,
            _req: LlmRequest,
        ) -> Result<bob_core::types::LlmStream, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
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
        _count: Mutex<usize>,
    }

    impl EventSink for StubSink {
        fn emit(&self, _event: bob_core::types::AgentEvent) {}
    }

    #[tokio::test]
    async fn typed_builder_enforces_required_fields() {
        let runtime = TypedRuntimeBuilder::new()
            .with_llm(Arc::new(StubLlm))
            .with_default_model("test-model")
            .with_store(Arc::new(StubStore))
            .with_events(Arc::new(StubSink { _count: Mutex::new(0) }))
            .build()
            .expect("should build");

        let result = runtime.health().await;
        assert_eq!(result.status, bob_core::types::HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn typed_builder_missing_store_errors() {
        let result = TypedRuntimeBuilder::new()
            .with_llm(Arc::new(StubLlm))
            .with_default_model("test-model")
            .with_events(Arc::new(StubSink { _count: Mutex::new(0) }))
            .build();

        assert!(result.is_err(), "missing store should return error");
    }
}
