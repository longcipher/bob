//! # Channel Abstraction
//!
//! Multi-endpoint channel interface for the Bob Agent Framework.
//!
//! A **channel** is the boundary between the outside world (user) and the
//! agent loop. Its only responsibility is receiving user input and delivering
//! agent output — no agent logic lives here.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────┐       ┌────────────┐       ┌───────────┐
//! │  User   │ ───→  │  Channel   │ ───→  │ AgentLoop │
//! │         │ ←───  │ (trait)    │ ←───  │           │
//! └─────────┘       └────────────┘       └───────────┘
//! ```
//!
//! Concrete implementations live in adapter or binary crates:
//!
//! | Channel      | Scenario                              |
//! |-------------|---------------------------------------|
//! | CLI          | One-shot `run --prompt "..."`          |
//! | REPL         | Interactive terminal                   |
//! | Telegram Bot | Long-polling with ACL                  |
//! | Discord Bot  | Gateway-based with channel filtering  |

use serde::{Deserialize, Serialize};

/// Incoming message from a channel to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    /// Raw user text (may be a slash command or natural language).
    pub text: String,
    /// Session identifier for conversation continuity.
    pub session_id: String,
    /// Optional sender identifier (username, user-id, etc.).
    pub sender: Option<String>,
}

/// Outgoing agent response delivered back through the channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOutput {
    /// Response text.
    pub text: String,
    /// Whether this output represents an error condition.
    pub is_error: bool,
}

/// Errors that can occur during channel I/O.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// Sending a message to the user failed.
    #[error("send failed: {0}")]
    SendFailed(String),
    /// The requested feature is not supported by this channel.
    #[error("not supported: {0}")]
    NotSupported(String),
    /// The channel has been closed (user disconnected, etc.).
    #[error("channel closed")]
    Closed,
}

/// Abstract channel for multi-endpoint agent interaction.
///
/// Each channel implementation handles the transport-specific details
/// of receiving user input and delivering agent output. The agent loop
/// is decoupled from any specific transport.
///
/// ## Implementing a Channel
///
/// ```rust,ignore
/// use bob_core::channel::{Channel, ChannelMessage, ChannelOutput, ChannelError};
///
/// struct MyTelegramChannel { /* ... */ }
///
/// #[async_trait::async_trait]
/// impl Channel for MyTelegramChannel {
///     async fn recv(&mut self) -> Option<ChannelMessage> {
///         // Poll Telegram API for new messages...
///     }
///
///     async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError> {
///         // Send via Telegram Bot API...
///         Ok(())
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Channel: Send {
    /// Receive the next user input message.
    ///
    /// Returns `None` when the channel is closed (e.g. user disconnected,
    /// stdin EOF, or graceful shutdown).
    async fn recv(&mut self) -> Option<ChannelMessage>;

    /// Send an agent response back to the user.
    async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError>;
}
