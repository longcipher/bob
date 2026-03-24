//! # Instrumenter Trait
//!
//! Fine-grained observation hooks for agent execution.
//!
//! Inspired by rustic-ai's `Instrumenter` trait, this provides granular hooks
//! for every stage of agent execution. Unlike [`EventSink`] which emits discrete
//! events, the [`Instrumenter`] trait provides structured callbacks with
//! dedicated info structs for each lifecycle event.
//!
//! All methods have default no-op implementations, so implementors only need
//! to override the hooks they care about.

use crate::types::{FinishReason, TokenUsage, ToolDescriptor};

// ── Info Structs ──────────────────────────────────────────────────────

/// Info passed to [`Instrumenter::on_run_start`].
#[derive(Debug, Clone)]
pub struct RunStartInfo<'a> {
    pub session_id: &'a str,
    pub model: &'a str,
}

/// Info passed to [`Instrumenter::on_run_end`].
#[derive(Debug, Clone)]
pub struct RunEndInfo<'a> {
    pub session_id: &'a str,
    pub finish_reason: FinishReason,
    pub usage: &'a TokenUsage,
}

/// Info passed to [`Instrumenter::on_run_error`].
#[derive(Debug, Clone)]
pub struct RunErrorInfo<'a> {
    pub session_id: &'a str,
    pub error: &'a str,
}

/// Info passed to [`Instrumenter::on_model_request`].
#[derive(Debug, Clone)]
pub struct ModelRequestInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub model: &'a str,
}

/// Info passed to [`Instrumenter::on_model_response`].
#[derive(Debug, Clone)]
pub struct ModelResponseInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub model: &'a str,
    pub usage: &'a TokenUsage,
}

/// Info passed to [`Instrumenter::on_model_error`].
#[derive(Debug, Clone)]
pub struct ModelErrorInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub model: &'a str,
    pub error: &'a str,
}

/// Info passed to [`Instrumenter::on_tool_call`].
#[derive(Debug, Clone)]
pub struct ToolCallInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub tool: &'a str,
}

/// Info passed to [`Instrumenter::on_tool_end`].
#[derive(Debug, Clone)]
pub struct ToolEndInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub tool: &'a str,
    pub is_error: bool,
}

/// Info passed to [`Instrumenter::on_tool_error`].
#[derive(Debug, Clone)]
pub struct ToolErrorInfo<'a> {
    pub session_id: &'a str,
    pub step: u32,
    pub tool: &'a str,
    pub error: &'a str,
}

/// Info passed to [`Instrumenter::on_tool_discovered`].
#[derive(Debug, Clone)]
pub struct ToolDiscoveredInfo<'a> {
    pub tools: &'a [ToolDescriptor],
}

/// Info passed to [`Instrumenter::on_output_validation_error`].
#[derive(Debug, Clone)]
pub struct OutputValidationErrorInfo<'a> {
    pub session_id: &'a str,
    pub error: &'a str,
}

// ── Instrumenter Trait ────────────────────────────────────────────────

/// Fine-grained observation hooks for agent execution.
///
/// All methods have default no-op implementations.
pub trait Instrumenter: Send + Sync {
    /// Called when an agent run starts.
    fn on_run_start(&self, _info: &RunStartInfo<'_>) {}
    /// Called when an agent run completes successfully.
    fn on_run_end(&self, _info: &RunEndInfo<'_>) {}
    /// Called when an agent run fails.
    fn on_run_error(&self, _info: &RunErrorInfo<'_>) {}
    /// Called before an LLM request.
    fn on_model_request(&self, _info: &ModelRequestInfo<'_>) {}
    /// Called after an LLM response.
    fn on_model_response(&self, _info: &ModelResponseInfo<'_>) {}
    /// Called when an LLM request fails.
    fn on_model_error(&self, _info: &ModelErrorInfo<'_>) {}
    /// Called when a tool call starts.
    fn on_tool_call(&self, _info: &ToolCallInfo<'_>) {}
    /// Called when a tool call completes.
    fn on_tool_end(&self, _info: &ToolEndInfo<'_>) {}
    /// Called when a tool call fails.
    fn on_tool_error(&self, _info: &ToolErrorInfo<'_>) {}
    /// Called when tools are discovered.
    fn on_tool_discovered(&self, _info: &ToolDiscoveredInfo<'_>) {}
    /// Called when output validation fails.
    fn on_output_validation_error(&self, _info: &OutputValidationErrorInfo<'_>) {}
}

// ── Implementations ───────────────────────────────────────────────────

/// A no-op instrumenter that does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopInstrumenter;

impl Instrumenter for NoopInstrumenter {}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Tracks which hooks were called.
    #[derive(Debug, Default)]
    struct RecordingInstrumenter {
        calls: Mutex<Vec<String>>,
    }

    impl Instrumenter for RecordingInstrumenter {
        fn on_run_start(&self, _info: &RunStartInfo<'_>) {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).push("run_start".into());
        }
        fn on_run_end(&self, _info: &RunEndInfo<'_>) {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).push("run_end".into());
        }
        fn on_model_request(&self, _info: &ModelRequestInfo<'_>) {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).push("model_request".into());
        }
        fn on_tool_call(&self, _info: &ToolCallInfo<'_>) {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).push("tool_call".into());
        }
    }

    #[test]
    fn noop_instrumenter_is_default() {
        let inst = NoopInstrumenter;
        inst.on_run_start(&RunStartInfo { session_id: "s1", model: "test" });
        // Just verify no panic.
    }

    #[test]
    fn recording_instrumenter_tracks_calls() {
        let inst = RecordingInstrumenter::default();
        inst.on_run_start(&RunStartInfo { session_id: "s1", model: "test" });
        inst.on_model_request(&ModelRequestInfo { session_id: "s1", step: 1, model: "test" });
        inst.on_run_end(&RunEndInfo {
            session_id: "s1",
            finish_reason: FinishReason::Stop,
            usage: &TokenUsage { prompt_tokens: 10, completion_tokens: 20 },
        });

        let calls = inst.calls.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(&*calls, &["run_start", "model_request", "run_end"]);
    }

    #[test]
    fn instrumenter_is_object_safe() {
        let _inst: Arc<dyn Instrumenter> = Arc::new(NoopInstrumenter);
    }
}
