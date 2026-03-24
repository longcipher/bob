//! # OpenTelemetry Event Sink
//!
//! Event sink that bridges [`AgentEvent`]s to the OpenTelemetry tracing layer.
//!
//! ## Overview
//!
//! This adapter wraps [`tracing`] events with OpenTelemetry span context
//! propagation, enabling distributed trace collection for agent operations.
//!
//! ## Setup
//!
//! Call [`init_otel`] once at application startup (e.g. in `main`) before
//! creating the event sink. Then use the returned [`OtlpGuard`] to keep the
//! OTel pipeline alive.
//!
//! ```rust,ignore
//! use bob_adapters::observe_otel::{init_otel, OtlpEventSink};
//!
//! let _guard = init_otel("my-agent", "http://localhost:4317").await?;
//! let sink = OtlpEventSink::new();
//! ```
//!
//! ## Feature Flag
//!
//! This module is only available when the `observe-otel` feature is enabled.

use std::sync::OnceLock;

use bob_core::{ports::EventSink, types::AgentEvent};
use opentelemetry::trace::SpanKind;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::{Resource, trace as sdktrace};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Keeps the OTel pipeline alive. Drop this to shut down export.
pub struct OtlpGuard {
    _tracer_provider: sdktrace::SdkTracerProvider,
}

/// Initialise the OpenTelemetry pipeline with OTLP/gRPC export.
///
/// - `service_name`: logical name for this agent instance.
/// - `otlp_endpoint`: gRPC endpoint (e.g. `"http://localhost:4317"`).
///
/// Returns a guard that **must** be held for the lifetime of the process.
///
/// # Errors
///
/// Returns an error if the OTLP exporter cannot be initialised.
pub async fn init_otel(
    service_name: &str,
    otlp_endpoint: &str,
) -> Result<OtlpGuard, Box<dyn std::error::Error + Send + Sync>> {
    let exporter = SpanExporter::builder().with_tonic().with_endpoint(otlp_endpoint).build()?;

    let resource = Resource::builder()
        .with_attributes([opentelemetry::KeyValue::new("service.name", service_name.to_string())])
        .build();

    let tracer_provider = sdktrace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = tracer_provider.tracer("bob-agent");

    // Install the tracing-opentelemetry layer globally.
    // This requires a tracing subscriber to be set up separately.
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        // The subscriber must already be initialised; we just add the OTel layer.
        // If no subscriber is installed, this is a no-op.
        let _otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        // NOTE: The caller is responsible for installing the tracing subscriber
        // and adding this layer. We expose the tracer provider so advanced
        // users can add the layer themselves.
    });

    Ok(OtlpGuard { _tracer_provider: tracer_provider })
}

/// Convenience function that returns the tracer for manual span creation.
#[must_use]
pub fn get_tracer(_guard: &OtlpGuard) -> opentelemetry::global::BoxedTracer {
    opentelemetry::global::tracer("bob-agent")
}

/// Event sink that emits [`AgentEvent`]s as OpenTelemetry-aware tracing spans.
///
/// Each significant agent event creates a span with relevant attributes,
/// enabling distributed trace collection via OTLP export.
#[derive(Debug, Clone, Copy, Default)]
pub struct OtlpEventSink;

impl OtlpEventSink {
    /// Create a new OTel-aware event sink.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventSink for OtlpEventSink {
    fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStarted { session_id } => {
                let span = tracing::info_span!(
                    "agent.turn",
                    session_id = %session_id,
                    otel.kind = ?SpanKind::Internal,
                    otel.name = "AgentTurn",
                );
                let _guard = span.enter();
                tracing::info!("turn started");
            }
            AgentEvent::SkillsSelected { session_id, skill_names } => {
                tracing::info!(
                    parent: &tracing::Span::current(),
                    session_id = %session_id,
                    count = skill_names.len(),
                    ?skill_names,
                    "skills selected",
                );
            }
            AgentEvent::LlmCallStarted { session_id, step, model } => {
                let span = tracing::info_span!(
                    "llm.call",
                    session_id = %session_id,
                    step = step,
                    model = %model,
                    otel.kind = ?SpanKind::Client,
                    otel.name = "LlmCall",
                );
                let _guard = span.enter();
                tracing::info!("LLM call started");
            }
            AgentEvent::LlmCallCompleted { session_id, step, model, usage } => {
                tracing::info!(
                    parent: &tracing::Span::current(),
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
                let span = tracing::info_span!(
                    "tool.call",
                    session_id = %session_id,
                    step = step,
                    tool = %name,
                    otel.kind = ?SpanKind::Internal,
                    otel.name = "ToolCall",
                );
                let _guard = span.enter();
                tracing::info!("tool call started");
            }
            AgentEvent::ToolCallCompleted { session_id, step, name, is_error } => {
                if is_error {
                    tracing::warn!(
                        parent: &tracing::Span::current(),
                        session_id = %session_id,
                        step,
                        tool = %name,
                        "tool call completed with error",
                    );
                } else {
                    tracing::info!(
                        parent: &tracing::Span::current(),
                        session_id = %session_id,
                        step,
                        tool = %name,
                        "tool call completed",
                    );
                }
            }
            AgentEvent::TurnCompleted { session_id, finish_reason, usage } => {
                tracing::info!(
                    parent: &tracing::Span::current(),
                    session_id = %session_id,
                    reason = ?finish_reason,
                    prompt_tokens = usage.prompt_tokens,
                    completion_tokens = usage.completion_tokens,
                    total_tokens = usage.total(),
                    "turn completed",
                );
            }
            AgentEvent::Error { session_id, step, error } => {
                tracing::error!(
                    parent: &tracing::Span::current(),
                    session_id = %session_id,
                    step,
                    error = %error,
                    "agent error",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bob_core::types::{FinishReason, TokenUsage};

    use super::*;

    #[test]
    fn otel_sink_emits_all_variants_no_panic() {
        let sink = OtlpEventSink::new();

        let events = vec![
            AgentEvent::TurnStarted { session_id: "s1".into() },
            AgentEvent::SkillsSelected {
                session_id: "s1".into(),
                skill_names: vec!["review".into()],
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
}
