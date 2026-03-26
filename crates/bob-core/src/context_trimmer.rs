//! # Context Trimmer
//!
//! Pluggable context window management strategies for the Bob Agent Framework.
//!
//! ## Overview
//!
//! As conversations grow, the context window can exceed model token limits.
//! The [`ContextTrimmer`] trait provides pluggable strategies for managing
//! context size before sending to the LLM.
//!
//! ## Strategies
//!
//! - [`SlidingWindowTrimmer`] — Keeps the N most recent messages (simple, fast)
//! - [`SummarizationTrimmer`] — Uses a small model to compress older messages
//! - [`HybridTrimmer`] — Sliding window with periodic summarization
//!
//! ## Example
//!
//! ```rust,ignore
//! use bob_core::context_trimmer::{ContextTrimmer, SlidingWindowTrimmer};
//!
//! let trimmer = SlidingWindowTrimmer::new(50, 4096);
//! let trimmed = trimmer.trim(&session.messages, &session.total_usage).await?;
//! ```

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    error::AgentError,
    types::{Message, Role, TokenUsage},
};

/// Configuration for context trimming behavior.
#[derive(Debug, Clone)]
pub struct TrimConfig {
    /// Maximum number of non-system messages to retain.
    pub max_messages: usize,
    /// Target token budget (approximate).
    pub target_tokens: usize,
    /// Whether to preserve the first user message (often contains the original task).
    pub preserve_first_user: bool,
    /// Ratio of context at which summarization triggers (0.0-1.0).
    pub summarization_threshold: f64,
}

impl Default for TrimConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            target_tokens: 8192,
            preserve_first_user: true,
            summarization_threshold: 0.8,
        }
    }
}

/// Result of a context trimming operation.
#[derive(Debug, Clone)]
pub struct TrimResult {
    /// The trimmed messages ready for LLM consumption.
    pub messages: Vec<Message>,
    /// Whether summarization was performed.
    pub was_summarized: bool,
    /// Estimated token count after trimming.
    pub estimated_tokens: usize,
    /// Number of messages dropped.
    pub messages_dropped: usize,
}

/// Trait for pluggable context trimming strategies.
///
/// Implementors define how to reduce message history to fit within
/// token or message count limits.
#[async_trait]
pub trait ContextTrimmer: Send + Sync {
    /// Trim the message history according to the strategy.
    ///
    /// # Arguments
    ///
    /// * `messages` — Full message history (including system messages)
    /// * `usage` — Current token usage statistics
    ///
    /// # Returns
    ///
    /// A [`TrimResult`] containing the trimmed messages and metadata.
    async fn trim(
        &self,
        messages: &[Message],
        usage: &TokenUsage,
    ) -> Result<TrimResult, AgentError>;

    /// Human-readable name of this trimming strategy.
    fn strategy_name(&self) -> &'static str;
}

/// Simple sliding window trimmer.
///
/// Keeps the most recent N messages, always preserving system messages
/// and optionally the first user message.
///
/// ## Example
///
/// ```rust,ignore
/// use bob_core::context_trimmer::{ContextTrimmer, SlidingWindowTrimmer};
///
/// // Keep 50 most recent messages, target ~8K tokens
/// let trimmer = SlidingWindowTrimmer::new(50, 8192);
/// ```
#[derive(Debug, Clone)]
pub struct SlidingWindowTrimmer {
    config: TrimConfig,
}

impl SlidingWindowTrimmer {
    /// Create a new sliding window trimmer.
    #[must_use]
    pub fn new(max_messages: usize, target_tokens: usize) -> Self {
        Self {
            config: TrimConfig {
                max_messages,
                target_tokens,
                preserve_first_user: true,
                summarization_threshold: 1.0, // Never summarize
            },
        }
    }

