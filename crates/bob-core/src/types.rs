//! Domain types for the Bob Agent Framework.

use std::{collections::HashMap, pin::Pin};

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
    /// Arbitrary metadata forwarded to adapters.
    pub metadata: HashMap<String, serde_json::Value>,
    /// Optional cancellation handle.
    pub cancel_token: Option<CancelToken>,
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
    pub name: String,
    pub arguments: serde_json::Value,
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

// ── Message History ──────────────────────────────────────────────────

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
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
}

// ── Observability Types ──────────────────────────────────────────────

/// An event emitted during agent execution for observability.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    TurnStarted { session_id: SessionId },
    LlmCallStarted { model: String },
    LlmCallCompleted { usage: TokenUsage },
    ToolCallStarted { name: String },
    ToolCallCompleted { name: String, is_error: bool },
    TurnCompleted { finish_reason: FinishReason },
    Error { error: String },
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
