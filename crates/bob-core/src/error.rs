//! # Error Types
//!
//! Error types for the Bob Agent Framework.
//!
//! This module provides comprehensive error handling with:
//!
//! - **`AgentError`**: Top-level error enum wrapping all error types
//! - **`LlmError`**: Errors from LLM providers
//! - **`ToolError`**: Errors from tool execution
//! - **`StoreError`**: Errors from session storage
//!
//! ## Error Handling Strategy
//!
//! All errors use [`thiserror`] for ergonomic error definitions and implement:
//! - `std::error::Error` for compatibility
//! - `Display` for user-friendly messages
//! - `From` for automatic conversion
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_core::error::{AgentError, LlmError};
//!
//! fn handle_error(err: AgentError) {
//!     match err {
//!         AgentError::Llm(e) => eprintln!("LLM error: {}", e),
//!         AgentError::Tool(e) => eprintln!("Tool error: {}", e),
//!         AgentError::Policy(msg) => eprintln!("Policy violation: {}", msg),
//!         AgentError::Timeout => eprintln!("Operation timed out"),
//!         _ => eprintln!("Other error: {}", err),
//!     }
//! }
//! ```

/// Top-level agent error.
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("LLM provider error: {0}")]
    Llm(#[from] LlmError),

    #[error("Tool execution error: {0}")]
    Tool(#[from] ToolError),

    #[error("Policy violation: {0}")]
    Policy(String),

    #[error("Store error: {0}")]
    Store(#[from] StoreError),

    #[error("timeout")]
    Timeout,

    #[error("guard exceeded: {reason:?}")]
    GuardExceeded { reason: crate::types::GuardReason },

    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// LLM adapter errors.
#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("rate limited")]
    RateLimited,

    #[error("context length exceeded")]
    ContextLengthExceeded,

    #[error("stream error: {0}")]
    Stream(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Tool execution errors.
#[derive(thiserror::Error, Debug)]
pub enum ToolError {
    #[error("tool not found: {name}")]
    NotFound { name: String },

    #[error("tool execution failed: {0}")]
    Execution(String),

    #[error("tool timeout: {name}")]
    Timeout { name: String },

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Session store errors.
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage backend error: {0}")]
    Backend(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
