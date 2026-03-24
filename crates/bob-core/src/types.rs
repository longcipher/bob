//! # Domain Types
//!
//! Domain types for the Bob Agent Framework.
//!
//! This module defines all the core data structures used throughout the framework:
//!
//! - **Request/Response**: `AgentRequest`, `AgentResponse`, `AgentRunResult`
//! - **Messages**: `Message`, `Role`, conversation history
//! - **Tools**: `ToolDescriptor`, `ToolCall`, `ToolResult`, `ToolSource`
//! - **LLM**: `LlmRequest`, `LlmResponse`, `LlmStream`, `LlmStreamChunk`
//! - **Session**: `SessionState`, `SessionId`
//! - **Events**: `AgentEvent`, `AgentEventStream`, `AgentStreamEvent`
//! - **Usage**: `TokenUsage`, `FinishReason`
//! - **Policy**: `TurnPolicy`, `GuardReason`
//! - **Health**: `RuntimeHealth`, `HealthStatus`
//!
//! ## Serialization
//!
//! All types implement `Serialize` and `Deserialize` where appropriate,
//! making them suitable for persistence and API boundaries.
//!
//! ## Thread Safety
//!
//! All types are `Send + Sync` and can be safely shared across threads.

use std::pin::Pin;

use serde::{Deserialize, Serialize};

// ── Identifiers ──────────────────────────────────────────────────────

/// Opaque session identifier.
pub type SessionId = String;

/// Cancellation token (re-export for convenience).
pub type CancelToken = tokio_util::sync::CancellationToken;

// ── Request / Response ───────────────────────────────────────────────

/// Input to the agent runtime.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    /// User message content.
    pub input: String,
    /// Session to continue (or create).
    pub session_id: SessionId,
    /// Model override (e.g. `"openai:gpt-4o-mini"`). `None` uses config default.
    pub model: Option<String>,
    /// Structured per-turn context (skills, prompt extensions, tool policy).
    pub context: RequestContext,
    /// Optional cancellation handle.
    pub cancel_token: Option<CancelToken>,
}

/// Structured per-turn request context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestContext {
    /// Extra instructions appended to the runtime system prompt.
    pub system_prompt: Option<String>,
    /// Skill names selected for this request.
    pub selected_skills: Vec<String>,
    /// Tool call policy resolved for this request.
    pub tool_policy: RequestToolPolicy,
}

/// Tool policy attached to a request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestToolPolicy {
    /// Tool names denied for this request.
    pub deny_tools: Vec<String>,
    /// Optional request allowlist. `None` means "allow all not denied".
    pub allow_tools: Option<Vec<String>>,
}

/// Final agent output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Final text produced by the agent.
    pub content: String,
    /// Tool calls executed during this turn.
    pub tool_transcript: Vec<ToolResult>,
    /// Token usage across the turn.
    pub usage: TokenUsage,
    /// Why the turn ended.
    pub finish_reason: FinishReason,
}

/// Outcome of a single agent turn.
#[derive(Debug)]
pub enum AgentRunResult {
    Finished(AgentResponse),
}

// ── Action Protocol ──────────────────────────────────────────────────

/// Provider-neutral action the LLM can emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Final { content: String },
    ToolCall { name: String, arguments: serde_json::Value },
    AskUser { question: String },
}

// ── Tool Types ───────────────────────────────────────────────────────

/// Description of an available tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// Unique tool identifier (e.g. `"mcp/filesystem/read_file"`).
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
    /// Where this tool lives.
    pub source: ToolSource,
}

/// Origin of a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolSource {
    Local,
    Mcp { server: String },
}

/// A request to call a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolCall {
    #[must_use]
    pub fn new(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self { call_id: None, name: name.into(), arguments }
    }

    #[must_use]
    pub fn with_call_id(mut self, call_id: impl Into<String>) -> Self {
        self.call_id = Some(call_id.into());
        self
    }
}

