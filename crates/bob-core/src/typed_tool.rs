//! # Typed Tool Abstractions
//!
//! Compile-time safe tool definitions with type-driven design.
//!
//! ## Design Philosophy
//!
//! This module demonstrates extreme trait reuse and type-driven design:
//! - **Associated types** bind input/output schemas to tool definitions
//! - **Blanket implementations** enable seamless adapter composition
//! - **Extension traits** provide ergonomic fluent APIs
//! - **Type-state patterns** enforce valid tool registration order

use std::marker::PhantomData;

use crate::{
    error::ToolError,
    ports::ToolPort,
    types::{ToolCall, ToolDescriptor, ToolResult},
};

// ── Type-State Markers for Tool Registration ─────────────────────────

/// Tool definition is incomplete (missing description).
#[derive(Debug, Clone, Copy, Default)]
pub struct Incomplete;

/// Tool definition has a description but no schema.
#[derive(Debug, Clone, Copy, Default)]
pub struct Described;

/// Tool definition is complete and ready for registration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Complete;

// ── Typed Tool Builder with Compile-Time Validation ──────────────────

/// Type-safe tool builder that enforces complete definitions at compile time.
///
/// The type parameter `S` tracks the tool definition state:
/// - [`Incomplete`]: Only name is set
/// - [`Described`]: Name and description are set, schema optional
/// - [`Complete`]: All required fields are set, can register
#[derive(Debug)]
pub struct TypedToolBuilder<S = Incomplete> {
    descriptor: ToolBuilder,
    _state: PhantomData<S>,
}

/// Internal tool descriptor builder (untyped).
#[derive(Debug, Clone)]
struct ToolBuilder {
    id: String,
    description: String,
    input_schema: serde_json::Value,
    source: ToolSource,
    kind: ToolKind,
    timeout_ms: Option<u64>,
    sequential: bool,
}

impl Default for ToolBuilder {
    fn default() -> Self {
        Self {
            id: String::new(),
            description: String::new(),
            input_schema: serde_json::Value::Object(Default::default()),
            source: ToolSource::Local,
            kind: ToolKind::default(),
            timeout_ms: None,
            sequential: false,
        }
    }
}

/// Tool source (re-exported for convenience).
#[derive(Debug, Clone)]
pub enum ToolSource {
    Local,
    Mcp { server: String },
}

/// Tool kind (re-exported for convenience).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolKind {
    #[default]
    Function,
    Unapproved,
    External,
}

impl TypedToolBuilder<Incomplete> {
    /// Start building a new tool with just a name.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            descriptor: ToolBuilder { id: id.into(), ..Default::default() },
            _state: PhantomData,
        }
    }

    /// Set the tool description. Transitions to [`Described`] state.
    #[must_use]
    pub fn with_description(
        mut self,
        description: impl Into<String>,
    ) -> TypedToolBuilder<Described> {
        self.descriptor.description = description.into();
        TypedToolBuilder { descriptor: self.descriptor, _state: PhantomData }
    }
}

impl TypedToolBuilder<Described> {
    /// Set the input schema. Transitions to [`Complete`] state.
    #[must_use]
    pub fn with_schema(mut self, schema: serde_json::Value) -> TypedToolBuilder<Complete> {
        self.descriptor.input_schema = schema;
        TypedToolBuilder { descriptor: self.descriptor, _state: PhantomData }
    }

    /// Set the tool kind (stays in [`Described`] state).
    #[must_use]
    pub fn with_kind(mut self, kind: ToolKind) -> Self {
        self.descriptor.kind = kind;
        self
    }

    /// Set a per-tool timeout (stays in [`Described`] state).
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.descriptor.timeout_ms = Some(timeout_ms);
        self
    }

    /// Mark as requiring sequential execution (stays in [`Described`] state).
    #[must_use]
    pub fn with_sequential(mut self) -> Self {
        self.descriptor.sequential = true;
        self
    }

    /// Set the tool source (stays in [`Described`] state).
    #[must_use]
    pub fn with_source(mut self, source: ToolSource) -> Self {
        self.descriptor.source = source;
        self
    }
}

