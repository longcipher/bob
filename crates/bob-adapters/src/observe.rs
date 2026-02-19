//! # Tracing Event Sink
//!
//! Tracing event sink — implements [`EventSink`] via the `tracing` crate.
//!
//! ## Overview
//!
//! This adapter provides structured logging of agent events using the
//! [`tracing`](https://docs.rs/tracing/latest/tracing/) ecosystem.
//!
//! Events are emitted at appropriate log levels:
//! - `INFO`: Normal operations (turn started, tool calls, LLM calls)
//! - `WARN`: Tool errors
//! - `ERROR`: Agent errors
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::observe::TracingEventSink;
//! use bob_core::{
//!     ports::EventSink,
//!     types::AgentEvent,
//! };
//!
//! // Initialize tracing subscriber (typically done in main)
//! tracing_subscriber::fmt::init();
//!
//! let sink = TracingEventSink::new();
//!
//! // Emit events
//! sink.emit(AgentEvent::TurnStarted {
//!     session_id: "session-1".to_string(),
//! });
//! ```
//!
//! ## Output Format
//!
//! Events are formatted as structured logs:
//!
//! ```text
//! 2024-01-15T10:30:00Z INFO turn started session_id="session-1"
//! 2024-01-15T10:30:01Z INFO LLM call started model="openai:gpt-4o-mini"
//! 2024-01-15T10:30:05Z INFO LLM call completed prompt_tokens=100 completion_tokens=50 total_tokens=150
//! ```
//!
//! ## Feature Flag
//!
//! This module is only available when the `observe-tracing` feature is enabled (default).

use bob_core::{ports::EventSink, types::AgentEvent};

/// Event sink that emits [`AgentEvent`]s as structured `tracing` events.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingEventSink;

impl TracingEventSink {
    /// Create a new tracing event sink.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventSink for TracingEventSink {
    fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStarted { session_id } => {
                tracing::info!(session_id = %session_id, "turn started");
            }
            AgentEvent::SkillsSelected { skill_names } => {
                tracing::info!(count = skill_names.len(), ?skill_names, "skills selected");
            }
            AgentEvent::LlmCallStarted { model } => {
                tracing::info!(model = %model, "LLM call started");
            }
            AgentEvent::LlmCallCompleted { usage } => {
                tracing::info!(
                    prompt_tokens = usage.prompt_tokens,
                    completion_tokens = usage.completion_tokens,
                    total_tokens = usage.total(),
                    "LLM call completed",
                );
            }
            AgentEvent::ToolCallStarted { name } => {
                tracing::info!(tool = %name, "tool call started");
            }
            AgentEvent::ToolCallCompleted { name, is_error } => {
                if is_error {
                    tracing::warn!(tool = %name, "tool call completed with error");
                } else {
                    tracing::info!(tool = %name, "tool call completed");
                }
            }
            AgentEvent::TurnCompleted { finish_reason } => {
                tracing::info!(reason = ?finish_reason, "turn completed");
            }
            AgentEvent::Error { error } => {
                tracing::error!(error = %error, "agent error");
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::types::{FinishReason, TokenUsage};

    use super::*;

    /// Verify every `AgentEvent` variant is handled without panic.
    #[test]
    fn emit_all_variants_no_panic() {
        let sink = TracingEventSink::new();

        let events = vec![
            AgentEvent::TurnStarted { session_id: "s1".into() },
            AgentEvent::SkillsSelected {
                skill_names: vec!["rust-review".into(), "security-audit".into()],
            },
            AgentEvent::LlmCallStarted { model: "gpt-4o".into() },
            AgentEvent::LlmCallCompleted {
                usage: TokenUsage { prompt_tokens: 10, completion_tokens: 20 },
            },
            AgentEvent::ToolCallStarted { name: "read_file".into() },
            AgentEvent::ToolCallCompleted { name: "read_file".into(), is_error: false },
            AgentEvent::ToolCallCompleted { name: "bad_tool".into(), is_error: true },
            AgentEvent::TurnCompleted { finish_reason: FinishReason::Stop },
            AgentEvent::Error { error: "something went wrong".into() },
        ];

        for event in events {
            sink.emit(event);
        }
    }

    #[test]
    fn is_object_safe_as_event_sink() {
        let sink: Arc<dyn EventSink> = Arc::new(TracingEventSink::new());
        sink.emit(AgentEvent::TurnStarted { session_id: "test".into() });
    }
}
