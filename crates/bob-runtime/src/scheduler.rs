//! Scheduler: loop guard and 6-state turn FSM.

use bob_core::{
    error::AgentError,
    ports::{EventSink, LlmPort, SessionStore, ToolPort},
    types::{
        AgentAction, AgentEvent, AgentRequest, AgentResponse, AgentRunResult, FinishReason,
        GuardReason, Message, Role, TokenUsage, ToolCall, ToolResult, TurnPolicy,
    },
};
use tokio::time::Instant;

/// Safety net that guarantees turn termination by tracking steps,
/// tool calls, consecutive errors, and elapsed time against [`TurnPolicy`] limits.
#[derive(Debug)]
pub struct LoopGuard {
    policy: TurnPolicy,
    steps: u32,
    tool_calls: u32,
    consecutive_errors: u32,
    start: Instant,
}

impl LoopGuard {
    /// Create a new guard tied to the given policy.
    #[must_use]
    pub fn new(policy: TurnPolicy) -> Self {
        Self { policy, steps: 0, tool_calls: 0, consecutive_errors: 0, start: Instant::now() }
    }

    /// Returns `true` if the turn may continue executing.
    #[must_use]
    pub fn can_continue(&self) -> bool {
        self.steps < self.policy.max_steps &&
            self.tool_calls < self.policy.max_tool_calls &&
            self.consecutive_errors < self.policy.max_consecutive_errors &&
            !self.timed_out()
    }

    /// Record one scheduler step.
    pub fn record_step(&mut self) {
        self.steps += 1;
    }

    /// Record one tool call.
    pub fn record_tool_call(&mut self) {
        self.tool_calls += 1;
    }

    /// Record a consecutive error.
    pub fn record_error(&mut self) {
        self.consecutive_errors += 1;
    }

    /// Reset the consecutive-error counter (e.g. after a successful call).
    pub fn reset_errors(&mut self) {
        self.consecutive_errors = 0;
    }

    /// The reason the guard stopped the turn.
    ///
    /// Only meaningful when [`can_continue`](Self::can_continue) returns `false`.
    #[must_use]
    pub fn reason(&self) -> GuardReason {
        if self.steps >= self.policy.max_steps {
            GuardReason::MaxSteps
        } else if self.tool_calls >= self.policy.max_tool_calls {
            GuardReason::MaxToolCalls
        } else if self.consecutive_errors >= self.policy.max_consecutive_errors {
            GuardReason::MaxConsecutiveErrors
        } else if self.timed_out() {
            GuardReason::TurnTimeout
        } else {
            // Fallback — shouldn't be called when `can_continue()` is true.
            GuardReason::Cancelled
        }
    }

    /// Returns `true` if the turn has exceeded its time budget.
    #[must_use]
    pub fn timed_out(&self) -> bool {
        self.start.elapsed().as_millis() >= u128::from(self.policy.turn_timeout_ms)
    }
}

// ── Default system instructions (v1) ─────────────────────────────────

const DEFAULT_SYSTEM_INSTRUCTIONS: &str = "\
You are a helpful AI assistant. \
Think step by step before answering. \
When you need external information, use the available tools.";

// ── Turn Loop FSM ────────────────────────────────────────────────────