impl TypedToolBuilder<Complete> {
    /// Build the final tool descriptor.
    ///
    /// This method is only available when all required fields are set,
    /// enforced by the type system.
    #[must_use]
    pub fn build(self) -> ToolDescriptor {
        ToolDescriptor {
            id: self.descriptor.id,
            description: self.descriptor.description,
            input_schema: self.descriptor.input_schema,
            source: match self.descriptor.source {
                ToolSource::Local => crate::types::ToolSource::Local,
                ToolSource::Mcp { server } => crate::types::ToolSource::Mcp { server },
            },
            kind: match self.descriptor.kind {
                ToolKind::Function => crate::types::ToolKind::Function,
                ToolKind::Unapproved => crate::types::ToolKind::Unapproved,
                ToolKind::External => crate::types::ToolKind::External,
            },
            timeout_ms: self.descriptor.timeout_ms,
            sequential: self.descriptor.sequential,
        }
    }

    /// Set the tool kind (stays in [`Complete`] state).
    #[must_use]
    pub fn with_kind(mut self, kind: ToolKind) -> Self {
        self.descriptor.kind = kind;
        self
    }

    /// Set a per-tool timeout (stays in [`Complete`] state).
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.descriptor.timeout_ms = Some(timeout_ms);
        self
    }
}

// ── Typed Tool Adapter with Associated Types ─────────────────────────

/// A typed tool that binds input/output types to a tool definition.
///
/// This enables compile-time validation of tool calls and results.
///
/// # Type Parameters
///
/// - `I`: The input type (must be serializable)
/// - `O`: The output type (must be deserializable)
///
/// # Example
///
/// ```rust,ignore
/// use bob_core::typed_tool::TypedToolAdapter;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize)]
/// struct SearchInput { query: String }
///
/// #[derive(Deserialize)]
/// struct SearchOutput { results: Vec<String> }
///
/// struct SearchTool;
///
/// impl TypedToolAdapter for SearchTool {
///     type Input = SearchInput;
///     type Output = SearchOutput;
///     
///     fn descriptor() -> ToolDescriptor {
///         TypedToolBuilder::new("search")
///             .with_description("Search the web")
///             .with_schema(serde_json::json!({
///                 "type": "object",
///                 "properties": {
///                     "query": {"type": "string"}
///                 }
///             }))
///             .build()
///     }
///     
///     async fn execute(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
///         Ok(SearchOutput { results: vec![format!("Result for: {}", input.query)] })
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait TypedToolAdapter: Send + Sync {
    /// The input type for this tool.
    type Input: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;

    /// The output type for this tool.
    type Output: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;

    /// Get the tool descriptor.
    fn descriptor() -> ToolDescriptor
    where
        Self: Sized;

    /// Execute the tool with typed input/output.
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, ToolError>;
}

/// Extension trait that adapts a [`TypedToolAdapter`] to the dynamic [`ToolPort`].
///
/// This provides a blanket implementation that bridges the type-safe world
/// with the dynamic runtime world.
#[async_trait::async_trait]
pub trait TypedToolExt: TypedToolAdapter {
    /// Execute a dynamic tool call, validating input/output types at runtime.
    async fn call_dynamic(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        // Deserialize input
        let input: Self::Input = serde_json::from_value(call.arguments)
            .map_err(|e| ToolError::Execution(format!("invalid input: {e}")))?;

        // Execute with typed input
        let output = self.execute(input).await?;

        // Serialize output
        let output_value = serde_json::to_value(output)
            .map_err(|e| ToolError::Execution(format!("failed to serialize output: {e}")))?;

        Ok(ToolResult { name: call.name, output: output_value, is_error: false })
    }
}

// Blanket implementation: every TypedToolAdapter automatically implements TypedToolExt
impl<T: TypedToolAdapter + ?Sized> TypedToolExt for T {}

/// Adapter that wraps a [`TypedToolAdapter`] as a [`ToolPort`].
///
/// This enables seamless integration with the runtime's dynamic tool system.
pub struct TypedToolPort<T> {
    inner: T,
}

impl<T> TypedToolPort<T> {
    /// Wrap a typed tool adapter as a dynamic tool port.
    #[must_use]
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> std::fmt::Debug for TypedToolPort<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedToolPort").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl<T: TypedToolAdapter> ToolPort for TypedToolPort<T> {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        Ok(vec![T::descriptor()])
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        self.inner.call_dynamic(call).await
    }
}