/// Result of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub name: String,
    pub output: serde_json::Value,
    pub is_error: bool,
}

// ── LLM Types ────────────────────────────────────────────────────────

/// An LLM inference request (internal representation).
#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// Model identifier (e.g. `"openai:gpt-4o-mini"`).
    pub model: String,
    /// Message history to send.
    pub messages: Vec<Message>,
    /// Available tools (may be empty).
    pub tools: Vec<ToolDescriptor>,
}

/// An LLM inference response (internal representation).
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// Raw text content from the model.
    pub content: String,
    /// Token usage for this call.
    pub usage: TokenUsage,
    /// Why the model stopped.
    pub finish_reason: FinishReason,
    /// Native tool calls emitted by providers that support structured calls.
    pub tool_calls: Vec<ToolCall>,
}

/// A streaming LLM response.
pub type LlmStream =
    Pin<Box<dyn futures_core::Stream<Item = Result<LlmStreamChunk, crate::LlmError>> + Send>>;

/// Single chunk in a streaming response.
#[derive(Debug, Clone)]
pub enum LlmStreamChunk {
    TextDelta(String),
    Done { usage: TokenUsage },
}

/// Declares what an LLM adapter supports at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmCapabilities {
    /// Whether this provider supports native tool calling payloads.
    pub native_tool_calling: bool,
    /// Whether this provider supports streaming responses.
    pub streaming: bool,
}

impl Default for LlmCapabilities {
    fn default() -> Self {
        Self { native_tool_calling: false, streaming: true }
    }
}

// ── Message History ──────────────────────────────────────────────────

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl Message {
    #[must_use]
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
        }
    }

    #[must_use]
    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_call_id: None,
            tool_name: None,
        }
    }

    #[must_use]
    pub fn tool_result(
        tool_name: impl Into<String>,
        call_id: Option<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: call_id,
            tool_name: Some(tool_name.into()),
        }
    }
}

/// Conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ── Session State ────────────────────────────────────────────────────

/// Persisted session state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Full message history.
    pub messages: Vec<Message>,
    /// Cumulative token usage across turns.
    pub total_usage: TokenUsage,
    /// Monotonically increasing version for CAS operations.
    ///
    /// Starts at `0` for a fresh session and is incremented on every
    /// successful [`SessionStore::save`] or [`SessionStore::save_if_version`].
    #[serde(default)]
    pub version: u64,
}

// ── Observability Types ──────────────────────────────────────────────

/// An event emitted during agent execution for observability.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    TurnStarted { session_id: SessionId },
    SkillsSelected { session_id: SessionId, skill_names: Vec<String> },
    LlmCallStarted { session_id: SessionId, step: u32, model: String },
    LlmCallCompleted { session_id: SessionId, step: u32, model: String, usage: TokenUsage },
    ToolCallStarted { session_id: SessionId, step: u32, name: String },
    ToolCallCompleted { session_id: SessionId, step: u32, name: String, is_error: bool },
    TurnCompleted { session_id: SessionId, finish_reason: FinishReason, usage: TokenUsage },
    Error { session_id: SessionId, step: Option<u32>, error: String },
}

/// Streaming event for `run_stream`.
pub type AgentEventStream = Pin<Box<dyn futures_core::Stream<Item = AgentStreamEvent> + Send>>;

/// Events yielded by the streaming API.
#[derive(Debug, Clone)]
pub enum AgentStreamEvent {
    TextDelta { content: String },
    ToolCallStarted { name: String, call_id: String },
    ToolCallCompleted { call_id: String, result: ToolResult },
    Finished { usage: TokenUsage },
    Error { error: String },
}

// ── Usage / Finish ───────────────────────────────────────────────────

/// Token usage counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    #[must_use]
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// Why a turn or LLM call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCall,
    Length,
    GuardExceeded,
    Cancelled,
}

// ── Guard / Policy ───────────────────────────────────────────────────

