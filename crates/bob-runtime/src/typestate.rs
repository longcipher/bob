//! # Typestate Agent Runner
//!
//! Exposes the agent turn FSM as an explicit typestate machine, enabling:
//!
//! - **Step-by-step execution**: developers can manually advance the agent
//! - **Human-in-the-loop**: pause before tool execution for approval
//! - **Distributed suspend/resume**: serialize state between steps
//! - **Compile-time safety**: invalid state transitions are rejected at build time
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::typestate::*;
//!
//! // High-level: run to completion
//! let result = AgentRunner::new(context, llm, tools)
//!     .run_to_completion()
//!     .await?;
//!
//! // Low-level: step-by-step control
//! let runner = AgentRunner::new(context, llm, tools);
//! let step = runner.infer().await?;
//! match step {
//!     AgentStepResult::Finished(runner) => {
//!         println!("done: {}", runner.response().content);
//!     }
//!     AgentStepResult::RequiresTool(runner) => {
//!         // Human-in-the-loop: ask for approval
//!         let results = execute_tools_with_approval(runner.pending_calls()).await;
//!         let runner = runner.provide_tool_results(results);
//!         // Continue to next step...
//!     }
//! }
//! ```

use bob_core::{
    error::AgentError,
    ports::{ContextCompactorPort, LlmPort, SessionStore, ToolPort},
    types::{
        AgentResponse, FinishReason, LlmRequest, Message, Role, SessionState, TokenUsage, ToolCall,
        ToolResult, TurnPolicy,
    },
};

// ── State Markers ────────────────────────────────────────────────────

/// Agent is ready to perform LLM inference.
#[derive(Debug)]
pub struct Ready;

/// Agent is waiting for tool call results before continuing.
#[derive(Debug)]
pub struct AwaitingToolCall {
    /// Pending tool calls requested by the LLM.
    pub pending_calls: Vec<ToolCall>,
    /// Native tool call IDs (if using provider-native tool calling).
    pub call_ids: Vec<Option<String>>,
}

/// Agent has finished execution with a final response.
#[derive(Debug)]
pub struct Finished {
    /// The final agent response.
    pub response: AgentResponse,
}

// ── Agent Runner ─────────────────────────────────────────────────────

/// Typestate-parameterized agent runner.
///
/// The type parameter `S` encodes the current FSM state:
/// - [`Ready`] — can call [`infer`](AgentRunner<Ready>::infer)
/// - [`AwaitingToolCall`] — can call
///   [`provide_tool_results`](AgentRunner<AwaitingToolCall>::provide_tool_results)
/// - [`Finished`] — can access the final [`response`](AgentRunner<Finished>::response)
#[derive(Debug)]
pub struct AgentRunner<S> {
    state: S,
    session: SessionState,
    context: RunnerContext,
}

/// Immutable execution context carried through the typestate machine.
#[derive(Debug, Clone)]
pub struct RunnerContext {
    pub session_id: String,
    pub model: String,
    pub system_instructions: String,
    pub policy: TurnPolicy,
    pub steps_taken: u32,
    pub tool_calls_made: u32,
    pub total_usage: TokenUsage,
    pub tool_transcript: Vec<ToolResult>,
}

/// Result of an inference step — either finished or needs tools.
pub enum AgentStepResult {
    /// Agent produced a final response.
    Finished(AgentRunner<Finished>),
    /// Agent requested tool calls that need to be executed.
    RequiresTool(AgentRunner<AwaitingToolCall>),
}

impl std::fmt::Debug for AgentStepResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finished(_) => f.write_str("AgentStepResult::Finished"),
            Self::RequiresTool(_) => f.write_str("AgentStepResult::RequiresTool"),
        }
    }
}

// ── Ready State ──────────────────────────────────────────────────────

impl AgentRunner<Ready> {
    /// Create a new runner in the `Ready` state.
    #[must_use]
    pub fn new(
        session_id: impl Into<String>,
        model: impl Into<String>,
        system_instructions: impl Into<String>,
        policy: TurnPolicy,
        session: SessionState,
    ) -> Self {
        Self {
            state: Ready,
            session,
            context: RunnerContext {
                session_id: session_id.into(),
                model: model.into(),
                system_instructions: system_instructions.into(),
                policy,
                steps_taken: 0,
                tool_calls_made: 0,
                total_usage: TokenUsage::default(),
                tool_transcript: Vec::new(),
            },
        }
    }