    /// Create with full configuration.
    #[must_use]
    pub fn with_config(config: TrimConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ContextTrimmer for SlidingWindowTrimmer {
    async fn trim(
        &self,
        messages: &[Message],
        _usage: &TokenUsage,
    ) -> Result<TrimResult, AgentError> {
        let original_count = messages.len();
        let trimmed = sliding_window_trim(
            messages,
            self.config.max_messages,
            self.config.preserve_first_user,
        );
        let dropped = original_count.saturating_sub(trimmed.len());
        let estimated = estimate_tokens(&trimmed);

        Ok(TrimResult {
            messages: trimmed,
            was_summarized: false,
            estimated_tokens: estimated,
            messages_dropped: dropped,
        })
    }

    fn strategy_name(&self) -> &'static str {
        "sliding_window"
    }
}

/// Summarization-based trimmer.
///
/// When context approaches the limit, uses a callback to summarize
/// older messages into a compact form.
///
/// ## Example
///
/// ```rust,ignore
/// use bob_core::context_trimmer::{ContextTrimmer, SummarizationTrimmer};
///
/// let trimmer = SummarizationTrimmer::new(
///     100,
///     4096,
///     Arc::new(my_summarizer), // impl MessageSummarizer
/// );
/// ```
#[derive(Debug)]
pub struct SummarizationTrimmer {
    config: TrimConfig,
    summarizer: Arc<dyn MessageSummarizer>,
}

impl SummarizationTrimmer {
    /// Create a new summarization trimmer.
    #[must_use]
    pub fn new(
        max_messages: usize,
        target_tokens: usize,
        summarizer: Arc<dyn MessageSummarizer>,
    ) -> Self {
        Self {
            config: TrimConfig {
                max_messages,
                target_tokens,
                preserve_first_user: true,
                summarization_threshold: 0.8,
            },
            summarizer,
        }
    }

    /// Create with full configuration.
    #[must_use]
    pub fn with_config(config: TrimConfig, summarizer: Arc<dyn MessageSummarizer>) -> Self {
        Self { config, summarizer }
    }
}

#[async_trait]
impl ContextTrimmer for SummarizationTrimmer {
    async fn trim(
        &self,
        messages: &[Message],
        _usage: &TokenUsage,
    ) -> Result<TrimResult, AgentError> {
        let estimated = estimate_tokens(messages);
        let threshold =
            (self.config.target_tokens as f64 * self.config.summarization_threshold) as usize;

        // If under threshold, use sliding window
        if estimated < threshold {
            let trimmed = sliding_window_trim(
                messages,
                self.config.max_messages,
                self.config.preserve_first_user,
            );
            let trimmed_tokens = estimate_tokens(&trimmed);
            let dropped = messages.len().saturating_sub(trimmed.len());
            return Ok(TrimResult {
                messages: trimmed,
                was_summarized: false,
                estimated_tokens: trimmed_tokens,
                messages_dropped: dropped,
            });
        }

        // Summarize older messages
        let (old_messages, recent_messages) = split_at_threshold(
            messages,
            self.config.max_messages / 2,
            self.config.preserve_first_user,
        );

        if old_messages.is_empty() {
            let trimmed = sliding_window_trim(
                messages,
                self.config.max_messages,
                self.config.preserve_first_user,
            );
            let trimmed_tokens = estimate_tokens(&trimmed);
            let dropped = messages.len().saturating_sub(trimmed.len());
            return Ok(TrimResult {
                messages: trimmed,
                was_summarized: false,
                estimated_tokens: trimmed_tokens,
                messages_dropped: dropped,
            });
        }

        let summary = self.summarizer.summarize(&old_messages).await?;
        let mut result_messages = Vec::with_capacity(1 + recent_messages.len());
        result_messages.push(Message::text(
            Role::System,
            format!("Previous conversation summary:\n{summary}"),
        ));
        result_messages.extend(recent_messages);

        Ok(TrimResult {
            messages: result_messages.clone(),
            was_summarized: true,
            estimated_tokens: estimate_tokens(&result_messages),
            messages_dropped: old_messages.len(),
        })
    }

    fn strategy_name(&self) -> &'static str {
        "summarization"
    }
}

/// Trait for message summarization backends.
///
/// Implementations can use a small LLM, extractive summarization,
/// or any other method to compress messages.
#[async_trait]
pub trait MessageSummarizer: Send + Sync + std::fmt::Debug {
    /// Summarize a batch of messages into a concise text.
    async fn summarize(&self, messages: &[Message]) -> Result<String, AgentError>;
}

