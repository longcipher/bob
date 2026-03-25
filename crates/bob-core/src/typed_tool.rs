//! # Typed Tool Abstraction
//!
//! Strongly-typed tool trait that eliminates manual JSON parsing in tool
//! implementations. Developers define a struct for their input, implement
//! [`TypedTool`], and the framework handles serialization/deserialization
//! automatically.
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_core::typed_tool::TypedTool;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct ReadFileInput {
//!     path: String,
//! }
//!
//! struct ReadFileTool;
//!
//! impl TypedTool for ReadFileTool {
//!     type Input = ReadFileInput;
//!     type Output = String;
//!
//!     fn name(&self) -> &str { "read_file" }
//!     fn description(&self) -> &str { "Read a file's contents" }
//!
//!     async fn execute(&self, input: ReadFileInput) -> Result<String, ToolError> {
//!         Ok(std::fs::read_to_string(&input.path)?)
//!     }
//! }
//! ```

use serde::{Serialize, de::DeserializeOwned};

use crate::{error::ToolError, types::ToolDescriptor};

/// Strongly-typed tool trait for ergonomic tool development.
///
/// Instead of manually parsing `serde_json::Value`, developers implement
/// this trait with concrete `Input` and `Output` types. The framework's
/// [`DynamicToolAdapter`] handles the JSON boundary automatically.
///
/// # Design
///
/// - Uses native `async fn in trait` (RPITIT) — no `#[async_trait]` overhead
/// - `Input` must be deserializable (from JSON)
/// - `Output` must be serializable (to JSON)
/// - `input_schema()` can be overridden to provide JSON Schema for LLM tool discovery
pub trait TypedTool: Send + Sync {
    /// The typed input parameters for this tool.
    type Input: DeserializeOwned + Send + 'static;

    /// The typed output produced by this tool.
    type Output: Serialize + Send + 'static;

    /// Tool name (unique identifier).
    fn name(&self) -> &str;

    /// Human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// JSON Schema describing the input parameters.
    ///
    /// Override this to provide a schema for LLM tool discovery.
    /// Default returns an empty object schema.
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    /// Execute the tool with strongly-typed input.
    fn execute(
        &self,
        input: Self::Input,
    ) -> impl std::future::Future<Output = Result<Self::Output, ToolError>> + Send;
}

/// Build a [`ToolDescriptor`] from a [`TypedTool`] reference.
#[must_use]
pub fn tool_descriptor<T: TypedTool>(tool: &T) -> ToolDescriptor {
    ToolDescriptor::new(tool.name(), tool.description()).with_input_schema(tool.input_schema())
}

/// Adapter that bridges a [`TypedTool`] to the framework's dynamic
/// [`ToolPort`](crate::ports::ToolPort) interface.
///
/// This is the glue layer that:
/// 1. Deserializes `serde_json::Value` → `T::Input`
/// 2. Calls `T::execute(input)`
/// 3. Serializes `T::Output` → `serde_json::Value`
///
/// ## Usage
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use bob_core::typed_tool::{TypedTool, DynamicToolAdapter};
///
/// let adapter = DynamicToolAdapter::new(MyTypedTool::new());
/// // Use as Arc<dyn ToolPort>
/// let tool_port: Arc<dyn ToolPort> = Arc::new(adapter);
/// ```
pub struct DynamicToolAdapter<T: TypedTool> {
    inner: T,
}

impl<T: TypedTool> DynamicToolAdapter<T> {
    /// Wrap a [`TypedTool`] instance for use as a dynamic [`ToolPort`](crate::ports::ToolPort).
    #[must_use]
    pub fn new(tool: T) -> Self {
        Self { inner: tool }
    }

    /// Get a reference to the inner typed tool.
    #[must_use]
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: TypedTool> std::fmt::Debug for DynamicToolAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicToolAdapter").field("name", &self.inner.name()).finish()
    }
}

#[async_trait::async_trait]
impl<T: TypedTool> crate::ports::ToolPort for DynamicToolAdapter<T> {
    async fn list_tools(
        &self,
    ) -> Result<Vec<crate::types::ToolDescriptor>, crate::error::ToolError> {
        Ok(vec![tool_descriptor(&self.inner)])
    }

    async fn call_tool(
        &self,
        call: crate::types::ToolCall,
    ) -> Result<crate::types::ToolResult, crate::error::ToolError> {
        let input: T::Input = serde_json::from_value(call.arguments).map_err(|e| {
            ToolError::Execution(format!("invalid arguments for '{}': {}", self.inner.name(), e))
        })?;

        let output = self.inner.execute(input).await?;
        let output_value = serde_json::to_value(output).unwrap_or(serde_json::Value::Null);

        Ok(crate::types::ToolResult { name: call.name, output: output_value, is_error: false })
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::ports::ToolPort;

    #[derive(Deserialize)]
    struct EchoInput {
        message: String,
    }

    struct EchoTool;

    impl TypedTool for EchoTool {
        type Input = EchoInput;
        type Output = String;

        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo a message back"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }

        async fn execute(&self, input: EchoInput) -> Result<String, ToolError> {
            Ok(input.message)
        }
    }

    #[tokio::test]
    async fn dynamic_adapter_call_tool() {
        let adapter = DynamicToolAdapter::new(EchoTool);
        let call = crate::types::ToolCall::new("echo", serde_json::json!({"message": "hello"}));
        let result = adapter.call_tool(call).await.expect("should succeed");
        assert!(!result.is_error);
        assert_eq!(result.output, serde_json::json!("hello"));
    }

    #[tokio::test]
    async fn dynamic_adapter_invalid_args() {
        let adapter = DynamicToolAdapter::new(EchoTool);
        let call = crate::types::ToolCall::new("echo", serde_json::json!({"wrong_field": 42}));
        let result = adapter.call_tool(call).await;
        assert!(result.is_err(), "should fail with invalid arguments");
    }

    #[tokio::test]
    async fn dynamic_adapter_list_tools() {
        let adapter = DynamicToolAdapter::new(EchoTool);
        let tools = adapter.list_tools().await.expect("should list tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "echo");
        assert_eq!(tools[0].description, "Echo a message back");
    }

    #[test]
    fn tool_descriptor_from_typed_tool() {
        let descriptor = tool_descriptor(&EchoTool);
        assert_eq!(descriptor.id, "echo");
        assert_eq!(descriptor.description, "Echo a message back");
    }
}