    /// Perform LLM inference and transition to the next state.
    ///
    /// Returns [`AgentStepResult::Finished`] if the LLM produces a final
    /// response, or [`AgentStepResult::RequiresTool`] if tool calls are
    /// needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the LLM call fails or policy limits are exceeded.
    pub async fn infer(
        mut self,
        llm: &(impl LlmPort + ?Sized),
        tools: &(impl ToolPort + ?Sized),
        compactor: &(impl ContextCompactorPort + ?Sized),
    ) -> Result<AgentStepResult, AgentError> {
        if self.context.steps_taken >= self.context.policy.max_steps {
            return Ok(AgentStepResult::Finished(AgentRunner {
                state: Finished {
                    response: AgentResponse {
                        content: "Max steps exceeded.".to_string(),
                        tool_transcript: self.context.tool_transcript.clone(),
                        usage: self.context.total_usage.clone(),
                        finish_reason: FinishReason::GuardExceeded,
                    },
                },
                session: self.session,
                context: self.context,
            }));
        }

        let tool_descriptors = tools.list_tools().await.unwrap_or_default();
        let messages = compactor.compact(&self.session).await;

        let request = LlmRequest {
            model: self.context.model.clone(),
            messages,
            tools: tool_descriptors,
            output_schema: None,
        };

        let response = llm.complete(request).await?;

        self.context.steps_taken += 1;
        self.context.total_usage.prompt_tokens =
            self.context.total_usage.prompt_tokens.saturating_add(response.usage.prompt_tokens);
        self.context.total_usage.completion_tokens = self
            .context
            .total_usage
            .completion_tokens
            .saturating_add(response.usage.completion_tokens);

        if response.tool_calls.is_empty() {
            let assistant_msg = Message::text(Role::Assistant, response.content.clone());
            self.session.messages.push(assistant_msg);

            Ok(AgentStepResult::Finished(AgentRunner {
                state: Finished {
                    response: AgentResponse {
                        content: response.content,
                        tool_transcript: self.context.tool_transcript.clone(),
                        usage: self.context.total_usage.clone(),
                        finish_reason: FinishReason::Stop,
                    },
                },
                session: self.session,
                context: self.context,
            }))
        } else {
            let call_ids: Vec<Option<String>> =
                response.tool_calls.iter().map(|c| c.call_id.clone()).collect();

            let assistant_msg =
                Message::assistant_tool_calls(response.content, response.tool_calls.clone());
            self.session.messages.push(assistant_msg);

            Ok(AgentStepResult::RequiresTool(AgentRunner {
                state: AwaitingToolCall { pending_calls: response.tool_calls, call_ids },
                session: self.session,
                context: self.context,
            }))
        }
    }

    /// Run the agent loop until completion (high-level convenience method).
    ///
    /// This is equivalent to calling [`infer`](Self::infer) in a loop until
    /// a [`Finished`] state is reached.
    ///
    /// # Errors
    ///
    /// Returns an error if any LLM call fails.
    pub async fn run_to_completion(
        self,
        llm: &(impl LlmPort + ?Sized),
        tools: &(impl ToolPort + ?Sized),
        compactor: &(impl ContextCompactorPort + ?Sized),
        store: &(impl SessionStore + ?Sized),
    ) -> Result<AgentRunner<Finished>, AgentError> {
        let mut current = self.infer(llm, tools, compactor).await?;

        loop {
            match current {
                AgentStepResult::Finished(runner) => {
                    store.save(&runner.context.session_id, &runner.session).await?;
                    return Ok(runner);
                }
                AgentStepResult::RequiresTool(runner) => {
                    let mut results = Vec::new();
                    for call in &runner.state.pending_calls {
                        match tools.call_tool(call.clone()).await {
                            Ok(result) => results.push(result),
                            Err(err) => results.push(ToolResult {
                                name: call.name.clone(),
                                output: serde_json::json!({"error": err.to_string()}),
                                is_error: true,
                            }),
                        }
                    }
                    let ready = runner.provide_tool_results(results);
                    current = ready.infer(llm, tools, compactor).await?;
                }
            }
        }
    }
}

// ── AwaitingToolCall State ───────────────────────────────────────────

impl AgentRunner<AwaitingToolCall> {
    /// Get the pending tool calls that need to be executed.
    #[must_use]
    pub fn pending_calls(&self) -> &[ToolCall] {
        &self.state.pending_calls
    }

    /// Provide tool execution results and transition back to `Ready`.
    ///
    /// The results must correspond 1:1 with the pending calls in the
    /// same order.
    #[must_use]
    pub fn provide_tool_results(mut self, results: Vec<ToolResult>) -> AgentRunner<Ready> {
        for (result, call_id) in results.iter().zip(self.state.call_ids.iter()) {
            let output_str = serde_json::to_string(&result.output).unwrap_or_default();
            self.session.messages.push(Message::tool_result(
                result.name.clone(),
                call_id.clone(),
                output_str,
            ));
            self.context.tool_calls_made += 1;
            self.context.tool_transcript.push(result.clone());
        }

        AgentRunner { state: Ready, session: self.session, context: self.context }
    }