/// Hybrid trimmer that combines sliding window with periodic summarization.
///
/// Maintains a sliding window but periodically summarizes old context
/// when approaching token limits.
#[derive(Debug)]
pub struct HybridTrimmer {
    sliding: SlidingWindowTrimmer,
    summarizer: Arc<dyn MessageSummarizer>,
    summarization_threshold: f64,
    target_tokens: usize,
}

impl HybridTrimmer {
    /// Create a new hybrid trimmer.
    #[must_use]
    pub fn new(
        max_messages: usize,
        target_tokens: usize,
        summarizer: Arc<dyn MessageSummarizer>,
    ) -> Self {
        Self {
            sliding: SlidingWindowTrimmer::new(max_messages, target_tokens),
            summarizer,
            summarization_threshold: 0.8,
            target_tokens,
        }
    }
}

#[async_trait]
impl ContextTrimmer for HybridTrimmer {
    async fn trim(
        &self,
        messages: &[Message],
        usage: &TokenUsage,
    ) -> Result<TrimResult, AgentError> {
        let estimated = estimate_tokens(messages);
        let threshold = (self.target_tokens as f64 * self.summarization_threshold) as usize;

        // Under threshold: use sliding window
        if estimated < threshold {
            return self.sliding.trim(messages, usage).await;
        }

        // Over threshold: summarize
        let split_point = messages.len() / 2;
        let (old_messages, recent_messages) = messages.split_at(split_point);

        let summary = self.summarizer.summarize(old_messages).await?;
        let mut result_messages = Vec::with_capacity(1 + recent_messages.len());
        result_messages.push(Message::text(
            Role::System,
            format!("Previous conversation summary:\n{summary}"),
        ));
        result_messages.extend_from_slice(recent_messages);

        Ok(TrimResult {
            messages: result_messages.clone(),
            was_summarized: true,
            estimated_tokens: estimate_tokens(&result_messages),
            messages_dropped: old_messages.len(),
        })
    }

    fn strategy_name(&self) -> &'static str {
        "hybrid"
    }
}

/// No-op trimmer that passes messages through unchanged.
///
/// Useful when no trimming is desired but the `ContextTrimmer`
/// trait is required by an interface.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpTrimmer;

#[async_trait]
impl ContextTrimmer for NoOpTrimmer {
    async fn trim(
        &self,
        messages: &[Message],
        _usage: &TokenUsage,
    ) -> Result<TrimResult, AgentError> {
        Ok(TrimResult {
            messages: messages.to_vec(),
            was_summarized: false,
            estimated_tokens: estimate_tokens(messages),
            messages_dropped: 0,
        })
    }

    fn strategy_name(&self) -> &'static str {
        "noop"
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Sliding window: keep most recent non-system messages, preserve system.
fn sliding_window_trim(
    messages: &[Message],
    max: usize,
    preserve_first_user: bool,
) -> Vec<Message> {
    let non_system: Vec<(usize, &Message)> =
        messages.iter().enumerate().filter(|(_, m)| m.role != Role::System).collect();

    if non_system.len() <= max {
        return messages.to_vec();
    }

    let first_user_idx =
        if preserve_first_user { messages.iter().position(|m| m.role == Role::User) } else { None };

    let recent_start = non_system.len().saturating_sub(max);
    let mut to_keep: std::collections::HashSet<usize> =
        non_system[recent_start..].iter().map(|(idx, _)| *idx).collect();

    // Add first user message if it's not already in the kept set
    if let Some(first_idx) = first_user_idx {
        to_keep.insert(first_idx);
    }

    messages
        .iter()
        .enumerate()
        .filter(|(idx, msg)| msg.role == Role::System || to_keep.contains(idx))
        .map(|(_, msg)| msg.clone())
        .collect()
}

/// Split messages into old and recent halves, preserving system messages and optionally the first
/// user message.
fn split_at_threshold(
    messages: &[Message],
    recent_count: usize,
    preserve_first_user: bool,
) -> (Vec<Message>, Vec<Message>) {
    let non_system: Vec<(usize, &Message)> =
        messages.iter().enumerate().filter(|(_, m)| m.role != Role::System).collect();

    if non_system.len() <= recent_count {
        return (Vec::new(), messages.to_vec());
    }

    let split_idx = non_system.len() - recent_count;
    let split_at = non_system[split_idx].0;

    let first_user_idx =
        if preserve_first_user { messages.iter().position(|m| m.role == Role::User) } else { None };

    let mut old = Vec::new();
    let mut recent = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.role == Role::System {
            // System messages go to both (or just recent)
            recent.push(msg.clone());
        } else if idx < split_at {
            if Some(idx) == first_user_idx {
                recent.push(msg.clone()); // Keep first user in recent
            } else {
                old.push(msg.clone());
            }
        } else {
            recent.push(msg.clone());
        }
    }

