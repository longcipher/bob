//! Tooling helpers for runtime composition.

use std::sync::Arc;

use bob_core::{
    error::ToolError,
    ports::ToolPort,
    types::{ToolCall, ToolDescriptor, ToolResult},
};

/// Decorator-style wrapper for [`ToolPort`] implementations.
pub trait ToolLayer: Send + Sync {
    /// Wraps an existing [`ToolPort`] with additional behavior.
    fn wrap(&self, inner: Arc<dyn ToolPort>) -> Arc<dyn ToolPort>;
}

/// A no-op tool port that advertises no tools and rejects all calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpToolPort;

#[async_trait::async_trait]
impl ToolPort for NoOpToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        Ok(vec![])
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        Err(ToolError::Execution(format!("no tool port configured, cannot call '{}'", call.name)))
    }
}

/// A [`ToolLayer`] that applies a timeout to tool calls.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutToolLayer {
    timeout_ms: u64,
}

impl TimeoutToolLayer {
    #[must_use]
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }
}

impl ToolLayer for TimeoutToolLayer {
    fn wrap(&self, inner: Arc<dyn ToolPort>) -> Arc<dyn ToolPort> {
        Arc::new(TimeoutToolPort { inner, timeout_ms: self.timeout_ms })
    }
}

struct TimeoutToolPort {
    inner: Arc<dyn ToolPort>,
    timeout_ms: u64,
}

impl std::fmt::Debug for TimeoutToolPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeoutToolPort")
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl ToolPort for TimeoutToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        self.inner.list_tools().await
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let tool_name = call.name.clone();
        match tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            self.inner.call_tool(call),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(ToolError::Timeout { name: tool_name }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::types::ToolSource;

    use super::*;

    struct SleepyToolPort;

    #[async_trait::async_trait]
    impl ToolPort for SleepyToolPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(vec![ToolDescriptor {
                id: "local/sleep".to_string(),
                description: "sleep".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
                source: ToolSource::Local,
            }])
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(ToolResult {
                name: call.name,
                output: serde_json::json!({"ok": true}),
                is_error: false,
            })
        }
    }

    #[tokio::test]
    async fn timeout_layer_times_out_slow_calls() {
        let layer = TimeoutToolLayer::new(5);
        let wrapped = layer.wrap(Arc::new(SleepyToolPort));
        let result = wrapped
            .call_tool(ToolCall {
                name: "local/sleep".to_string(),
                arguments: serde_json::json!({}),
            })
            .await;
        assert!(matches!(result, Err(ToolError::Timeout { .. })));
    }
}