    /// Cancel the pending tool calls and transition to `Finished`.
    #[must_use]
    pub fn cancel(self, reason: impl Into<String>) -> AgentRunner<Finished> {
        AgentRunner {
            state: Finished {
                response: AgentResponse {
                    content: reason.into(),
                    tool_transcript: self.context.tool_transcript.clone(),
                    usage: self.context.total_usage.clone(),
                    finish_reason: FinishReason::Cancelled,
                },
            },
            session: self.session,
            context: self.context,
        }
    }
}

// ── Finished State ───────────────────────────────────────────────────

impl AgentRunner<Finished> {
    /// Get a reference to the final response.
    #[must_use]
    pub fn response(&self) -> &AgentResponse {
        &self.state.response
    }

    /// Consume the runner and return the final response.
    #[must_use]
    pub fn into_response(self) -> AgentResponse {
        self.state.response
    }

    /// Get the final session state (for persistence).
    #[must_use]
    pub fn session(&self) -> &SessionState {
        &self.session
    }

    /// Get the execution context with usage stats.
    #[must_use]
    pub fn context(&self) -> &RunnerContext {
        &self.context
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use bob_core::types::ToolDescriptor;

    use super::*;

    struct StubLlm;

    impl StubLlm {
        fn finish_response(content: &str) -> bob_core::types::LlmResponse {
            bob_core::types::LlmResponse {
                content: content.to_string(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
                tool_calls: Vec::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn complete(
            &self,
            _req: LlmRequest,
        ) -> Result<bob_core::types::LlmResponse, bob_core::error::LlmError> {
            Ok(Self::finish_response("done"))
        }

        async fn complete_stream(
            &self,
            _req: LlmRequest,
        ) -> Result<bob_core::types::LlmStream, bob_core::error::LlmError> {
            Err(bob_core::error::LlmError::Provider("not implemented".into()))
        }
    }

    struct StubTools;

    #[async_trait::async_trait]
    impl ToolPort for StubTools {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, bob_core::error::ToolError> {
            Ok(vec![])
        }

        async fn call_tool(
            &self,
            call: ToolCall,
        ) -> Result<ToolResult, bob_core::error::ToolError> {
            Ok(ToolResult { name: call.name, output: serde_json::json!(null), is_error: false })
        }
    }

    struct StubCompactor;

    #[async_trait::async_trait]
    impl ContextCompactorPort for StubCompactor {
        async fn compact(&self, session: &SessionState) -> Vec<Message> {
            session.messages.clone()
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

    #[tokio::test]
    async fn ready_infer_to_finished() {
        let runner = AgentRunner::new(
            "test-session",
            "test-model",
            "You are a test assistant.",
            TurnPolicy::default(),
            SessionState::default(),
        );

        let result = runner.infer(&StubLlm, &StubTools, &StubCompactor).await;
        assert!(result.is_ok(), "infer should succeed");

        if let Ok(AgentStepResult::Finished(runner)) = result {
            assert_eq!(runner.response().content, "done");
            assert_eq!(runner.response().finish_reason, FinishReason::Stop);
        } else {
            panic!("expected Finished result");
        }
    }

    #[tokio::test]
    async fn run_to_completion() {
        let runner = AgentRunner::new(
            "test-session",
            "test-model",
            "You are a test assistant.",
            TurnPolicy::default(),
            SessionState::default(),
        );

        let result =
            runner.run_to_completion(&StubLlm, &StubTools, &StubCompactor, &StubStore).await;
        assert!(result.is_ok(), "run_to_completion should succeed");

        let finished = result.unwrap();
        assert_eq!(finished.response().content, "done");
    }

    #[test]
    fn awaiting_tool_call_provide_results() {
        let runner = AgentRunner {
            state: AwaitingToolCall {
                pending_calls: vec![ToolCall::new("test", serde_json::json!({}))],
                call_ids: vec![Some("call-1".into())],
            },
            session: SessionState::default(),
            context: RunnerContext {
                session_id: "test".into(),
                model: "test".into(),
                system_instructions: String::new(),
                policy: TurnPolicy::default(),
                steps_taken: 1,
                tool_calls_made: 0,
                total_usage: TokenUsage::default(),
                tool_transcript: Vec::new(),
            },
        };

        let results = vec![ToolResult {
            name: "test".into(),
            output: serde_json::json!({"ok": true}),
            is_error: false,
        }];

        let ready = runner.provide_tool_results(results);
        assert_eq!(ready.context.tool_calls_made, 1);
        assert_eq!(ready.session.messages.len(), 1);
    }
}
