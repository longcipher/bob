//! # Session Abstraction
//!
//! High-level session management that encapsulates conversation state,
//! eliminating the need for manual session ID and context passing.
//!
//! ## Design Principles
//!
//! 1. **Hide mechanics**: Users shouldn't know about `Tape`, `Store`, `SkillsContext` unless they
//!    want to customize (provide default abstraction layer).
//! 2. **Ownership and state isolation**: `Agent` owns immutable config and tools, `Session` owns
//!    time-varying history and context.
//! 3. **Rust conventions**: Use `impl Into<String>`, builder patterns, friendly errors.

use std::sync::Arc;

use bob_core::{
    error::AgentError,
    ports::{EventSink, SessionStore, TapeStorePort, ToolPort},
    types::{AgentRunResult, FinishReason, RequestContext, SessionState, TokenUsage},
};
use uuid::Uuid;

use crate::{AgentRuntime, agent_loop::AgentLoop};

/// Flattened response type for public API.
///
/// This hides internal implementation details like `AgentRunResult::Finished`
/// and provides a clean, simple interface.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    /// Final text content from the agent.
    pub content: String,
    /// Token usage for this turn.
    pub usage: TokenUsage,
    /// Why the turn ended.
    pub finish_reason: FinishReason,
    /// Whether this response indicates a quit command.
    pub is_quit: bool,
}

impl AgentResponse {
    /// Create a normal response.
    #[must_use]
    pub fn new(content: impl Into<String>, usage: TokenUsage, finish_reason: FinishReason) -> Self {
        Self { content: content.into(), usage, finish_reason, is_quit: false }
    }

    /// Create a quit response.
    #[must_use]
    pub fn quit() -> Self {
        Self {
            content: String::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
            is_quit: true,
        }
    }

    /// Create a command output response.
    #[must_use]
    pub fn command_output(output: impl Into<String>) -> Self {
        Self {
            content: output.into(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
            is_quit: false,
        }
    }
}

/// Agent configuration for building sessions.
///
/// This is the stateless component that holds configuration and tools.
/// Use `Agent::builder()` or `Agent::from_runtime()` to create.
pub struct Agent {
    runtime: Arc<dyn AgentRuntime>,
    tools: Arc<dyn ToolPort>,
    store: Option<Arc<dyn SessionStore>>,
    tape: Option<Arc<dyn TapeStorePort>>,
    events: Option<Arc<dyn EventSink>>,
    system_prompt: Option<String>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("has_store", &self.store.is_some())
            .field("has_tape", &self.tape.is_some())
            .field("has_system_prompt", &self.system_prompt.is_some())
            .finish_non_exhaustive()
    }
}

impl Agent {
    /// Create an Agent from pre-built runtime components.
    ///
    /// This is the most flexible constructor, allowing full control over
    /// all components.
    #[must_use]
    pub fn from_runtime(runtime: Arc<dyn AgentRuntime>, tools: Arc<dyn ToolPort>) -> AgentBuilder {
        AgentBuilder::new(runtime, tools)
    }

    /// Start a new session with auto-generated session ID.
    #[must_use]
    pub fn start_session(&self) -> Session {
        Session::new(self.clone(), format!("session-{}", Uuid::new_v4()))
    }

    /// Start a new session with a specific session ID.
    #[must_use]
    pub fn start_session_with_id(&self, session_id: impl Into<String>) -> Session {
        Session::new(self.clone(), session_id.into())
    }

    /// Get a reference to the underlying runtime.
    #[must_use]
    pub fn runtime(&self) -> &Arc<dyn AgentRuntime> {
        &self.runtime
    }

    /// Get a reference to the tool port.
    #[must_use]
    pub fn tools(&self) -> &Arc<dyn ToolPort> {
        &self.tools
    }
}

impl Clone for Agent {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            tools: self.tools.clone(),
            store: self.store.clone(),
            tape: self.tape.clone(),
            events: self.events.clone(),
            system_prompt: self.system_prompt.clone(),
        }
    }
}

/// Builder for constructing an [`Agent`] with optional components.
pub struct AgentBuilder {
    runtime: Arc<dyn AgentRuntime>,
    tools: Arc<dyn ToolPort>,
    store: Option<Arc<dyn SessionStore>>,
    tape: Option<Arc<dyn TapeStorePort>>,
    events: Option<Arc<dyn EventSink>>,
    system_prompt: Option<String>,
}

impl std::fmt::Debug for AgentBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("has_store", &self.store.is_some())
            .field("has_tape", &self.tape.is_some())
            .field("has_system_prompt", &self.system_prompt.is_some())
            .finish_non_exhaustive()
    }
}

impl AgentBuilder {
    /// Create a new builder with required components.
    #[must_use]
    pub fn new(runtime: Arc<dyn AgentRuntime>, tools: Arc<dyn ToolPort>) -> Self {
        Self { runtime, tools, store: None, tape: None, events: None, system_prompt: None }
    }

    /// Attach a session store for persistence.
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Attach a tape store for conversation recording.
    #[must_use]
    pub fn with_tape(mut self, tape: Arc<dyn TapeStorePort>) -> Self {
        self.tape = Some(tape);
        self
    }

    /// Attach an event sink for observability.
    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = Some(events);
        self
    }

    /// Set a system prompt override.
    #[must_use]
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Build the Agent.
    #[must_use]
    pub fn build(self) -> Agent {
        Agent {
            runtime: self.runtime,
            tools: self.tools,
            store: self.store,
            tape: self.tape,
            events: self.events,
            system_prompt: self.system_prompt,
        }
    }
}

