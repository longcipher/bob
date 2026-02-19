//! # Composite Tool Port
//!
//! Composite tool port — aggregates multiple [`ToolPort`] implementations.
//!
//! ## Overview
//!
//! When multiple MCP servers (or other tool sources) are configured, a
//! `CompositeToolPort` collects tools from all inner ports and routes
//! `call_tool` requests to the port that owns the tool based on namespace.
//!
//! ## Routing Strategy
//!
//! Each inner port is identified by a `server_id` (e.g. `"filesystem"`).
//! Tool IDs are expected to be already namespaced (e.g. `"mcp/filesystem/read_file"`).
//! Routing uses the first inner port whose tool list contains the requested name.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_runtime::composite::CompositeToolPort;
//! use bob_core::ports::ToolPort;
//! use std::sync::Arc;
//!
//! let filesystem_port: Arc<dyn ToolPort> = /* ... */;
//! let shell_port: Arc<dyn ToolPort> = /* ... */;
//!
//! let composite = CompositeToolPort::new(vec![
//!     ("filesystem".to_string(), filesystem_port),
//!     ("shell".to_string(), shell_port),
//! ]);
//!
//! // List all tools from all sources
//! let all_tools = composite.list_tools().await?;
//!
//! // Call a tool - automatically routed to the correct port
//! let result = composite.call_tool(ToolCall {
//!     name: "mcp/filesystem/read_file".to_string(),
//!     arguments: json!({"path": "/tmp/test.txt"}),
//! }).await?;
//! ```

use std::sync::Arc;

use bob_core::{
    error::ToolError,
    ports::ToolPort,
    types::{ToolCall, ToolDescriptor, ToolResult},
};

/// A [`ToolPort`] that delegates to multiple inner ports.
///
/// Each inner port is identified by a `server_id` (e.g. `"filesystem"`).
/// Tool IDs are expected to be already namespaced (e.g. `"mcp/filesystem/read_file"`).
/// Routing uses the first inner port whose tool list contains the requested name.
pub struct CompositeToolPort {
    ports: Vec<(String, Arc<dyn ToolPort>)>,
}

impl std::fmt::Debug for CompositeToolPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.ports.iter().map(|(id, _)| id.as_str()).collect();
        f.debug_struct("CompositeToolPort").field("ports", &ids).finish()
    }
}

impl CompositeToolPort {
    /// Create a composite from a list of `(server_id, port)` pairs.
    #[must_use]
    pub fn new(ports: Vec<(String, Arc<dyn ToolPort>)>) -> Self {
        Self { ports }
    }
}

#[async_trait::async_trait]
impl ToolPort for CompositeToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        let mut all = Vec::new();
        for (_id, port) in &self.ports {
            let tools = port.list_tools().await?;
            all.extend(tools);
        }
        Ok(all)
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        // Route to the port whose namespace matches the tool name.
        for (id, port) in &self.ports {
            let prefix = format!("mcp/{id}/");
            if call.name.starts_with(&prefix) {
                return port.call_tool(call).await;
            }
        }
        Err(ToolError::Execution(format!("no tool port owns tool '{}'", call.name)))
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bob_core::types::{ToolResult, ToolSource};

    use super::*;

    /// Stub tool port that returns fixed tools.
    struct StubPort {
        tools: Vec<ToolDescriptor>,
    }

    #[async_trait::async_trait]
    impl ToolPort for StubPort {
        async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                name: call.name,
                output: serde_json::json!({"ok": true}),
                is_error: false,
            })
        }
    }

    #[tokio::test]
    async fn lists_tools_from_all_ports() {
        let p1 = Arc::new(StubPort {
            tools: vec![ToolDescriptor {
                id: "mcp/fs/read_file".into(),
                description: "Read a file".into(),
                input_schema: serde_json::json!({}),
                source: ToolSource::Mcp { server: "fs".into() },
            }],
        });
        let p2 = Arc::new(StubPort {
            tools: vec![ToolDescriptor {
                id: "mcp/git/log".into(),
                description: "Git log".into(),
                input_schema: serde_json::json!({}),
                source: ToolSource::Mcp { server: "git".into() },
            }],
        });

        let composite = CompositeToolPort::new(vec![
            ("fs".into(), p1 as Arc<dyn ToolPort>),
            ("git".into(), p2 as Arc<dyn ToolPort>),
        ]);

        let tools = composite.list_tools().await.ok();
        assert_eq!(tools.as_ref().map(Vec::len), Some(2));
    }

    #[tokio::test]
    async fn routes_call_to_correct_port() {
        let p1 = Arc::new(StubPort {
            tools: vec![ToolDescriptor {
                id: "mcp/fs/read_file".into(),
                description: "Read".into(),
                input_schema: serde_json::json!({}),
                source: ToolSource::Mcp { server: "fs".into() },
            }],
        });
        let p2 = Arc::new(StubPort {
            tools: vec![ToolDescriptor {
                id: "mcp/git/log".into(),
                description: "Log".into(),
                input_schema: serde_json::json!({}),
                source: ToolSource::Mcp { server: "git".into() },
            }],
        });

        let composite = CompositeToolPort::new(vec![
            ("fs".into(), p1 as Arc<dyn ToolPort>),
            ("git".into(), p2 as Arc<dyn ToolPort>),
        ]);

        let call = ToolCall { name: "mcp/git/log".into(), arguments: serde_json::json!({}) };
        let result = composite.call_tool(call).await;
        assert!(result.is_ok());
        assert_eq!(result.ok().map(|r| r.name), Some("mcp/git/log".into()));
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let composite = CompositeToolPort::new(vec![]);
        let call = ToolCall { name: "mcp/unknown/tool".into(), arguments: serde_json::json!({}) };
        let result = composite.call_tool(call).await;
        assert!(result.is_err());
    }
}
