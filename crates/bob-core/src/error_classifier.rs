//! # Error Classifier
//!
//! Classifies errors into actionable categories for retry and failover decisions.
//!
//! Inspired by rustic-ai's error classification approach, this module provides
//! classifiers that categorize errors by their error codes for decision-making.
//!
//! ## Categories
//!
//! | Category         | Meaning                                     | Failover? |
//! |------------------|---------------------------------------------|-----------|
//! | `"timeout"`      | Operation exceeded time limit               | Yes       |
//! | `"rate_limited"` | Provider returned HTTP 429                  | Yes       |
//! | `"http_5xx"`     | Provider returned server error              | Yes       |
//! | `"connect_error"`| Network/connection failure                  | Yes       |
//! | `"context_length"`| Input exceeded model context window         | No        |
//! | `"other"`        | Unknown or unclassified error               | Config    |

use crate::error::{AgentError, LlmError};

/// Classify an [`AgentError`] into a stable category string.
#[must_use]
pub fn classify_agent_error(err: &AgentError) -> &'static str {
    match err {
        AgentError::Llm(llm) => classify_llm_error(llm),
        AgentError::Timeout => "timeout",
        AgentError::Tool(crate::error::ToolError::Timeout { .. }) => "timeout",
        AgentError::Tool(_) => "other",
        _ => "other",
    }
}

/// Classify an [`LlmError`] into a stable category string.
#[must_use]
pub fn classify_llm_error(err: &LlmError) -> &'static str {
    match err {
        LlmError::RateLimited => "rate_limited",
        LlmError::ContextLengthExceeded => "context_length",
        LlmError::Provider(msg) => classify_provider_message(msg),
        LlmError::Stream(msg) => classify_provider_message(msg),
        LlmError::Other(_) => "other",
    }
}

/// Classify a free-form error message string.
fn classify_provider_message(msg: &str) -> &'static str {
    let lower = msg.to_lowercase();
    if lower.contains("429") || lower.contains("rate") || lower.contains("throttl") {
        "rate_limited"
    } else if lower.contains("500") || lower.contains("502") || lower.contains("503") {
        "http_5xx"
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("connect") || lower.contains("dns") || lower.contains("network") {
        "connect_error"
    } else if lower.contains("context length") || lower.contains("maximum") {
        "context_length"
    } else {
        "other"
    }
}

/// Configuration for which error categories should trigger failover.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Error categories that should trigger failover to a backup provider.
    pub failover_on: Vec<String>,
    /// Maximum retries on the primary provider before failing over.
    pub retry_limit: u32,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failover_on: vec![
                "timeout".into(),
                "rate_limited".into(),
                "http_5xx".into(),
                "connect_error".into(),
            ],
            retry_limit: 2,
        }
    }
}

/// Result of a failover-aware execution.
#[derive(Debug, Clone)]
pub struct FailoverResult<T> {
    /// The successful value.
    pub value: T,
    /// Whether failover to a backup provider occurred.
    pub failed_over: bool,
    /// Number of attempts on the primary provider.
    pub primary_attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rate_limited() {
        let err = LlmError::RateLimited;
        assert_eq!(classify_llm_error(&err), "rate_limited");
    }

    #[test]
    fn classify_context_length() {
        let err = LlmError::ContextLengthExceeded;
        assert_eq!(classify_llm_error(&err), "context_length");
    }

    #[test]
    fn classify_timeout_from_provider_message() {
        let err = LlmError::Provider("request timed out".into());
        assert_eq!(classify_llm_error(&err), "timeout");
    }

    #[test]
    fn classify_429_from_provider_message() {
        let err = LlmError::Provider("HTTP 429 Too Many Requests".into());
        assert_eq!(classify_llm_error(&err), "rate_limited");
    }

    #[test]
    fn classify_503_from_provider_message() {
        let err = LlmError::Provider("HTTP 503 Service Unavailable".into());
        assert_eq!(classify_llm_error(&err), "http_5xx");
    }

    #[test]
    fn classify_connect_error() {
        let err = LlmError::Provider("connection refused".into());
        assert_eq!(classify_llm_error(&err), "connect_error");
    }

    #[test]
    fn classify_unknown_returns_other() {
        let err = LlmError::Provider("something weird".into());
        assert_eq!(classify_llm_error(&err), "other");
    }

    #[test]
    fn classify_agent_timeout() {
        let err = AgentError::Timeout;
        assert_eq!(classify_agent_error(&err), "timeout");
    }

    #[test]
    fn classify_agent_llm_wrapped() {
        let err = AgentError::Llm(LlmError::RateLimited);
        assert_eq!(classify_agent_error(&err), "rate_limited");
    }

    #[test]
    fn classify_agent_tool_timeout() {
        let err = AgentError::Tool(crate::error::ToolError::Timeout { name: "x".into() });
        assert_eq!(classify_agent_error(&err), "timeout");
    }

    #[test]
    fn failover_config_defaults() {
        let config = FailoverConfig::default();
        assert_eq!(config.retry_limit, 2);
        assert!(config.failover_on.contains(&"timeout".to_string()));
        assert!(config.failover_on.contains(&"rate_limited".to_string()));
        assert!(config.failover_on.contains(&"http_5xx".to_string()));
        assert!(config.failover_on.contains(&"connect_error".to_string()));
    }
}