/// Execute a single agent turn as a 6-state FSM.
///
/// States: Start → BuildPrompt → LlmInfer → ParseAction → CallTool → Done.
/// The loop guard guarantees termination under all conditions.
pub(crate) async fn run_turn(
    llm: &dyn LlmPort,
    tools: &dyn ToolPort,
    store: &dyn SessionStore,
    events: &dyn EventSink,
    req: AgentRequest,
    policy: &TurnPolicy,
    default_model: &str,
) -> Result<AgentRunResult, AgentError> {
    let model = req.model.as_deref().unwrap_or(default_model);
    let cancel_token = req.cancel_token.clone();

    // ── 1. Start ─────────────────────────────────────────────────
    let mut session = store.load(&req.session_id).await?.unwrap_or_default();
    let tool_descriptors = tools.list_tools().await?;
    let mut guard = LoopGuard::new(policy.clone());

    events.emit(AgentEvent::TurnStarted { session_id: req.session_id.clone() });

    session.messages.push(Message { role: Role::User, content: req.input.clone() });

    let mut tool_transcript: Vec<ToolResult> = Vec::new();
    let mut total_usage = TokenUsage::default();
    let mut consecutive_parse_failures: u32 = 0;

    loop {
        // ── Check cancellation ───────────────────────────────────
        if let Some(ref token) = cancel_token &&
            token.is_cancelled()
        {
            return finish_turn(
                store,
                events,
                &req.session_id,
                &session,
                FinishResult {
                    content: "Turn cancelled.",
                    tool_transcript,
                    usage: total_usage,
                    finish_reason: FinishReason::Cancelled,
                },
            )
            .await;
        }

        // ── Check guard ──────────────────────────────────────────
        if !guard.can_continue() {
            let reason = guard.reason();
            let msg = format!("Turn stopped: {reason:?}");
            return finish_turn(
                store,
                events,
                &req.session_id,
                &session,
                FinishResult {
                    content: &msg,
                    tool_transcript,
                    usage: total_usage,
                    finish_reason: FinishReason::GuardExceeded,
                },
            )
            .await;
        }

        // ── 2. BuildPrompt ───────────────────────────────────────
        let llm_request = crate::prompt::build_llm_request(
            model,
            &session,
            &tool_descriptors,
            DEFAULT_SYSTEM_INSTRUCTIONS,
        );

        // ── 3. LlmInfer ─────────────────────────────────────────
        events.emit(AgentEvent::LlmCallStarted { model: model.to_string() });

        let llm_response = if let Some(ref token) = cancel_token {
            tokio::select! {
                result = llm.complete(llm_request) => result?,
                () = token.cancelled() => {
                    return finish_turn(
                        store, events, &req.session_id, &session,
                        FinishResult { content: "Turn cancelled.", tool_transcript, usage: total_usage, finish_reason: FinishReason::Cancelled },
                    ).await;
                }
            }
        } else {
            llm.complete(llm_request).await?
        };

        guard.record_step();
        total_usage.prompt_tokens += llm_response.usage.prompt_tokens;
        total_usage.completion_tokens += llm_response.usage.completion_tokens;

        events.emit(AgentEvent::LlmCallCompleted { usage: llm_response.usage.clone() });

        session
            .messages
            .push(Message { role: Role::Assistant, content: llm_response.content.clone() });

        // ── 4. ParseAction ───────────────────────────────────────
        match crate::action::parse_action(&llm_response.content) {
            Ok(action) => {
                consecutive_parse_failures = 0;

                match action {
                    AgentAction::Final { content } => {
                        return finish_turn(
                            store,
                            events,
                            &req.session_id,
                            &session,
                            FinishResult {
                                content: &content,
                                tool_transcript,
                                usage: total_usage,
                                finish_reason: FinishReason::Stop,
                            },
                        )
                        .await;
                    }
                    AgentAction::AskUser { question } => {
                        return finish_turn(
                            store,
                            events,
                            &req.session_id,
                            &session,
                            FinishResult {
                                content: &question,
                                tool_transcript,
                                usage: total_usage,
                                finish_reason: FinishReason::Stop,
                            },
                        )
                        .await;
                    }
                    AgentAction::ToolCall { name, arguments } => {
                        // ── 5. CallTool ──────────────────────────
                        events.emit(AgentEvent::ToolCallStarted { name: name.clone() });

                        let tool_call = ToolCall { name: name.clone(), arguments };

                        let tool_result = match tokio::time::timeout(
                            std::time::Duration::from_millis(policy.tool_timeout_ms),
                            tools.call_tool(tool_call),
                        )
                        .await
                        {
                            Ok(Ok(result)) => {
                                guard.reset_errors();
                                result
                            }
                            Ok(Err(e)) => {
                                guard.record_error();
                                ToolResult {
                                    name: name.clone(),
                                    output: serde_json::json!({"error": e.to_string()}),
                                    is_error: true,
                                }
                            }
                            Err(_timeout) => {
                                guard.record_error();
                                ToolResult {
                                    name: name.clone(),
                                    output: serde_json::json!({"error": "tool call timed out"}),
                                    is_error: true,
                                }
                            }
                        };

                        guard.record_tool_call();

                        let is_error = tool_result.is_error;
                        events.emit(AgentEvent::ToolCallCompleted { name: name.clone(), is_error });

                        // Append tool result to session as a Tool message.
                        let output_str =
                            serde_json::to_string(&tool_result.output).unwrap_or_default();
                        session.messages.push(Message { role: Role::Tool, content: output_str });

                        tool_transcript.push(tool_result);
                        // Loop back to BuildPrompt.
                    }
                }
            }
            Err(_parse_err) => {
                consecutive_parse_failures += 1;
                if consecutive_parse_failures >= 2 {
                    let _ = store.save(&req.session_id, &session).await;
                    return Err(AgentError::Internal(
                        "LLM produced invalid JSON after re-prompt".into(),
                    ));
                }
                // Re-prompt: ask the LLM to produce valid JSON.
                session.messages.push(Message {
                    role: Role::User,
                    content: "Your response was not valid JSON. \
                              Please respond with exactly one JSON object \
                              matching the required schema."
                        .into(),
                });
            }
        }
    }
}