    (old, recent)
}

/// Rough token estimation (4 chars ≈ 1 token).
fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| m.content.len() / 4).sum()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, content: &str) -> Message {
        Message::text(role, content.to_string())
    }

    // ── Sliding Window ──────────────────────────────────────────────

    #[tokio::test]
    async fn sliding_window_noop_when_under_limit() {
        let trimmer = SlidingWindowTrimmer::new(50, 8192);
        let messages = vec![msg(Role::User, "hello"), msg(Role::Assistant, "hi")];
        let usage = TokenUsage::default();

        let result = trimmer.trim(&messages, &usage).await.unwrap();
        assert_eq!(result.messages.len(), 2);
        assert!(!result.was_summarized);
        assert_eq!(result.messages_dropped, 0);
    }

    #[tokio::test]
    async fn sliding_window_drops_oldest() {
        let trimmer = SlidingWindowTrimmer::new(3, 8192);
        let messages = vec![
            msg(Role::User, "msg-0"),
            msg(Role::Assistant, "msg-1"),
            msg(Role::User, "msg-2"),
            msg(Role::Assistant, "msg-3"),
            msg(Role::User, "msg-4"),
        ];
        let usage = TokenUsage::default();

        let result = trimmer.trim(&messages, &usage).await.unwrap();
        assert_eq!(result.messages.len(), 4); // 3 recent + 1 system (first user preserved)
        assert_eq!(result.messages_dropped, 1);
    }

    #[tokio::test]
    async fn sliding_window_preserves_system() {
        let trimmer = SlidingWindowTrimmer::new(2, 8192);
        let messages = vec![
            msg(Role::System, "system instructions"),
            msg(Role::User, "old"),
            msg(Role::Assistant, "mid"),
            msg(Role::User, "new"),
        ];
        let usage = TokenUsage::default();

        let result = trimmer.trim(&messages, &usage).await.unwrap();
        assert_eq!(result.messages[0].role, Role::System);
        assert!(result.messages.iter().any(|m| m.content == "new"));
    }

    #[tokio::test]
    async fn sliding_window_preserves_first_user() {
        let trimmer = SlidingWindowTrimmer::new(2, 8192);
        let messages = vec![
            msg(Role::User, "original-task"),
            msg(Role::Assistant, "response-1"),
            msg(Role::User, "follow-up"),
            msg(Role::Assistant, "response-2"),
        ];
        let usage = TokenUsage::default();

        let result = trimmer.trim(&messages, &usage).await.unwrap();
        assert!(result.messages.iter().any(|m| m.content == "original-task"));
    }

    // ── NoOp Trimmer ────────────────────────────────────────────────

    #[tokio::test]
    async fn noop_trimmer_passes_through() {
        let trimmer = NoOpTrimmer;
        let messages = vec![msg(Role::User, "a"), msg(Role::Assistant, "b")];
        let usage = TokenUsage::default();

        let result = trimmer.trim(&messages, &usage).await.unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages_dropped, 0);
        assert!(!result.was_summarized);
    }

    // ── Token Estimation ────────────────────────────────────────────

    #[test]
    fn estimate_tokens_basic() {
        let messages = vec![msg(Role::User, "hello world")]; // 11 chars ≈ 2 tokens
        assert_eq!(estimate_tokens(&messages), 2);
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(&[]), 0);
    }
}
