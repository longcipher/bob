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

use std::sync::Arc;

use bob_core::{ports::EventSink, types::AgentEvent};

/// Event sink that forwards every event to multiple child sinks.
#[derive(Default)]
pub struct FanoutEventSink {
    sinks: Vec<Arc<dyn EventSink>>,
}

impl std::fmt::Debug for FanoutEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FanoutEventSink").field("sink_count", &self.sinks.len()).finish()
    }
}

impl FanoutEventSink {
    /// Create an empty fanout sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a child sink.
    #[must_use]
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sinks.push(sink);
        self
    }
}

impl EventSink for FanoutEventSink {
    fn emit(&self, event: AgentEvent) {
        for sink in &self.sinks {
            sink.emit(event.clone());
        }
    }
}

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
            AgentEvent::SkillsSelected { session_id, skill_names } => {
                tracing::info!(
                    session_id = %session_id,
                    count = skill_names.len(),
                    ?skill_names,
                    "skills selected",
                );
            }
            AgentEvent::LlmCallStarted { session_id, step, model } => {
                tracing::info!(session_id = %session_id, step, model = %model, "LLM call started");
            }
            AgentEvent::LlmCallCompleted { session_id, step, model, usage } => {
                tracing::info!(
                    session_id = %session_id,
                    step,
                    model = %model,
                    prompt_tokens = usage.prompt_tokens,
                    completion_tokens = usage.completion_tokens,
                    total_tokens = usage.total(),
                    "LLM call completed",
                );
            }
            AgentEvent::ToolCallStarted { session_id, step, name } => {
                tracing::info!(session_id = %session_id, step, tool = %name, "tool call started");
            }
            AgentEvent::ToolCallCompleted { session_id, step, name, is_error } => {
                if is_error {
                    tracing::warn!(
                        session_id = %session_id,
                        step,
                        tool = %name,
                        "tool call completed with error",
                    );
                } else {
                    tracing::info!(
                        session_id = %session_id,
                        step,
                        tool = %name,
                        "tool call completed",
                    );
                }
            }
            AgentEvent::TurnCompleted { session_id, finish_reason, usage } => {
                tracing::info!(
                    session_id = %session_id,
                    reason = ?finish_reason,
                    prompt_tokens = usage.prompt_tokens,
                    completion_tokens = usage.completion_tokens,
                    total_tokens = usage.total(),
                    "turn completed",
                );
            }
            AgentEvent::Error { session_id, step, error } => {
                tracing::error!(session_id = %session_id, step, error = %error, "agent error");
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bob_core::types::{FinishReason, TokenUsage};

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingSink {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl EventSink for RecordingSink {
        fn emit(&self, event: AgentEvent) {
            self.events.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(event);
        }
    }

    /// Verify every `AgentEvent` variant is handled without panic.
    #[test]
    fn emit_all_variants_no_panic() {
        let sink = TracingEventSink::new();

        let events = vec![
            AgentEvent::TurnStarted { session_id: "s1".into() },
            AgentEvent::SkillsSelected {
                session_id: "s1".into(),
                skill_names: vec!["rust-review".into(), "security-audit".into()],
            },
            AgentEvent::LlmCallStarted { session_id: "s1".into(), step: 1, model: "gpt-4o".into() },
            AgentEvent::LlmCallCompleted {
                session_id: "s1".into(),
                step: 1,
                model: "gpt-4o".into(),
                usage: TokenUsage { prompt_tokens: 10, completion_tokens: 20 },
            },
            AgentEvent::ToolCallStarted {
                session_id: "s1".into(),
                step: 1,
                name: "read_file".into(),
            },
            AgentEvent::ToolCallCompleted {
                session_id: "s1".into(),
                step: 1,
                name: "read_file".into(),
                is_error: false,
            },
            AgentEvent::ToolCallCompleted {
                session_id: "s1".into(),
                step: 1,
                name: "bad_tool".into(),
                is_error: true,
            },
            AgentEvent::TurnCompleted {
                session_id: "s1".into(),
                finish_reason: FinishReason::Stop,
                usage: TokenUsage { prompt_tokens: 10, completion_tokens: 20 },
            },
            AgentEvent::Error {
                session_id: "s1".into(),
                step: Some(1),
                error: "something went wrong".into(),
            },
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

    #[test]
    fn fanout_sink_emits_to_all_children() {
        let left = Arc::new(RecordingSink::default());
        let right = Arc::new(RecordingSink::default());
        let fanout = FanoutEventSink::new()
            .with_sink(left.clone() as Arc<dyn EventSink>)
            .with_sink(right.clone() as Arc<dyn EventSink>);

        fanout.emit(AgentEvent::TurnCompleted {
            session_id: "session-1".into(),
            finish_reason: FinishReason::Stop,
            usage: TokenUsage { prompt_tokens: 3, completion_tokens: 4 },
        });

        let left_events = left.events.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let right_events = right.events.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        assert_eq!(left_events.len(), 1);
        assert_eq!(right_events.len(), 1);
        assert!(matches!(
            &left_events[0],
            AgentEvent::TurnCompleted { session_id, usage, .. }
                if session_id == "session-1" && usage.total() == 7
        ));
        assert!(matches!(
            &right_events[0],
            AgentEvent::TurnCompleted { session_id, usage, .. }
                if session_id == "session-1" && usage.total() == 7
        ));
    }
}
