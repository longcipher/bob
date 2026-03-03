//! # Built-in Tool Port
//!
//! Workspace-sandboxed file and shell tools that run locally without
//! requiring an external MCP server.
//!
//! ## Tools
//!
//! | Tool ID            | Description                      |
//! |-------------------|----------------------------------|
//! | `local/file_read`  | Read a file relative to workspace |
//! | `local/file_write` | Write a file relative to workspace|
//! | `local/file_list`  | List directory contents           |
//! | `local/shell_exec` | Execute a shell command           |
//!
//! ## Security
//!
//! All file operations are sandboxed to the workspace directory.
//! Absolute paths and `..` traversal are rejected.

use std::path::{Component, Path, PathBuf};

use bob_core::{
    error::ToolError,
    types::{ToolCall, ToolDescriptor, ToolResult, ToolSource},
};
use serde_json::json;

/// Built-in tool port providing workspace-sandboxed file and shell operations.
#[derive(Debug, Clone)]
pub struct BuiltinToolPort {
    workspace: PathBuf,
}

impl BuiltinToolPort {
    /// Create a new built-in tool port rooted at the given workspace directory.
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Resolve a relative path safely within the workspace sandbox.
    ///
    /// Rejects absolute paths and parent-directory (`..`) traversal.
    fn resolve_safe_path(&self, relative: &str) -> Result<PathBuf, ToolError> {
        let path = Path::new(relative);

        if path.is_absolute() {
            return Err(ToolError::Execution("absolute paths not allowed in sandbox".to_string()));
        }

        for component in path.components() {
            if matches!(component, Component::ParentDir) {
                return Err(ToolError::Execution(
                    "parent directory traversal (..) not allowed".to_string(),
                ));
            }
        }

        Ok(self.workspace.join(relative))
    }

    /// Execute the `local/file_read` tool.
    async fn file_read(&self, args: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path_str = args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::Execution("missing 'path' argument".to_string()))?;

        let full_path = self.resolve_safe_path(path_str)?;
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::Execution(format!("read failed: {e}")))?;

        Ok(json!({ "content": content, "path": path_str }))
    }

    /// Execute the `local/file_write` tool.
    async fn file_write(&self, args: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path_str = args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::Execution("missing 'path' argument".to_string()))?;

        let content = args
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::Execution("missing 'content' argument".to_string()))?;

        let full_path = self.resolve_safe_path(path_str)?;

        // Create parent directories if needed.
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("mkdir failed: {e}")))?;
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| ToolError::Execution(format!("write failed: {e}")))?;

        Ok(json!({ "written": true, "path": path_str, "bytes": content.len() }))
    }

    /// Execute the `local/file_list` tool.
    async fn file_list(&self, args: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path_str = args.get("path").and_then(serde_json::Value::as_str).unwrap_or(".");

        let full_path = self.resolve_safe_path(path_str)?;
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| ToolError::Execution(format!("read_dir failed: {e}")))?;

        loop {
            match dir.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.is_ok_and(|ft| ft.is_dir());
                    entries.push(json!({
                        "name": name,
                        "is_dir": is_dir,
                    }));
                }
                Ok(None) => break,
                Err(e) => {
                    return Err(ToolError::Execution(format!("readdir entry failed: {e}")));
                }
            }
        }

        Ok(json!({ "path": path_str, "entries": entries }))
    }

    /// Execute the `local/shell_exec` tool.
    async fn shell_exec(&self, args: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let command = args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::Execution("missing 'command' argument".to_string()))?;

        let timeout_ms =
            args.get("timeout_ms").and_then(serde_json::Value::as_u64).unwrap_or(15_000);

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            tokio::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .current_dir(&self.workspace)
                .output(),
        )
        .await
        .map_err(|_| ToolError::Timeout { name: "local/shell_exec".to_string() })?
        .map_err(|e| ToolError::Execution(format!("exec failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(json!({
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

#[async_trait::async_trait]
impl bob_core::ports::ToolPort for BuiltinToolPort {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        Ok(vec![
            ToolDescriptor {
                id: "local/file_read".to_string(),
                description: "Read file contents from the workspace".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within workspace" }
                    },
                    "required": ["path"]
                }),
                source: ToolSource::Local,
            },
            ToolDescriptor {
                id: "local/file_write".to_string(),
                description: "Write content to a file in the workspace".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within workspace" },
                        "content": { "type": "string", "description": "File content to write" }
                    },
                    "required": ["path", "content"]
                }),
                source: ToolSource::Local,
            },
            ToolDescriptor {
                id: "local/file_list".to_string(),
                description: "List directory contents in the workspace".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative directory path (default: '.')" }
                    }
                }),
                source: ToolSource::Local,
            },
            ToolDescriptor {
                id: "local/shell_exec".to_string(),
                description: "Execute a shell command in the workspace directory".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to execute" },
                        "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default: 15000)" }
                    },
                    "required": ["command"]
                }),
                source: ToolSource::Local,
            },
        ])
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let result = match call.name.as_str() {
            "local/file_read" => self.file_read(&call.arguments).await,
            "local/file_write" => self.file_write(&call.arguments).await,
            "local/file_list" => self.file_list(&call.arguments).await,
            "local/shell_exec" => self.shell_exec(&call.arguments).await,
            _ => return Err(ToolError::NotFound { name: call.name }),
        };

        match result {
            Ok(output) => Ok(ToolResult { name: call.name, output, is_error: false }),
            Err(e) => Ok(ToolResult {
                name: call.name,
                output: json!({ "error": e.to_string() }),
                is_error: true,
            }),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use bob_core::ports::ToolPort;

    use super::*;

    #[tokio::test]
    async fn list_tools_returns_four_builtins() {
        let port = BuiltinToolPort::new(PathBuf::from("/tmp"));
        let tools = port.list_tools().await;
        assert!(tools.is_ok());
        let tools = tools.unwrap_or_default();
        assert_eq!(tools.len(), 4);
        assert!(tools.iter().all(|t| t.id.starts_with("local/")));
    }

    #[test]
    fn resolve_safe_path_rejects_absolute() {
        let port = BuiltinToolPort::new(PathBuf::from("/workspace"));
        assert!(port.resolve_safe_path("/etc/passwd").is_err());
    }

    #[test]
    fn resolve_safe_path_rejects_traversal() {
        let port = BuiltinToolPort::new(PathBuf::from("/workspace"));
        assert!(port.resolve_safe_path("../etc/passwd").is_err());
        assert!(port.resolve_safe_path("foo/../../etc/passwd").is_err());
    }

    #[test]
    fn resolve_safe_path_allows_relative() {
        let port = BuiltinToolPort::new(PathBuf::from("/workspace"));
        let result = port.resolve_safe_path("src/main.rs");
        assert!(result.is_ok());
        assert_eq!(result.unwrap_or_default(), PathBuf::from("/workspace/src/main.rs"));
    }

    #[tokio::test]
    async fn file_read_on_temp_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            // Fallback that won't be reached in tests but satisfies no-unwrap lint
            tempfile::TempDir::new().unwrap_or_else(|_| unreachable!())
        });
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap_or_default();

        let port = BuiltinToolPort::new(dir.path().to_path_buf());
        let result = port
            .call_tool(ToolCall {
                name: "local/file_read".to_string(),
                arguments: json!({ "path": "test.txt" }),
            })
            .await;

        assert!(result.is_ok());
        if let Ok(r) = result {
            assert!(!r.is_error);
            assert_eq!(r.output.get("content").and_then(|v| v.as_str()), Some("hello"));
        }
    }
}
