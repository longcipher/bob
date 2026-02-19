//! # MCP Tool Adapter
//!
//! MCP tool adapter — implements [`ToolPort`] via the `rmcp` crate.
//!
//! ## Overview
//!
//! This adapter connects to [Model Context Protocol (MCP)](https://modelcontextprotocol.io/)
//! servers and exposes their tools through the [`ToolPort`] interface.
//!
//! MCP is a protocol for connecting AI assistants to external tools and data sources.
//! This adapter uses stdio transport to communicate with MCP servers running as child processes.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_adapters::mcp_rmcp::McpToolAdapter;
//!
//! // Connect to the official filesystem MCP server
//! let adapter = McpToolAdapter::connect_stdio(
//!     "filesystem",
//!     "npx",
//!     &[
//!         "-y".to_string(),
//!         "@modelcontextprotocol/server-filesystem".to_string(),
//!         "/tmp".to_string(),
//!     ],
//!     &[], // environment variables
//! ).await?;
//!
//! // List available tools
//! let tools = adapter.list_tools().await?;
//!
//! // Call a tool
//! let result = adapter.call_tool(ToolCall {
//!     name: "mcp/filesystem/read_file".to_string(),
//!     arguments: json!({"path": "/tmp/test.txt"}),
//! }).await?;
//! ```
//!
//! ## Feature Flag
//!
//! This module is only available when the `mcp-rmcp` feature is enabled (default).

use std::borrow::Cow;

use bob_core::{
    error::ToolError,
    ports::ToolPort,
    types::{ToolCall, ToolDescriptor, ToolResult, ToolSource},
};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::process::Command;

/// Type alias for the concrete running MCP service returned by `rmcp`.
///
/// `()` is the client handler — `rmcp` uses the unit type as the default
/// [`Service<RoleClient>`] when the caller needs no server-initiated
/// request handling beyond what the SDK provides.
type McpService = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// Adapter that connects to an MCP server (stdio transport) and exposes
/// its tools through the [`ToolPort`] interface.
pub struct McpToolAdapter {
    /// Human-readable server identifier (e.g. `"filesystem"`).
    server_id: String,
    /// Running service handle — used for `list_all_tools` and `call_tool`.
    service: McpService,
}

impl std::fmt::Debug for McpToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpToolAdapter").field("server_id", &self.server_id).finish_non_exhaustive()
    }
}

impl McpToolAdapter {
    /// Spawn an MCP server as a child process via stdio transport and connect.
    ///
    /// # Arguments
    /// * `server_id` — logical name (used in tool namespacing)
    /// * `command` — executable to spawn (e.g. `"npx"`)
    /// * `args` — arguments for the command (e.g. `["-y",
    ///   "@modelcontextprotocol/server-filesystem", "."]`)
    /// * `env` — optional environment variables to set on the child process
    pub async fn connect_stdio(
        server_id: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<Self, ToolError> {
        let args_owned: Vec<String> = args.to_vec();
        let env_owned: Vec<(String, String)> = env.to_vec();

        let transport = TokioChildProcess::new(Command::new(command).configure(move |cmd| {
            for arg in &args_owned {
                cmd.arg(arg);
            }
            for (k, v) in &env_owned {
                cmd.env(k, v);
            }
        }))
        .map_err(|e| ToolError::Execution(format!("failed to spawn MCP server: {e}")))?;

        let service = ()
            .serve(transport)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to connect to MCP server: {e}")))?;

        Ok(Self { server_id: server_id.to_string(), service })
    }

    /// Gracefully shut down the MCP connection.
    pub async fn shutdown(self) -> Result<(), ToolError> {
        self.service
            .cancel()
            .await
            .map_err(|e| ToolError::Execution(format!("shutdown error: {e}")))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ToolPort for McpToolAdapter {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|e| ToolError::Execution(format!("list_tools failed: {e}")))?;

        let descriptors = tools
            .into_iter()
            .map(|t| {
                let schema_value = serde_json::to_value(t.input_schema.as_ref())
                    .unwrap_or_else(|_| serde_json::json!({"type": "object"}));

                ToolDescriptor {
                    id: format!("mcp/{}/{}", self.server_id, t.name),
                    description: t.description.as_deref().unwrap_or_default().to_string(),
                    input_schema: schema_value,
                    source: ToolSource::Mcp { server: self.server_id.clone() },
                }
            })
            .collect();

        Ok(descriptors)
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        // Strip namespace prefix to get the raw tool name.
        let prefix = format!("mcp/{}/", self.server_id);
        let raw_name = call.name.strip_prefix(&prefix).unwrap_or(&call.name);

        let arguments: Option<serde_json::Map<String, serde_json::Value>> =
            call.arguments.as_object().cloned();

        let result = self
            .service
            .call_tool(CallToolRequestParams {
                meta: None,
                name: Cow::Owned(raw_name.to_string()),
                arguments,
                task: None,
            })
            .await
            .map_err(|e| ToolError::Execution(format!("call_tool failed: {e}")))?;

        // Extract text content from the result.
        let output_text: String = result
            .content
            .iter()
            .filter_map(|c| {
                let raw: &rmcp::model::RawContent = c;
                raw.as_text().map(|t| t.text.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n");

        let output = if output_text.is_empty() {
            serde_json::json!(null)
        } else {
            // Try to parse as JSON first; fall back to plain string.
            serde_json::from_str(&output_text).unwrap_or(serde_json::Value::String(output_text))
        };

        Ok(ToolResult { name: call.name, output, is_error: result.is_error.unwrap_or(false) })
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    #[test]
    fn namespace_prefix_strip() {
        let prefix = format!("mcp/{}/", "fs");
        let raw = "mcp/fs/read_file".strip_prefix(&prefix);
        assert_eq!(raw, Some("read_file"));
    }

    #[test]
    fn namespace_prefix_passthrough() {
        let prefix = format!("mcp/{}/", "fs");
        // A name without the prefix is returned as-is.
        let raw = "read_file".strip_prefix(&prefix).unwrap_or("read_file");
        assert_eq!(raw, "read_file");
    }
}
