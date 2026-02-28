//! # Scheduler
//!
//! Scheduler: loop guard and 6-state turn FSM.
//!
//! ## Overview
//!
//! The scheduler is the core orchestration component that manages agent turn execution.
//! It implements a finite state machine (FSM) with the following states:
//!
//! 1. **Start**: Initialize turn, load session
//! 2. **LLM Call**: Send request to LLM
//! 3. **Parse Action**: Parse LLM response into structured action
//! 4. **Execute Tool**: Run tool if action is a tool call
//! 5. **Update State**: Update session state with results
//! 6. **Loop/Finish**: Either continue or complete the turn
//!
//! ## Loop Guard
//!
//! The [`LoopGuard`] ensures turn termination by tracking:
//! - Number of steps (LLM calls)
//! - Number of tool calls
//! - Consecutive errors
//! - Elapsed time
//!
//! If any limit is exceeded, the turn is terminated with an appropriate [`GuardReason`].
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::scheduler::LoopGuard;
//! use bob_core::types::TurnPolicy;
//!
//! let policy = TurnPolicy::default();
//! let mut guard = LoopGuard::new(policy);
//!
//! while guard.can_continue() {
//!     guard.record_step();
//!     // Execute step...
//! }
//! ```

use std::sync::Arc;

use bob_core::{
    error::AgentError,
    normalize_tool_list,
    ports::{
        ApprovalPort, ArtifactStorePort, CostMeterPort, EventSink, LlmPort, SessionStore,
        ToolPolicyPort, ToolPort, TurnCheckpointStorePort,
    },
    types::{
        AgentAction, AgentEvent, AgentEventStream, AgentRequest, AgentResponse, AgentRunResult,
        AgentStreamEvent, ApprovalContext, ApprovalDecision, ArtifactRecord, FinishReason,
        GuardReason, Message, Role, TokenUsage, ToolCall, ToolResult, TurnCheckpoint, TurnPolicy,
    },
};
use futures_util::StreamExt;
use tokio::time::Instant;
use tokio_stream::wrappers::UnboundedReceiverStream;

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
        self.steps < self.policy.max_steps
            && self.tool_calls < self.policy.max_tool_calls
            && self.consecutive_errors < self.policy.max_consecutive_errors
            && !self.timed_out()
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

fn resolve_system_instructions(req: &AgentRequest) -> String {
    let Some(skills_prompt) = req.context.system_prompt.as_deref() else {
        return DEFAULT_SYSTEM_INSTRUCTIONS.to_string();
    };

    if skills_prompt.trim().is_empty() {
        DEFAULT_SYSTEM_INSTRUCTIONS.to_string()
    } else {
        format!("{DEFAULT_SYSTEM_INSTRUCTIONS}\n\n{skills_prompt}")
    }
}

fn resolve_selected_skills(req: &AgentRequest) -> Vec<String> {
    req.context.selected_skills.clone()
}

#[derive(Debug, Clone, Default)]
struct ToolCallPolicy {
    deny_tools: Vec<String>,
    allow_tools: Option<Vec<String>>,
}

fn resolve_tool_call_policy(req: &AgentRequest) -> ToolCallPolicy {
    let deny_tools =
        normalize_tool_list(req.context.tool_policy.deny_tools.iter().map(String::as_str));
    let allow_tools = req
        .context
        .tool_policy
        .allow_tools
        .as_ref()
        .map(|tools| normalize_tool_list(tools.iter().map(String::as_str)));
    ToolCallPolicy { deny_tools, allow_tools }
}

fn prompt_options_for_mode(
    dispatch_mode: crate::DispatchMode,
    llm: &dyn LlmPort,
) -> crate::prompt::PromptBuildOptions {
    match dispatch_mode {
        crate::DispatchMode::PromptGuided => crate::prompt::PromptBuildOptions::default(),
        crate::DispatchMode::NativePreferred => {
            if llm.capabilities().native_tool_calling {
                crate::prompt::PromptBuildOptions {
                    include_action_schema: false,
                    include_tool_schema: false,
                }
            } else {
                crate::prompt::PromptBuildOptions::default()
            }
        }
    }
}

fn parse_action_for_mode(
    dispatch_mode: crate::DispatchMode,
    llm: &dyn LlmPort,
    response: &bob_core::types::LlmResponse,
) -> Result<AgentAction, crate::action::ActionParseError> {
    match dispatch_mode {
        crate::DispatchMode::PromptGuided => crate::action::parse_action(&response.content),
        crate::DispatchMode::NativePreferred => {
            if llm.capabilities().native_tool_calling {
                if let Some(tool_call) = response.tool_calls.first() {
                    return Ok(AgentAction::ToolCall {
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                    });
                }
            }
            crate::action::parse_action(&response.content)
        }
    }
}