/// Why the loop guard terminated the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardReason {
    MaxSteps,
    MaxToolCalls,
    MaxConsecutiveErrors,
    TurnTimeout,
    Cancelled,
}

/// Additional context passed to approval checks for one tool call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalContext {
    /// Session identifier for correlation and policy rules.
    pub session_id: SessionId,
    /// 1-based LLM step index in the current turn.
    pub turn_step: u32,
    /// Selected skills for this request.
    pub selected_skills: Vec<String>,
}

/// Outcome of a tool approval decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// Snapshot of turn progress for suspend/resume and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCheckpoint {
    pub session_id: SessionId,
    pub step: u32,
    pub tool_calls: u32,
    pub usage: TokenUsage,
}

/// Stored artifact created during a turn (e.g. tool output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub session_id: SessionId,
    pub kind: String,
    pub name: String,
    pub content: serde_json::Value,
}

/// Turn execution policy with guard limits.
#[derive(Debug, Clone)]
pub struct TurnPolicy {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_consecutive_errors: u32,
    pub turn_timeout_ms: u64,
    pub tool_timeout_ms: u64,
}

impl Default for TurnPolicy {
    fn default() -> Self {
        Self {
            max_steps: 12,
            max_tool_calls: 8,
            max_consecutive_errors: 2,
            turn_timeout_ms: 90_000,
            tool_timeout_ms: 15_000,
        }
    }
}

// ── Health ────────────────────────────────────────────────────────────

/// Runtime health status.
#[derive(Debug, Clone)]
pub struct RuntimeHealth {
    pub status: HealthStatus,
    pub llm_ready: bool,
    pub mcp_pool_ready: bool,
}

impl Default for RuntimeHealth {
    fn default() -> Self {
        Self { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

/// Overall health state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

// ── Turn Context ─────────────────────────────────────────────────────

/// Immutable context for a single turn.
#[derive(Debug, Clone)]
pub struct TurnContext {
    pub session_id: SessionId,
    pub trace_id: String,
    pub policy: TurnPolicy,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn session_state_deserializes_legacy_messages_without_tool_metadata() {
        let raw = json!({
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "hi"},
                {"role": "tool", "content": "{\"ok\":true}"}
            ],
            "total_usage": {
                "prompt_tokens": 3,
                "completion_tokens": 5
            }
        });

        let state: SessionState =
            serde_json::from_value(raw).expect("legacy session payload should deserialize");

        assert_eq!(state.messages.len(), 3);
        assert!(state.messages[0].tool_calls.is_empty());
        assert_eq!(state.messages[1].tool_call_id, None);
        assert_eq!(state.messages[2].tool_name, None);
        assert_eq!(state.total_usage.total(), 8);
    }

    #[test]
    fn session_state_roundtrips_structured_tool_metadata() {
        let state = SessionState {
            messages: vec![
                Message {
                    role: Role::Assistant,
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        call_id: Some("call-1".into()),
                        name: "search".into(),
                        arguments: json!({"q": "rust"}),
                    }],
                    tool_call_id: None,
                    tool_name: None,
                },
                Message {
                    role: Role::Tool,
                    content: "{\"hits\":2}".into(),
                    tool_calls: Vec::new(),
                    tool_call_id: Some("call-1".into()),
                    tool_name: Some("search".into()),
                },
            ],
            total_usage: TokenUsage { prompt_tokens: 11, completion_tokens: 7 },
            ..Default::default()
        };

        let encoded = serde_json::to_value(&state).expect("session state should serialize");
        let decoded: SessionState =
            serde_json::from_value(encoded).expect("structured session state should deserialize");

        assert_eq!(decoded.messages.len(), 2);
        assert_eq!(decoded.messages[0].tool_calls.len(), 1);
        assert_eq!(decoded.messages[0].tool_calls[0].call_id.as_deref(), Some("call-1"));
        assert_eq!(decoded.messages[1].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(decoded.messages[1].tool_name.as_deref(), Some("search"));
    }
}