// ── Extension Traits for Ergonomic Tool Composition ───────────────────

/// Extension trait for composing multiple tool ports.
///
/// This provides a fluent API for combining tool sources without
/// manual `Arc<dyn ToolPort>` manipulation.
pub trait ToolPortExt: ToolPort + Sized {
    /// Chain this tool port with another, combining their tool lists.
    ///
    /// Tool calls are routed to the first port that has the matching tool.
    fn chain<O: ToolPort>(self, other: O) -> ChainedToolPorts<Self, O>;

    /// Filter tools by name predicate.
    fn filter<F: Fn(&str) -> bool>(self, predicate: F) -> FilteredToolPort<Self, F>;
}

// Blanket implementation for all ToolPort implementations
impl<T: ToolPort> ToolPortExt for T {
    fn chain<O: ToolPort>(self, other: O) -> ChainedToolPorts<Self, O> {
        ChainedToolPorts { first: self, second: other }
    }

    fn filter<F: Fn(&str) -> bool>(self, predicate: F) -> FilteredToolPort<Self, F> {
        FilteredToolPort { inner: self, predicate }
    }
}

/// Chains two tool ports, routing calls to the first that has the tool.
#[derive(Debug)]
pub struct ChainedToolPorts<A, B> {
    first: A,
    second: B,
}

#[async_trait::async_trait]
impl<A: ToolPort, B: ToolPort> ToolPort for ChainedToolPorts<A, B> {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        let mut tools = self.first.list_tools().await?;
        tools.extend(self.second.list_tools().await?);
        Ok(tools)
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        // Try first port first
        let tools = self.first.list_tools().await?;
        if tools.iter().any(|t| t.id == call.name) {
            return self.first.call_tool(call).await;
        }
        // Fall back to second port
        self.second.call_tool(call).await
    }
}

/// Filters tools by a predicate.
#[derive(Debug)]
pub struct FilteredToolPort<T, F> {
    inner: T,
    predicate: F,
}

#[async_trait::async_trait]
impl<T: ToolPort, F: Fn(&str) -> bool + Send + Sync> ToolPort for FilteredToolPort<T, F> {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError> {
        let tools = self.inner.list_tools().await?;
        Ok(tools.into_iter().filter(|t| (self.predicate)(&t.id)).collect())
    }

    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if !(self.predicate)(&call.name) {
            return Err(ToolError::NotFound { name: call.name });
        }
        self.inner.call_tool(call).await
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_tool_builder_enforces_description() {
        // This compiles because we set description
        let _builder = TypedToolBuilder::new("test/tool").with_description("A test tool");
        // State is now Described, can set schema
    }

    #[test]
    fn typed_tool_builder_enforces_schema() {
        // This compiles because we set schema
        let _builder = TypedToolBuilder::new("test/tool")
            .with_description("A test tool")
            .with_schema(serde_json::json!({"type": "object"}));
        // State is now Complete, can build
    }

    #[test]
    fn typed_tool_builder_complete_can_build() {
        let descriptor = TypedToolBuilder::new("test/tool")
            .with_description("A test tool")
            .with_schema(serde_json::json!({"type": "object"}))
            .build();

        assert_eq!(descriptor.id, "test/tool");
        assert_eq!(descriptor.description, "A test tool");
    }

    struct EchoTool;

    #[async_trait::async_trait]
    impl TypedToolAdapter for EchoTool {
        type Input = String;
        type Output = String;

        fn descriptor() -> ToolDescriptor {
            TypedToolBuilder::new("echo")
                .with_description("Echo input back")
                .with_schema(serde_json::json!({
                    "type": "string"
                }))
                .build()
        }

        async fn execute(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
            Ok(input)
        }
    }

    #[tokio::test]
    async fn typed_tool_adapter_works() {
        let port = TypedToolPort::new(EchoTool);

        let tools = port.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "echo");

        let result =
            port.call_tool(ToolCall::new("echo", serde_json::json!("hello"))).await.unwrap();
        assert_eq!(result.output, serde_json::json!("hello"));
        assert!(!result.is_error);
    }
}