async fn execute_tool_call(
    tools: &dyn ToolPort,
    guard: &mut LoopGuard,
    tool_call: ToolCall,
    policy: &ToolCallPolicy,
    tool_policy_port: &dyn ToolPolicyPort,
    approval_port: &dyn ApprovalPort,
    approval_context: &ApprovalContext,
    timeout_ms: u64,
) -> ToolResult {
    if !tool_policy_port.is_tool_allowed(
        &tool_call.name,
        &policy.deny_tools,
        policy.allow_tools.as_deref(),
    ) {
        guard.record_error();
        return ToolResult {
            name: tool_call.name.clone(),
            output: serde_json::json!({
                "error": format!("tool '{}' denied by policy", tool_call.name)
            }),
            is_error: true,
        };
    }

    match approval_port.approve_tool_call(&tool_call, approval_context).await {
        Ok(ApprovalDecision::Approved) => {}
        Ok(ApprovalDecision::Denied { reason }) => {
            guard.record_error();
            return ToolResult {
                name: tool_call.name.clone(),
                output: serde_json::json!({"error": reason}),
                is_error: true,
            };
        }
        Err(err) => {
            guard.record_error();
            return ToolResult {
                name: tool_call.name.clone(),
                output: serde_json::json!({"error": err.to_string()}),
                is_error: true,
            };
        }
    }

    match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        tools.call_tool(tool_call.clone()),
    )
    .await
    {
        Ok(Ok(result)) => {
            guard.reset_errors();
            result
        }
        Ok(Err(err)) => {
            guard.record_error();
            ToolResult {
                name: tool_call.name,
                output: serde_json::json!({"error": err.to_string()}),
                is_error: true,
            }
        }
        Err(_) => {
            guard.record_error();
            ToolResult {
                name: tool_call.name,
                output: serde_json::json!({"error": "tool call timed out"}),
                is_error: true,
            }
        }
    }
}

// ── Turn Loop FSM ────────────────────────────────────────────────────

/// Execute a single agent turn as a 6-state FSM.
///
/// States: Start → BuildPrompt → LlmInfer → ParseAction → CallTool → Done.
/// The loop guard guarantees termination under all conditions.
pub async fn run_turn(
    llm: &dyn LlmPort,
    tools: &dyn ToolPort,
    store: &dyn SessionStore,
    events: &dyn EventSink,
    req: AgentRequest,
    policy: &TurnPolicy,
    default_model: &str,
) -> Result<AgentRunResult, AgentError> {
    let tool_policy = crate::DefaultToolPolicyPort;
    let approval = crate::AllowAllApprovalPort;
    let checkpoint_store = crate::NoOpCheckpointStorePort;
    let artifact_store = crate::NoOpArtifactStorePort;
    let cost_meter = crate::NoOpCostMeterPort;
    run_turn_with_extensions(
        llm,
        tools,
        store,
        events,
        req,
        policy,
        default_model,
        &tool_policy,
        &approval,
        crate::DispatchMode::NativePreferred,
        &checkpoint_store,
        &artifact_store,
        &cost_meter,
    )
    .await
}

/// Execute a single turn with explicit policy/approval controls.
#[allow(dead_code)]
pub(crate) async fn run_turn_with_controls(
    llm: &dyn LlmPort,
    tools: &dyn ToolPort,
    store: &dyn SessionStore,
    events: &dyn EventSink,
    req: AgentRequest,
    policy: &TurnPolicy,
    default_model: &str,
    tool_policy_port: &dyn ToolPolicyPort,
    approval_port: &dyn ApprovalPort,
) -> Result<AgentRunResult, AgentError> {
    let checkpoint_store = crate::NoOpCheckpointStorePort;
    let artifact_store = crate::NoOpArtifactStorePort;
    let cost_meter = crate::NoOpCostMeterPort;
    run_turn_with_extensions(
        llm,
        tools,
        store,
        events,
        req,
        policy,
        default_model,
        tool_policy_port,
        approval_port,
        crate::DispatchMode::PromptGuided,
        &checkpoint_store,
        &artifact_store,
        &cost_meter,
    )
    .await
}