/// Bundled data for building the final response (reduces argument count).
struct FinishResult<'a> {
    content: &'a str,
    tool_transcript: Vec<ToolResult>,
    usage: TokenUsage,
    finish_reason: FinishReason,
}

/// Helper: save session, emit `TurnCompleted`, and build the final response.
async fn finish_turn(
    store: &dyn SessionStore,
    events: &dyn EventSink,
    session_id: &bob_core::types::SessionId,
    session: &bob_core::types::SessionState,
    result: FinishResult<'_>,
) -> Result<AgentRunResult, AgentError> {
    let _ = store.save(session_id, session).await;
    events.emit(AgentEvent::TurnCompleted { finish_reason: result.finish_reason });
    Ok(AgentRunResult::Finished(AgentResponse {
        content: result.content.to_string(),
        tool_transcript: result.tool_transcript,
        usage: result.usage,
        finish_reason: result.finish_reason,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small policy with tight limits for fast, deterministic tests.
    fn test_policy() -> TurnPolicy {
        TurnPolicy {
            max_steps: 3,
            max_tool_calls: 2,
            max_consecutive_errors: 2,
            turn_timeout_ms: 100,
            tool_timeout_ms: 50,
        }
    }

    #[test]
    fn trips_on_max_steps() {
        let mut guard = LoopGuard::new(test_policy());
        assert!(guard.can_continue());

        for _ in 0..3 {
            guard.record_step();
        }

        assert!(!guard.can_continue(), "guard should trip after reaching max_steps");
        assert_eq!(guard.reason(), GuardReason::MaxSteps);
    }

    #[test]
    fn trips_on_max_tool_calls() {
        let mut guard = LoopGuard::new(test_policy());
        assert!(guard.can_continue());

        for _ in 0..2 {
            guard.record_tool_call();
        }

        assert!(!guard.can_continue(), "guard should trip after reaching max_tool_calls");
        assert_eq!(guard.reason(), GuardReason::MaxToolCalls);
    }

    #[test]
    fn trips_on_max_consecutive_errors() {
        let mut guard = LoopGuard::new(test_policy());
        assert!(guard.can_continue());

        for _ in 0..2 {
            guard.record_error();
        }

        assert!(!guard.can_continue(), "guard should trip after reaching max_consecutive_errors");
        assert_eq!(guard.reason(), GuardReason::MaxConsecutiveErrors);
    }

    #[tokio::test]
    async fn trips_on_timeout() {
        let guard = LoopGuard::new(test_policy());
        assert!(guard.can_continue());
        assert!(!guard.timed_out());

        // Sleep past the 100 ms timeout.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        assert!(!guard.can_continue(), "guard should trip after timeout");
        assert!(guard.timed_out());
        assert_eq!(guard.reason(), GuardReason::TurnTimeout);
    }

    #[test]
    fn reset_errors_clears_counter() {
        let mut guard = LoopGuard::new(test_policy());

        guard.record_error();
        guard.reset_errors();

        // After reset, a single error should NOT trip the guard.
        guard.record_error();
        assert!(guard.can_continue(), "single error after reset should not trip guard");
    }

    // ── run_turn FSM tests ───────────────────────────────────────

    use std::{
        collections::{HashMap, VecDeque},
        sync::Mutex,
    };

    use bob_core::{
        error::{LlmError, StoreError, ToolError},
        ports::{EventSink, LlmPort, SessionStore, ToolPort},
        types::{
            AgentEvent, AgentRequest, AgentRunResult, CancelToken, LlmRequest, LlmResponse,
            LlmStream, SessionId, SessionState, ToolCall, ToolDescriptor, ToolResult, ToolSource,
        },
    };

    // ── Mock ports ───────────────────────────────────────────────

    /// LLM mock that returns queued responses in order.
    struct SequentialLlm {
        responses: Mutex<VecDeque<Result<LlmResponse, LlmError>>>,
    }

    impl SequentialLlm {
        fn from_contents(contents: Vec<&str>) -> Self {
            let responses = contents
                .into_iter()
                .map(|c| {
                    Ok(LlmResponse {
                        content: c.to_string(),
                        usage: TokenUsage::default(),
                        finish_reason: FinishReason::Stop,
                    })
                })
                .collect();
            Self { responses: Mutex::new(responses) }
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for SequentialLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            let mut q = self.responses.lock().unwrap_or_else(|p| p.into_inner());
            q.pop_front().unwrap_or_else(|| {
                Ok(LlmResponse {
                    content: r#"{"type": "final", "content": "fallback"}"#.to_string(),
                    usage: TokenUsage::default(),
                    finish_reason: FinishReason::Stop,
                })
            })
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("not implemented".into()))
        }
    }

    /// Tool port mock with configurable tools and call results.
    struct MockToolPort {
        tools: Vec<ToolDescriptor>,
        call_results: Mutex<VecDeque<Result<ToolResult, ToolError>>>,
    }

    impl MockToolPort {
        fn empty() -> Self {
            Self { tools: vec![], call_results: Mutex::new(VecDeque::new()) }
        }

        fn with_tool_and_results(
            tool_name: &str,
            results: Vec<Result<ToolResult, ToolError>>,
        ) -> Self {
            Self {
                tools: vec![ToolDescriptor {
                    id: tool_name.to_string(),
                    description: format!("{tool_name} tool"),
                    input_schema: serde_json::json!({"type": "object"}),
                    source: ToolSource::Local,
                }],
                call_results: Mutex::new(results.into()),
            }
        }
    }

    #[async_trait::async_trait]
    impl ToolPort for MockToolPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            let mut q = self.call_results.lock().unwrap_or_else(|p| p.into_inner());
            q.pop_front().unwrap_or_else(|| {
                Ok(ToolResult {
                    name: call.name,
                    output: serde_json::json!({"result": "default"}),
                    is_error: false,
                })
            })
        }
    }

    struct MemoryStore {
        data: Mutex<HashMap<SessionId, SessionState>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self { data: Mutex::new(HashMap::new()) }
        }
    }

    #[async_trait::async_trait]
    impl SessionStore for MemoryStore {
        async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            let map = self.data.lock().unwrap_or_else(|p| p.into_inner());
            Ok(map.get(id).cloned())
        }

        async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError> {
            let mut map = self.data.lock().unwrap_or_else(|p| p.into_inner());
            map.insert(id.clone(), state.clone());
            Ok(())
        }
    }

    struct CollectingSink {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl CollectingSink {
        fn new() -> Self {
            Self { events: Mutex::new(Vec::new()) }
        }

        fn event_count(&self) -> usize {
            self.events.lock().unwrap_or_else(|p| p.into_inner()).len()
        }
    }

    impl EventSink for CollectingSink {
        fn emit(&self, event: AgentEvent) {
            self.events.lock().unwrap_or_else(|p| p.into_inner()).push(event);
        }
    }

    fn make_request(input: &str) -> AgentRequest {
        AgentRequest {
            input: input.into(),
            session_id: "test-session".into(),
            model: None,
            metadata: HashMap::new(),
            cancel_token: None,
        }
    }

    fn generous_policy() -> TurnPolicy {
        TurnPolicy {
            max_steps: 20,
            max_tool_calls: 10,
            max_consecutive_errors: 3,
            turn_timeout_ms: 30_000,
            tool_timeout_ms: 5_000,
        }
    }

    // ── TC-01: Simple Final response ─────────────────────────────

    #[tokio::test]
    async fn tc01_simple_final_response() {
        let llm =
            SequentialLlm::from_contents(vec![r#"{"type": "final", "content": "Hello there!"}"#]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("Hi"),
            &generous_policy(),
            "test-model",
        )
        .await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished, got {other:?}"),
        };

        assert_eq!(resp.content, "Hello there!");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert!(resp.tool_transcript.is_empty());
        assert!(sink.event_count() >= 3, "should emit TurnStarted, LlmCall*, TurnCompleted");
    }

    // ── TC-02: ToolCall → Final chain ────────────────────────────

    #[tokio::test]
    async fn tc02_tool_call_then_final() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "search", "arguments": {"q": "rust"}}"#,
            r#"{"type": "final", "content": "Found results."}"#,
        ]);
        let tools = MockToolPort::with_tool_and_results(
            "search",
            vec![Ok(ToolResult {
                name: "search".into(),
                output: serde_json::json!({"hits": 42}),
                is_error: false,
            })],
        );
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("Search for rust"),
            &generous_policy(),
            "test-model",
        )
        .await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished, got {other:?}"),
        };

        assert_eq!(resp.content, "Found results.");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.tool_transcript.len(), 1);
        assert_eq!(resp.tool_transcript[0].name, "search");
        assert!(!resp.tool_transcript[0].is_error);
    }

    // ── TC-03: Parse error → re-prompt → success ────────────────

    #[tokio::test]
    async fn tc03_parse_error_reprompt_success() {
        let llm = SequentialLlm::from_contents(vec![
            "This is not JSON at all.",
            r#"{"type": "final", "content": "Recovered"}"#,
        ]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("Hi"),
            &generous_policy(),
            "test-model",
        )
        .await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished after re-prompt, got {other:?}"),
        };

        assert_eq!(resp.content, "Recovered");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }

    // ── TC-04: Double parse error → AgentError ──────────────────

    #[tokio::test]
    async fn tc04_double_parse_error() {
        let llm = SequentialLlm::from_contents(vec!["not json 1", "not json 2"]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("Hi"),
            &generous_policy(),
            "test-model",
        )
        .await;

        assert!(result.is_err(), "should return error after two parse failures");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid JSON"), "error message = {msg}");
    }

    // ── TC-05: max_steps exhaustion → GuardExceeded ─────────────

    #[tokio::test]
    async fn tc05_max_steps_exhaustion() {
        // LLM always returns tool calls — the guard should stop after max_steps.
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "t1", "arguments": {}}"#,
            r#"{"type": "tool_call", "name": "t1", "arguments": {}}"#,
            r#"{"type": "tool_call", "name": "t1", "arguments": {}}"#,
            r#"{"type": "tool_call", "name": "t1", "arguments": {}}"#,
        ]);
        let tools = MockToolPort::with_tool_and_results(
            "t1",
            vec![
                Ok(ToolResult {
                    name: "t1".into(),
                    output: serde_json::json!(null),
                    is_error: false,
                }),
                Ok(ToolResult {
                    name: "t1".into(),
                    output: serde_json::json!(null),
                    is_error: false,
                }),
                Ok(ToolResult {
                    name: "t1".into(),
                    output: serde_json::json!(null),
                    is_error: false,
                }),
            ],
        );
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let policy = TurnPolicy {
            max_steps: 2,
            max_tool_calls: 10,
            max_consecutive_errors: 5,
            turn_timeout_ms: 30_000,
            tool_timeout_ms: 5_000,
        };

        let result =
            run_turn(&llm, &tools, &store, &sink, make_request("do work"), &policy, "test-model")
                .await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished with GuardExceeded, got {other:?}"),
        };

        assert_eq!(resp.finish_reason, FinishReason::GuardExceeded);
        assert!(resp.content.contains("MaxSteps"), "content = {}", resp.content);
    }

    // ── TC-06: Cancellation mid-turn → Cancelled ────────────────

    #[tokio::test]
    async fn tc06_cancellation() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "final", "content": "should not reach"}"#,
        ]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let token = CancelToken::new();
        // Cancel before running.
        token.cancel();

        let mut req = make_request("Hi");
        req.cancel_token = Some(token);

        let result =
            run_turn(&llm, &tools, &store, &sink, req, &generous_policy(), "test-model").await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished with Cancelled, got {other:?}"),
        };

        assert_eq!(resp.finish_reason, FinishReason::Cancelled);
    }

    // ── TC-07: Tool error → is_error result → LLM sees error → Final ───

    #[tokio::test]
    async fn tc07_tool_error_then_final() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "flaky_tool", "arguments": {}}"#,
            r#"{"type": "final", "content": "Recovered from tool error."}"#,
        ]);
        let tools = MockToolPort::with_tool_and_results(
            "flaky_tool",
            vec![Err(ToolError::Execution("connection refused".into()))],
        );
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("call flaky"),
            &generous_policy(),
            "test-model",
        )
        .await;

        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            other => panic!("expected Finished, got {other:?}"),
        };

        assert_eq!(resp.content, "Recovered from tool error.");
        assert_eq!(resp.tool_transcript.len(), 1);
        assert!(resp.tool_transcript[0].is_error);
    }
}
