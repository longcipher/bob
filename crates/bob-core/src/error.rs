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
//! ## Error Codes
//!
//! Every error variant carries a stable machine-readable **error code**
//! (e.g. `"BOB_LLM_RATE_LIMITED"`, `"BOB_TOOL_TIMEOUT"`).
//! Codes are stable across patch releases and can be used for monitoring,
//! alerting rules, and structured error reporting.
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
//!     eprintln!("[{}] {}", err.code(), err);
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

    #[error("configuration error: {0}")]
    Config(String),

    #[error("Store error: {0}")]
    Store(#[from] StoreError),

    #[error("Cost meter error: {0}")]
    Cost(#[from] CostError),

    #[error("timeout")]
    Timeout,

    #[error("guard exceeded: {reason:?}")]
    GuardExceeded { reason: crate::types::GuardReason },

    #[error("conflict: {0}")]
    Conflict(String),

    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl AgentError {
    /// Stable machine-readable error code for monitoring and alerting.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Llm(e) => e.code(),
            Self::Tool(e) => e.code(),
            Self::Policy(_) => "BOB_POLICY_VIOLATION",
            Self::Config(_) => "BOB_CONFIG_ERROR",
            Self::Store(e) => e.code(),
            Self::Cost(e) => e.code(),
            Self::Timeout => "BOB_TIMEOUT",
            Self::GuardExceeded { .. } => "BOB_GUARD_EXCEEDED",
            Self::Conflict(_) => "BOB_CONFLICT",
            Self::Internal(_) => "BOB_INTERNAL",
        }
    }
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

impl LlmError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Provider(_) => "BOB_LLM_PROVIDER",
            Self::RateLimited => "BOB_LLM_RATE_LIMITED",
            Self::ContextLengthExceeded => "BOB_LLM_CONTEXT_LENGTH",
            Self::Stream(_) => "BOB_LLM_STREAM",
            Self::Other(_) => "BOB_LLM_OTHER",
        }
    }
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

impl ToolError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "BOB_TOOL_NOT_FOUND",
            Self::Execution(_) => "BOB_TOOL_EXECUTION",
            Self::Timeout { .. } => "BOB_TOOL_TIMEOUT",
            Self::Other(_) => "BOB_TOOL_OTHER",
        }
    }
}

/// Session store errors.
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage backend error: {0}")]
    Backend(String),

    #[error("version conflict: expected version {expected}, found {actual}")]
    VersionConflict { expected: u64, actual: u64 },

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl StoreError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Serialization(_) => "BOB_STORE_SERIALIZATION",
            Self::Backend(_) => "BOB_STORE_BACKEND",
            Self::VersionConflict { .. } => "BOB_STORE_VERSION_CONFLICT",
            Self::Other(_) => "BOB_STORE_OTHER",
        }
    }
}

/// Cost meter errors.
#[derive(thiserror::Error, Debug)]
pub enum CostError {
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("cost backend error: {0}")]
    Backend(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl CostError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BudgetExceeded(_) => "BOB_COST_BUDGET_EXCEEDED",
            Self::Backend(_) => "BOB_COST_BACKEND",
            Self::Other(_) => "BOB_COST_OTHER",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_error_codes_are_stable() {
        assert_eq!(AgentError::Timeout.code(), "BOB_TIMEOUT");
        assert_eq!(AgentError::Policy("x".into()).code(), "BOB_POLICY_VIOLATION");
        assert_eq!(AgentError::Config("x".into()).code(), "BOB_CONFIG_ERROR");
        assert_eq!(AgentError::Conflict("x".into()).code(), "BOB_CONFLICT");
    }

    #[test]
    fn llm_error_codes_are_stable() {
        assert_eq!(LlmError::RateLimited.code(), "BOB_LLM_RATE_LIMITED");
        assert_eq!(LlmError::ContextLengthExceeded.code(), "BOB_LLM_CONTEXT_LENGTH");
        assert_eq!(LlmError::Provider("x".into()).code(), "BOB_LLM_PROVIDER");
    }

    #[test]
    fn tool_error_codes_are_stable() {
        assert_eq!(ToolError::NotFound { name: "x".into() }.code(), "BOB_TOOL_NOT_FOUND");
        assert_eq!(ToolError::Timeout { name: "x".into() }.code(), "BOB_TOOL_TIMEOUT");
    }

    #[test]
    fn store_error_codes_are_stable() {
        assert_eq!(
            StoreError::VersionConflict { expected: 1, actual: 2 }.code(),
            "BOB_STORE_VERSION_CONFLICT"
        );
        assert_eq!(StoreError::Backend("x".into()).code(), "BOB_STORE_BACKEND");
    }

    #[test]
    fn cost_error_codes_are_stable() {
        assert_eq!(CostError::BudgetExceeded("x".into()).code(), "BOB_COST_BUDGET_EXCEEDED");
    }

    #[test]
    fn error_code_wraps_correctly() {
        let llm_err = AgentError::Llm(LlmError::RateLimited);
        assert_eq!(llm_err.code(), "BOB_LLM_RATE_LIMITED");

        let tool_err = AgentError::Tool(ToolError::Timeout { name: "t".into() });
        assert_eq!(tool_err.code(), "BOB_TOOL_TIMEOUT");

        let store_err = AgentError::Store(StoreError::VersionConflict { expected: 1, actual: 2 });
        assert_eq!(store_err.code(), "BOB_STORE_VERSION_CONFLICT");
    }
}