/// Execute a single turn with all extensibility controls injected.
pub(crate) async fn run_turn_with_extensions(
    llm: &dyn LlmPort,
    tools: &dyn ToolPort,
    store: &dyn SessionStore,
    events: &dyn EventSink,
    req: AgentRequest,
    policy: &TurnPolicy,
    default_model: &str,
    tool_policy_port: &dyn ToolPolicyPort,
    approval_port: &dyn ApprovalPort,
    dispatch_mode: crate::DispatchMode,
    checkpoint_store: &dyn TurnCheckpointStorePort,
    artifact_store: &dyn ArtifactStorePort,
    cost_meter: &dyn CostMeterPort,
) -> Result<AgentRunResult, AgentError> {
    let model = req.model.as_deref().unwrap_or(default_model);
    let cancel_token = req.cancel_token.clone();
    let system_instructions = resolve_system_instructions(&req);
    let selected_skills = resolve_selected_skills(&req);
    let tool_call_policy = resolve_tool_call_policy(&req);

    let mut session = store.load(&req.session_id).await?.unwrap_or_default();
    let tool_descriptors = tools.list_tools().await?;
    let mut guard = LoopGuard::new(policy.clone());

    events.emit(AgentEvent::TurnStarted { session_id: req.session_id.clone() });
    if !selected_skills.is_empty() {
        events.emit(AgentEvent::SkillsSelected { skill_names: selected_skills.clone() });
    }

    session.messages.push(Message { role: Role::User, content: req.input.clone() });

    let mut tool_transcript: Vec<ToolResult> = Vec::new();
    let mut total_usage = TokenUsage::default();
    let mut consecutive_parse_failures: u32 = 0;

    loop {
        if let Some(ref token) = cancel_token
            && token.is_cancelled()
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

        cost_meter.check_budget(&req.session_id).await?;

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

        let llm_request = crate::prompt::build_llm_request_with_options(
            model,
            &session,
            &tool_descriptors,
            &system_instructions,
            prompt_options_for_mode(dispatch_mode, llm),
        );

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
        cost_meter.record_llm_usage(&req.session_id, model, &llm_response.usage).await?;

        events.emit(AgentEvent::LlmCallCompleted { usage: llm_response.usage.clone() });

        session
            .messages
            .push(Message { role: Role::Assistant, content: llm_response.content.clone() });

        let _ = checkpoint_store
            .save_checkpoint(&TurnCheckpoint {
                session_id: req.session_id.clone(),
                step: guard.steps,
                tool_calls: guard.tool_calls,
                usage: total_usage.clone(),
            })
            .await;

        match parse_action_for_mode(dispatch_mode, llm, &llm_response) {
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
                        events.emit(AgentEvent::ToolCallStarted { name: name.clone() });
                        let approval_context = ApprovalContext {
                            session_id: req.session_id.clone(),
                            turn_step: guard.steps.max(1),
                            selected_skills: selected_skills.clone(),
                        };

                        let tool_result = execute_tool_call(
                            tools,
                            &mut guard,
                            ToolCall { name: name.clone(), arguments },
                            &tool_call_policy,
                            tool_policy_port,
                            approval_port,
                            &approval_context,
                            policy.tool_timeout_ms,
                        )
                        .await;

                        guard.record_tool_call();
                        let _ = cost_meter.record_tool_result(&req.session_id, &tool_result).await;

                        let is_error = tool_result.is_error;
                        events.emit(AgentEvent::ToolCallCompleted { name: name.clone(), is_error });

                        let output_str =
                            serde_json::to_string(&tool_result.output).unwrap_or_default();
                        session.messages.push(Message { role: Role::Tool, content: output_str });

                        let _ = artifact_store
                            .put(ArtifactRecord {
                                session_id: req.session_id.clone(),
                                kind: "tool_result".to_string(),
                                name: name.clone(),
                                content: tool_result.output.clone(),
                            })
                            .await;

                        tool_transcript.push(tool_result);
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
    store.save(session_id, session).await?;
    events.emit(AgentEvent::TurnCompleted { finish_reason: result.finish_reason });
    Ok(AgentRunResult::Finished(AgentResponse {
        content: result.content.to_string(),
        tool_transcript: result.tool_transcript,
        usage: result.usage,
        finish_reason: result.finish_reason,
    }))
}

/// Execute a single turn in streaming mode and return an event stream.
pub async fn run_turn_stream(
    llm: Arc<dyn LlmPort>,
    tools: Arc<dyn ToolPort>,
    store: Arc<dyn SessionStore>,
    events: Arc<dyn EventSink>,
    req: AgentRequest,
    policy: TurnPolicy,
    default_model: String,
) -> Result<AgentEventStream, AgentError> {
    let tool_policy: Arc<dyn ToolPolicyPort> = Arc::new(crate::DefaultToolPolicyPort);
    let approval: Arc<dyn ApprovalPort> = Arc::new(crate::AllowAllApprovalPort);
    let checkpoint_store: Arc<dyn TurnCheckpointStorePort> =
        Arc::new(crate::NoOpCheckpointStorePort);
    let artifact_store: Arc<dyn ArtifactStorePort> = Arc::new(crate::NoOpArtifactStorePort);
    let cost_meter: Arc<dyn CostMeterPort> = Arc::new(crate::NoOpCostMeterPort);
    run_turn_stream_with_controls(
        llm,
        tools,
        store,
        events,
        req,
        policy,
        default_model,
        tool_policy,
        approval,
        crate::DispatchMode::NativePreferred,
        checkpoint_store,
        artifact_store,
        cost_meter,
    )
    .await
}

/// Execute a single turn in streaming mode with explicit policy/approval controls.
pub(crate) async fn run_turn_stream_with_controls(
    llm: Arc<dyn LlmPort>,
    tools: Arc<dyn ToolPort>,
    store: Arc<dyn SessionStore>,
    events: Arc<dyn EventSink>,
    req: AgentRequest,
    policy: TurnPolicy,
    default_model: String,
    tool_policy: Arc<dyn ToolPolicyPort>,
    approval: Arc<dyn ApprovalPort>,
    dispatch_mode: crate::DispatchMode,
    checkpoint_store: Arc<dyn TurnCheckpointStorePort>,
    artifact_store: Arc<dyn ArtifactStorePort>,
    cost_meter: Arc<dyn CostMeterPort>,
) -> Result<AgentEventStream, AgentError> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AgentStreamEvent>();
    let config = StreamRunConfig {
        policy,
        default_model,
        tool_policy,
        approval,
        dispatch_mode,
        checkpoint_store,
        artifact_store,
        cost_meter,
    };

    tokio::spawn(async move {
        if let Err(err) = run_turn_stream_inner(llm, tools, store, events, req, &config, &tx).await
        {
            let _ = tx.send(AgentStreamEvent::Error { error: err.to_string() });
        }
    });

    Ok(Box::pin(UnboundedReceiverStream::new(rx)))
}

struct StreamRunConfig {
    policy: TurnPolicy,
    default_model: String,
    tool_policy: Arc<dyn ToolPolicyPort>,
    approval: Arc<dyn ApprovalPort>,
    dispatch_mode: crate::DispatchMode,
    checkpoint_store: Arc<dyn TurnCheckpointStorePort>,
    artifact_store: Arc<dyn ArtifactStorePort>,
    cost_meter: Arc<dyn CostMeterPort>,
}

async fn run_turn_stream_inner(
    llm: Arc<dyn LlmPort>,
    tools: Arc<dyn ToolPort>,
    store: Arc<dyn SessionStore>,
    events: Arc<dyn EventSink>,
    req: AgentRequest,
    config: &StreamRunConfig,
    tx: &tokio::sync::mpsc::UnboundedSender<AgentStreamEvent>,
) -> Result<(), AgentError> {
    let model = req.model.as_deref().unwrap_or(&config.default_model);
    let cancel_token = req.cancel_token.clone();
    let system_instructions = resolve_system_instructions(&req);
    let selected_skills = resolve_selected_skills(&req);
    let tool_call_policy = resolve_tool_call_policy(&req);

    let mut session = store.load(&req.session_id).await?.unwrap_or_default();
    let tool_descriptors = tools.list_tools().await?;
    let mut guard = LoopGuard::new(config.policy.clone());
    let mut total_usage = TokenUsage::default();
    let mut consecutive_parse_failures: u32 = 0;
    let mut next_call_id: u64 = 0;

    events.emit(AgentEvent::TurnStarted { session_id: req.session_id.clone() });
    if !selected_skills.is_empty() {
        events.emit(AgentEvent::SkillsSelected { skill_names: selected_skills.clone() });
    }
    session.messages.push(Message { role: Role::User, content: req.input.clone() });

    loop {
        if let Some(ref token) = cancel_token
            && token.is_cancelled()
        {
            events.emit(AgentEvent::Error { error: "turn cancelled".to_string() });
            events.emit(AgentEvent::TurnCompleted { finish_reason: FinishReason::Cancelled });
            store.save(&req.session_id, &session).await?;
            let _ = tx.send(AgentStreamEvent::Error { error: "turn cancelled".to_string() });
            let _ = tx.send(AgentStreamEvent::Finished { usage: total_usage.clone() });
            return Ok(());
        }

        config.cost_meter.check_budget(&req.session_id).await?;

        if !guard.can_continue() {
            let reason = guard.reason();
            let msg = format!("Turn stopped: {reason:?}");
            events.emit(AgentEvent::Error { error: msg.clone() });
            events.emit(AgentEvent::TurnCompleted { finish_reason: FinishReason::GuardExceeded });
            store.save(&req.session_id, &session).await?;
            let _ = tx.send(AgentStreamEvent::Error { error: msg });
            let _ = tx.send(AgentStreamEvent::Finished { usage: total_usage.clone() });
            return Ok(());
        }

        let llm_request = crate::prompt::build_llm_request_with_options(
            model,
            &session,
            &tool_descriptors,
            &system_instructions,
            prompt_options_for_mode(config.dispatch_mode, llm.as_ref()),
        );
        events.emit(AgentEvent::LlmCallStarted { model: model.to_string() });

        let mut assistant_content = String::new();
        let mut llm_usage = TokenUsage::default();
        let mut llm_tool_calls: Vec<ToolCall> = Vec::new();
        let mut fallback_to_complete = false;

        match llm.complete_stream(llm_request.clone()).await {
            Ok(mut stream) => {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(bob_core::types::LlmStreamChunk::TextDelta(delta)) => {
                            assistant_content.push_str(&delta);
                            let _ = tx.send(AgentStreamEvent::TextDelta { content: delta });
                        }
                        Ok(bob_core::types::LlmStreamChunk::Done { usage }) => {
                            llm_usage = usage;
                        }
                        Err(err) => {
                            events.emit(AgentEvent::Error { error: err.to_string() });
                            return Err(AgentError::Llm(err));
                        }
                    }
                }
            }
            Err(err) => {
                fallback_to_complete = true;
                events.emit(AgentEvent::Error { error: err.to_string() });
            }
        }

        // Provider may not support streaming — fall back to non-streaming complete.
        if fallback_to_complete {
            let llm_response = llm.complete(llm_request).await?;
            assistant_content = llm_response.content.clone();
            llm_usage = llm_response.usage;
            llm_tool_calls = llm_response.tool_calls;
            let _ = tx.send(AgentStreamEvent::TextDelta { content: llm_response.content });
        }

        guard.record_step();
        total_usage.prompt_tokens += llm_usage.prompt_tokens;
        total_usage.completion_tokens += llm_usage.completion_tokens;
        config.cost_meter.record_llm_usage(&req.session_id, model, &llm_usage).await?;
        events.emit(AgentEvent::LlmCallCompleted { usage: llm_usage.clone() });
        session
            .messages
            .push(Message { role: Role::Assistant, content: assistant_content.clone() });

        let _ = config
            .checkpoint_store
            .save_checkpoint(&TurnCheckpoint {
                session_id: req.session_id.clone(),
                step: guard.steps,
                tool_calls: guard.tool_calls,
                usage: total_usage.clone(),
            })
            .await;

        let response_for_dispatch = bob_core::types::LlmResponse {
            content: assistant_content.clone(),
            usage: llm_usage.clone(),
            finish_reason: FinishReason::Stop,
            tool_calls: llm_tool_calls,
        };

        if let Ok(action) =
            parse_action_for_mode(config.dispatch_mode, llm.as_ref(), &response_for_dispatch)
        {
            consecutive_parse_failures = 0;
            match action {
                AgentAction::Final { .. } | AgentAction::AskUser { .. } => {
                    store.save(&req.session_id, &session).await?;
                    events.emit(AgentEvent::TurnCompleted { finish_reason: FinishReason::Stop });
                    let _ = tx.send(AgentStreamEvent::Finished { usage: total_usage.clone() });
                    return Ok(());
                }
                AgentAction::ToolCall { name, arguments } => {
                    events.emit(AgentEvent::ToolCallStarted { name: name.clone() });
                    next_call_id += 1;
                    let call_id = format!("call-{next_call_id}");
                    let _ = tx.send(AgentStreamEvent::ToolCallStarted {
                        name: name.clone(),
                        call_id: call_id.clone(),
                    });
                    let approval_context = ApprovalContext {
                        session_id: req.session_id.clone(),
                        turn_step: guard.steps.max(1),
                        selected_skills: selected_skills.clone(),
                    };

                    let tool_result = execute_tool_call(
                        tools.as_ref(),
                        &mut guard,
                        ToolCall { name: name.clone(), arguments },
                        &tool_call_policy,
                        config.tool_policy.as_ref(),
                        config.approval.as_ref(),
                        &approval_context,
                        config.policy.tool_timeout_ms,
                    )
                    .await;

                    guard.record_tool_call();
                    let _ =
                        config.cost_meter.record_tool_result(&req.session_id, &tool_result).await;
                    let is_error = tool_result.is_error;
                    events.emit(AgentEvent::ToolCallCompleted { name: name.clone(), is_error });
                    let _ = tx.send(AgentStreamEvent::ToolCallCompleted {
                        call_id,
                        result: tool_result.clone(),
                    });

                    let output_str = serde_json::to_string(&tool_result.output).unwrap_or_default();
                    session.messages.push(Message { role: Role::Tool, content: output_str });
                    let _ = config
                        .artifact_store
                        .put(ArtifactRecord {
                            session_id: req.session_id.clone(),
                            kind: "tool_result".to_string(),
                            name: name.clone(),
                            content: tool_result.output.clone(),
                        })
                        .await;
                }
            }
        } else {
            consecutive_parse_failures += 1;
            if consecutive_parse_failures >= 2 {
                store.save(&req.session_id, &session).await?;
                events.emit(AgentEvent::Error {
                    error: "LLM produced invalid JSON after re-prompt".to_string(),
                });
                return Err(AgentError::Internal(
                    "LLM produced invalid JSON after re-prompt".into(),
                ));
            }
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
        sync::{Arc, Mutex},
    };

    use bob_core::{
        error::{CostError, LlmError, StoreError, ToolError},
        ports::{
            ApprovalPort, ArtifactStorePort, CostMeterPort, EventSink, LlmPort, SessionStore,
            ToolPolicyPort, ToolPort, TurnCheckpointStorePort,
        },
        types::{
            AgentEvent, AgentRequest, AgentRunResult, AgentStreamEvent, ApprovalContext,
            ApprovalDecision, ArtifactRecord, CancelToken, LlmRequest, LlmResponse, LlmStream,
            LlmStreamChunk, SessionId, SessionState, ToolCall, ToolDescriptor, ToolResult,
            ToolSource, TurnCheckpoint,
        },
    };
    use futures_util::StreamExt;

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
                        tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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

    struct NoCallToolPort {
        tools: Vec<ToolDescriptor>,
    }

    #[async_trait::async_trait]
    impl ToolPort for NoCallToolPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(&self, _call: ToolCall) -> Result<ToolResult, ToolError> {
            Err(ToolError::Execution(
                "tool call should be blocked by policy before execution".to_string(),
            ))
        }
    }

    struct AllowAllPolicyPort;

    impl ToolPolicyPort for AllowAllPolicyPort {
        fn is_tool_allowed(
            &self,
            _tool: &str,
            _deny_tools: &[String],
            _allow_tools: Option<&[String]>,
        ) -> bool {
            true
        }
    }

    struct DenySearchPolicyPort;

    impl ToolPolicyPort for DenySearchPolicyPort {
        fn is_tool_allowed(
            &self,
            tool: &str,
            _deny_tools: &[String],
            _allow_tools: Option<&[String]>,
        ) -> bool {
            tool != "search"
        }
    }

    struct AlwaysApprovePort;

    #[async_trait::async_trait]
    impl ApprovalPort for AlwaysApprovePort {
        async fn approve_tool_call(
            &self,
            _call: &ToolCall,
            _context: &ApprovalContext,
        ) -> Result<ApprovalDecision, ToolError> {
            Ok(ApprovalDecision::Approved)
        }
    }

    struct AlwaysDenyApprovalPort;

    #[async_trait::async_trait]
    impl ApprovalPort for AlwaysDenyApprovalPort {
        async fn approve_tool_call(
            &self,
            _call: &ToolCall,
            _context: &ApprovalContext,
        ) -> Result<ApprovalDecision, ToolError> {
            Ok(ApprovalDecision::Denied {
                reason: "approval policy rejected tool call".to_string(),
            })
        }
    }

    struct CountingCheckpointPort {
        saved: Mutex<Vec<TurnCheckpoint>>,
    }

    impl CountingCheckpointPort {
        fn new() -> Self {
            Self { saved: Mutex::new(Vec::new()) }
        }
    }

    #[async_trait::async_trait]
    impl TurnCheckpointStorePort for CountingCheckpointPort {
        async fn save_checkpoint(&self, checkpoint: &TurnCheckpoint) -> Result<(), StoreError> {
            self.saved.lock().unwrap_or_else(|p| p.into_inner()).push(checkpoint.clone());
            Ok(())
        }

        async fn load_latest(
            &self,
            _session_id: &SessionId,
        ) -> Result<Option<TurnCheckpoint>, StoreError> {
            Ok(None)
        }
    }

    struct NoopArtifactStore;

    #[async_trait::async_trait]
    impl ArtifactStorePort for NoopArtifactStore {
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

    struct CountingCostMeter {
        llm_calls: Mutex<u32>,
    }

    impl CountingCostMeter {
        fn new() -> Self {
            Self { llm_calls: Mutex::new(0) }
        }
    }

    #[async_trait::async_trait]
    impl CostMeterPort for CountingCostMeter {
        async fn check_budget(&self, _session_id: &SessionId) -> Result<(), CostError> {
            Ok(())
        }

        async fn record_llm_usage(
            &self,
            _session_id: &SessionId,
            _model: &str,
            _usage: &TokenUsage,
        ) -> Result<(), CostError> {
            let mut count = self.llm_calls.lock().unwrap_or_else(|p| p.into_inner());
            *count += 1;
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

    struct MemoryStore {
        data: Mutex<HashMap<SessionId, SessionState>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self { data: Mutex::new(HashMap::new()) }
        }
    }

    struct FailingSaveStore;

    #[async_trait::async_trait]
    impl SessionStore for FailingSaveStore {
        async fn load(&self, _id: &SessionId) -> Result<Option<SessionState>, StoreError> {
            Ok(None)
        }

        async fn save(&self, _id: &SessionId, _state: &SessionState) -> Result<(), StoreError> {
            Err(StoreError::Backend("simulated save failure".into()))
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

        fn all_events(&self) -> Vec<AgentEvent> {
            self.events.lock().unwrap_or_else(|p| p.into_inner()).clone()
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
            context: bob_core::types::RequestContext::default(),
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

    struct StreamLlm {
        chunks: Mutex<VecDeque<Result<LlmStreamChunk, LlmError>>>,
    }

    impl StreamLlm {
        fn new(chunks: Vec<Result<LlmStreamChunk, LlmError>>) -> Self {
            Self { chunks: Mutex::new(chunks.into()) }
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for StreamLlm {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::Provider("complete() should not be called in stream test".into()))
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            let mut chunks = self.chunks.lock().unwrap_or_else(|p| p.into_inner());
            let items: Vec<Result<LlmStreamChunk, LlmError>> = chunks.drain(..).collect();
            Ok(Box::pin(futures_util::stream::iter(items)))
        }
    }

    struct InspectingLlm {
        expected_substring: String,
    }

    #[async_trait::async_trait]
    impl LlmPort for InspectingLlm {
        async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
            let system = req
                .messages
                .iter()
                .find(|m| m.role == Role::System)
                .map(|m| m.content.clone())
                .unwrap_or_default();
            if !system.contains(&self.expected_substring) {
                return Err(LlmError::Provider(format!(
                    "expected system prompt to include '{}', got: {}",
                    self.expected_substring, system
                )));
            }
            Ok(LlmResponse {
                content: r#"{"type": "final", "content": "ok"}"#.to_string(),
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
                tool_calls: Vec::new(),
            })
        }

        async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
            Err(LlmError::Provider("not used".into()))
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished after re-prompt, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
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
        let msg = match result {
            Err(err) => err.to_string(),
            Ok(value) => format!("unexpected success: {value:?}"),
        };
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished with GuardExceeded, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished with Cancelled, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
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

        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected Finished, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
        };

        assert_eq!(resp.content, "Recovered from tool error.");
        assert_eq!(resp.tool_transcript.len(), 1);
        assert!(resp.tool_transcript[0].is_error);
    }

    #[tokio::test]
    async fn tc08_save_failure_is_propagated() {
        let llm = SequentialLlm::from_contents(vec![r#"{"type": "final", "content": "done"}"#]);
        let tools = MockToolPort::empty();
        let store = FailingSaveStore;
        let sink = CollectingSink::new();

        let result = run_turn(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("hello"),
            &generous_policy(),
            "test-model",
        )
        .await;

        assert!(matches!(result, Err(AgentError::Store(_))), "expected Store error to be returned");
    }

    #[tokio::test]
    async fn tc09_stream_turn_emits_text_and_finished() {
        let llm: Arc<dyn LlmPort> = Arc::new(StreamLlm::new(vec![
            Ok(LlmStreamChunk::TextDelta("{\"type\":\"final\",\"content\":\"he".into())),
            Ok(LlmStreamChunk::TextDelta("llo\"}".into())),
            Ok(LlmStreamChunk::Done {
                usage: TokenUsage { prompt_tokens: 3, completion_tokens: 4 },
            }),
        ]));
        let tools: Arc<dyn ToolPort> = Arc::new(MockToolPort::empty());
        let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
        let sink: Arc<dyn EventSink> = Arc::new(CollectingSink::new());

        let stream_result = run_turn_stream(
            llm,
            tools,
            store,
            sink,
            make_request("hello"),
            generous_policy(),
            "test-model".to_string(),
        )
        .await;
        assert!(stream_result.is_ok(), "run_turn_stream should produce a stream");
        let mut stream = match stream_result {
            Ok(stream) => stream,
            Err(_) => return,
        };

        let mut saw_text = false;
        let mut saw_finished = false;
        while let Some(event) = stream.next().await {
            match event {
                AgentStreamEvent::TextDelta { content } => {
                    saw_text = saw_text || !content.is_empty();
                }
                AgentStreamEvent::Finished { usage } => {
                    saw_finished = true;
                    assert_eq!(usage.prompt_tokens, 3);
                    assert_eq!(usage.completion_tokens, 4);
                }
                AgentStreamEvent::ToolCallStarted { .. }
                | AgentStreamEvent::ToolCallCompleted { .. }
                | AgentStreamEvent::Error { .. } => {}
            }
        }

        assert!(saw_text, "expected at least one text delta");
        assert!(saw_finished, "expected a finished event");
    }

    #[tokio::test]
    async fn tc10_skills_prompt_context_is_injected() {
        let llm = InspectingLlm { expected_substring: "Skill: rust-review".to_string() };
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let mut req = make_request("review this code");
        req.context.system_prompt = Some("Skill: rust-review\nUse strict checks.".to_string());

        let result =
            run_turn(&llm, &tools, &store, &sink, req, &generous_policy(), "test-model").await;

        assert!(result.is_ok(), "run should succeed when skills prompt is injected");
    }

    #[tokio::test]
    async fn tc11_selected_skills_context_emits_event() {
        let llm =
            SequentialLlm::from_contents(vec![r#"{"type": "final", "content": "looks good"}"#]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let mut req = make_request("review code");
        req.context.selected_skills = vec!["rust-review".to_string(), "security-audit".to_string()];

        let result =
            run_turn(&llm, &tools, &store, &sink, req, &generous_policy(), "test-model").await;
        assert!(result.is_ok(), "run should succeed");

        let events = sink.all_events();
        assert!(
            events.iter().any(|event| matches!(
                event,
                AgentEvent::SkillsSelected { skill_names }
                    if skill_names == &vec!["rust-review".to_string(), "security-audit".to_string()]
            )),
            "skills.selected event should be emitted with context skill names"
        );
    }

    #[tokio::test]
    async fn tc12_policy_deny_tool_blocks_execution() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "search", "arguments": {"q": "rust"}}"#,
            r#"{"type": "final", "content": "done"}"#,
        ]);
        let tools = NoCallToolPort {
            tools: vec![ToolDescriptor {
                id: "search".to_string(),
                description: "search tool".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
                source: ToolSource::Local,
            }],
        };
        let store = MemoryStore::new();
        let sink = CollectingSink::new();

        let mut req = make_request("search rust");
        req.context.tool_policy.deny_tools =
            vec!["search".to_string(), "local/shell_exec".to_string()];

        let result =
            run_turn(&llm, &tools, &store, &sink, req, &generous_policy(), "test-model").await;
        assert!(
            matches!(&result, Ok(AgentRunResult::Finished(_))),
            "expected finished response, got {result:?}"
        );
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
        };

        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.tool_transcript.len(), 1);
        assert!(resp.tool_transcript[0].is_error);
        assert!(
            resp.tool_transcript[0].output.to_string().contains("denied"),
            "tool error should explain policy denial"
        );
    }

    #[tokio::test]
    async fn tc13_approval_denied_blocks_execution() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "search", "arguments": {"q": "rust"}}"#,
            r#"{"type": "final", "content": "done"}"#,
        ]);
        let tools = NoCallToolPort {
            tools: vec![ToolDescriptor {
                id: "search".to_string(),
                description: "search tool".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
                source: ToolSource::Local,
            }],
        };
        let store = MemoryStore::new();
        let sink = CollectingSink::new();
        let req = make_request("search rust");
        let tool_policy = AllowAllPolicyPort;
        let approval = AlwaysDenyApprovalPort;

        let result = run_turn_with_controls(
            &llm,
            &tools,
            &store,
            &sink,
            req,
            &generous_policy(),
            "test-model",
            &tool_policy,
            &approval,
        )
        .await;
        assert!(matches!(&result, Ok(AgentRunResult::Finished(_))), "unexpected result {result:?}");
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
        };

        assert_eq!(resp.tool_transcript.len(), 1);
        assert!(resp.tool_transcript[0].is_error);
        assert!(
            resp.tool_transcript[0].output.to_string().contains("approval policy rejected"),
            "tool error should explain approval denial"
        );
    }

    #[tokio::test]
    async fn tc14_custom_policy_port_blocks_execution() {
        let llm = SequentialLlm::from_contents(vec![
            r#"{"type": "tool_call", "name": "search", "arguments": {"q": "rust"}}"#,
            r#"{"type": "final", "content": "done"}"#,
        ]);
        let tools = NoCallToolPort {
            tools: vec![ToolDescriptor {
                id: "search".to_string(),
                description: "search tool".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
                source: ToolSource::Local,
            }],
        };
        let store = MemoryStore::new();
        let sink = CollectingSink::new();
        let req = make_request("search rust");
        let tool_policy = DenySearchPolicyPort;
        let approval = AlwaysApprovePort;

        let result = run_turn_with_controls(
            &llm,
            &tools,
            &store,
            &sink,
            req,
            &generous_policy(),
            "test-model",
            &tool_policy,
            &approval,
        )
        .await;
        assert!(matches!(&result, Ok(AgentRunResult::Finished(_))), "unexpected result {result:?}");
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
        };

        assert_eq!(resp.tool_transcript.len(), 1);
        assert!(resp.tool_transcript[0].is_error);
        assert!(
            resp.tool_transcript[0].output.to_string().contains("denied"),
            "tool error should explain policy denial"
        );
    }

    #[tokio::test]
    async fn tc15_native_dispatch_mode_uses_llm_tool_calls() {
        struct NativeToolLlm {
            responses: Mutex<VecDeque<LlmResponse>>,
        }

        #[async_trait::async_trait]
        impl LlmPort for NativeToolLlm {
            fn capabilities(&self) -> bob_core::types::LlmCapabilities {
                bob_core::types::LlmCapabilities { native_tool_calling: true, streaming: true }
            }

            async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
                let mut q = self.responses.lock().unwrap_or_else(|p| p.into_inner());
                Ok(q.pop_front().unwrap_or(LlmResponse {
                    content: r#"{"type":"final","content":"fallback"}"#.to_string(),
                    usage: TokenUsage::default(),
                    finish_reason: FinishReason::Stop,
                    tool_calls: Vec::new(),
                }))
            }

            async fn complete_stream(&self, _req: LlmRequest) -> Result<LlmStream, LlmError> {
                Err(LlmError::Provider("not used".into()))
            }
        }

        let llm = NativeToolLlm {
            responses: Mutex::new(VecDeque::from(vec![
                LlmResponse {
                    content: "ignored".to_string(),
                    usage: TokenUsage::default(),
                    finish_reason: FinishReason::Stop,
                    tool_calls: vec![ToolCall {
                        name: "search".to_string(),
                        arguments: serde_json::json!({"q":"rust"}),
                    }],
                },
                LlmResponse {
                    content: r#"{"type":"final","content":"done"}"#.to_string(),
                    usage: TokenUsage::default(),
                    finish_reason: FinishReason::Stop,
                    tool_calls: Vec::new(),
                },
            ])),
        };
        let tools = MockToolPort::with_tool_and_results(
            "search",
            vec![Ok(ToolResult {
                name: "search".to_string(),
                output: serde_json::json!({"hits": 2}),
                is_error: false,
            })],
        );
        let store = MemoryStore::new();
        let sink = CollectingSink::new();
        let checkpoint = CountingCheckpointPort::new();
        let artifacts = NoopArtifactStore;
        let cost = CountingCostMeter::new();
        let policy = AllowAllPolicyPort;
        let approval = AlwaysApprovePort;

        let result = run_turn_with_extensions(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("search rust"),
            &generous_policy(),
            "test-model",
            &policy,
            &approval,
            crate::DispatchMode::NativePreferred,
            &checkpoint,
            &artifacts,
            &cost,
        )
        .await;

        assert!(matches!(&result, Ok(AgentRunResult::Finished(_))), "unexpected {result:?}");
        let resp = match result {
            Ok(AgentRunResult::Finished(r)) => r,
            _ => return,
        };
        assert_eq!(resp.tool_transcript.len(), 1);
        assert_eq!(resp.tool_transcript[0].name, "search");
    }

    #[tokio::test]
    async fn tc16_checkpoint_and_cost_ports_are_invoked() {
        let llm = SequentialLlm::from_contents(vec![r#"{"type": "final", "content": "ok"}"#]);
        let tools = MockToolPort::empty();
        let store = MemoryStore::new();
        let sink = CollectingSink::new();
        let checkpoint = CountingCheckpointPort::new();
        let artifacts = NoopArtifactStore;
        let cost = CountingCostMeter::new();
        let policy = AllowAllPolicyPort;
        let approval = AlwaysApprovePort;

        let result = run_turn_with_extensions(
            &llm,
            &tools,
            &store,
            &sink,
            make_request("hello"),
            &generous_policy(),
            "test-model",
            &policy,
            &approval,
            crate::DispatchMode::PromptGuided,
            &checkpoint,
            &artifacts,
            &cost,
        )
        .await;
        assert!(result.is_ok(), "turn should succeed");
        let checkpoints = checkpoint.saved.lock().unwrap_or_else(|p| p.into_inner()).len();
        let llm_calls = *cost.llm_calls.lock().unwrap_or_else(|p| p.into_inner());
        assert!(checkpoints >= 1, "checkpoint port should be invoked at least once");
        assert!(llm_calls >= 1, "cost meter should record llm usage");
    }
}