/// A stateful conversation session.
///
/// Manages conversation history, session ID, and context automatically.
/// Users only need to call `chat()` with their input.
pub struct Session {
    agent: Agent,
    id: String,
    agent_loop: AgentLoop,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Session {
    /// Create a new session.
    fn new(agent: Agent, id: String) -> Self {
        let mut agent_loop = AgentLoop::new(agent.runtime.clone(), agent.tools.clone());

        if let Some(ref store) = agent.store {
            agent_loop = agent_loop.with_store(store.clone());
        }
        if let Some(ref tape) = agent.tape {
            agent_loop = agent_loop.with_tape(tape.clone());
        }
        if let Some(ref events) = agent.events {
            agent_loop = agent_loop.with_events(events.clone());
        }
        if let Some(ref prompt) = agent.system_prompt {
            agent_loop = agent_loop.with_system_prompt(prompt.clone());
        }

        Self { agent, id, agent_loop }
    }

    /// Send a message and get a response.
    ///
    /// This is the primary API for conversation. The session automatically
    /// manages context, history, and tool execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the LLM call fails or internal state is inconsistent.
    pub async fn chat(&self, input: impl Into<String>) -> Result<AgentResponse, AgentError> {
        let input = input.into();
        self.chat_with_context(input, RequestContext::default()).await
    }

    /// Send a message with an explicit request context.
    ///
    /// Use this when you need to customize tool policies, add skills context,
    /// or provide other per-request overrides.
    ///
    /// # Errors
    ///
    /// Returns an error if the LLM call fails or internal state is inconsistent.
    pub async fn chat_with_context(
        &self,
        input: impl Into<String>,
        context: RequestContext,
    ) -> Result<AgentResponse, AgentError> {
        let input = input.into();

        match self.agent_loop.handle_input_with_context(&input, &self.id, context).await {
            Ok(crate::agent_loop::AgentLoopOutput::Response(AgentRunResult::Finished(resp))) => {
                Ok(AgentResponse::new(resp.content, resp.usage, resp.finish_reason))
            }
            Ok(crate::agent_loop::AgentLoopOutput::CommandOutput(output)) => {
                Ok(AgentResponse::command_output(output))
            }
            Ok(crate::agent_loop::AgentLoopOutput::Quit) => Ok(AgentResponse::quit()),
            Err(err) => Err(err),
        }
    }

    /// Get the session ID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.id
    }

    /// Get a reference to the underlying agent.
    #[must_use]
    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    /// Reset the session (clear history but keep the same session ID).
    ///
    /// This clears the conversation history in the session store while
    /// preserving cumulative token usage.
    pub async fn reset(&self) -> Result<(), AgentError> {
        if let Some(ref store) = self.agent.store {
            let retained_usage = store
                .load(&self.id)
                .await?
                .map_or_else(TokenUsage::default, |state| state.total_usage);

            store
                .save(
                    &self.id,
                    &SessionState {
                        messages: Vec::new(),
                        total_usage: retained_usage,
                        ..Default::default()
                    },
                )
                .await?;
        }
        Ok(())
    }

    /// Start a new session with the same agent configuration.
    #[must_use]
    pub fn new_session(&self) -> Self {
        self.agent.start_session()
    }
}

#[cfg(test)]
mod tests {
    use bob_core::{
        error::{LlmError, ToolError},
        types::{LlmRequest, LlmResponse, ToolCall, ToolDescriptor, ToolResult},
    };

    use super::*;
    use crate::{AgentBootstrap, RuntimeBuilder};

    struct StubLlm;

    #[async_trait::async_trait]
    impl bob_core::ports::LlmPort for StubLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "hello from session"}"#.into(),
                usage: TokenUsage { prompt_tokens: 10, completion_tokens: 5 },
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
        async fn load(
            &self,
            _id: &bob_core::types::SessionId,
        ) -> Result<Option<SessionState>, bob_core::error::StoreError> {
            Ok(None)
        }

        async fn save(
            &self,
            _id: &bob_core::types::SessionId,
            _state: &SessionState,
        ) -> Result<(), bob_core::error::StoreError> {
            Ok(())
        }
    }

    struct StubSink;

    impl EventSink for StubSink {
        fn emit(&self, _event: bob_core::types::AgentEvent) {}
    }

    #[tokio::test]
    async fn session_chat_returns_flattened_response() {
        let runtime = RuntimeBuilder::new()
            .with_llm(Arc::new(StubLlm))
            .with_tools(Arc::new(StubTools))
            .with_store(Arc::new(StubStore))
            .with_events(Arc::new(StubSink))
            .with_default_model("test-model")
            .build()
            .expect("runtime should build");

        let agent = Agent::from_runtime(runtime, Arc::new(StubTools))
            .with_store(Arc::new(StubStore))
            .build();

        let session = agent.start_session();
        let response = session.chat("hello").await.expect("chat should succeed");

        assert_eq!(response.content, "hello from session");
        assert_eq!(response.usage.prompt_tokens, 10);
        assert_eq!(response.usage.completion_tokens, 5);
        assert!(!response.is_quit);
    }

    #[tokio::test]
    async fn session_has_unique_id() {
        let runtime = RuntimeBuilder::new()
            .with_llm(Arc::new(StubLlm))
            .with_tools(Arc::new(StubTools))
            .with_store(Arc::new(StubStore))
            .with_events(Arc::new(StubSink))
            .with_default_model("test-model")
            .build()
            .expect("runtime should build");

        let agent = Agent::from_runtime(runtime, Arc::new(StubTools))
            .with_store(Arc::new(StubStore))
            .build();

        let session1 = agent.start_session();
        let session2 = agent.start_session();

        assert_ne!(session1.session_id(), session2.session_id());
    }
}
