//! Core message types for the chat channel layer.

use serde::{Deserialize, Serialize};

use crate::file::Attachment;

// ---------------------------------------------------------------------------
// Author
// ---------------------------------------------------------------------------

/// Identifies the author of an incoming message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Platform-specific user identifier.
    pub user_id: String,
    /// Short username / handle.
    pub user_name: String,
    /// Display name.
    pub full_name: String,
    /// Whether this author is a bot.
    pub is_bot: bool,
}

// ---------------------------------------------------------------------------
// Postable messages
// ---------------------------------------------------------------------------

/// A message that the agent wants to send to a channel.
///
/// Additional variants (`Card`, `Stream`) will be added when the
/// corresponding modules are implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PostableMessage {
    /// Plain text content.
    Text(String),
    /// Markdown-formatted content.
    Markdown(String),
}

/// Adapter-level postable message, extending [`PostableMessage`] with
/// attachment support.
///
/// Additional variants (`Card`, `WithAttachments`) will be added when
/// the corresponding modules are implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AdapterPostableMessage {
    /// Plain text content.
    Text(String),
    /// Markdown-formatted content.
    Markdown(String),
}

impl From<String> for PostableMessage {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for PostableMessage {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Sent / Ephemeral / Incoming
// ---------------------------------------------------------------------------

/// Confirmation returned after a message has been successfully sent.
///
/// The `raw` field carries the adapter-native response for advanced
/// use-cases and is intentionally excluded from serialization.
pub struct SentMessage {
    /// Platform-assigned message identifier.
    pub id: String,
    /// Thread the message belongs to.
    pub thread_id: String,
    /// Name of the adapter that sent this message.
    pub adapter_name: String,
    /// Adapter-native raw response object.
    pub raw: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl std::fmt::Debug for SentMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SentMessage")
            .field("id", &self.id)
            .field("thread_id", &self.thread_id)
            .field("adapter_name", &self.adapter_name)
            .field("raw", &self.raw.as_ref().map(|_| "..."))
            .finish()
    }
}

/// An ephemeral (visible-only-to-user) message confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralMessage {
    /// Platform-assigned message identifier.
    pub id: String,
    /// Thread the message belongs to.
    pub thread_id: String,
    /// Whether a plaintext fallback was used because the adapter does
    /// not support true ephemeral messages.
    pub used_fallback: bool,
}

/// A message received from an external chat platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// Platform-assigned message identifier.
    pub id: String,
    /// The text body of the message.
    pub text: String,
    /// Who sent this message.
    pub author: Author,
    /// Attachments included with the message.
    pub attachments: Vec<Attachment>,
    /// Whether the bot was explicitly mentioned.
    pub is_mention: bool,
    /// Thread this message belongs to.
    pub thread_id: String,
    /// ISO 8601 timestamp of the message, if available.
    pub timestamp: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_serde_roundtrip() {
        let author = Author {
            user_id: "U123".into(),
            user_name: "alice".into(),
            full_name: "Alice Smith".into(),
            is_bot: false,
        };
        let json = serde_json::to_string(&author).expect("serialize");
        let back: Author = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.user_id, "U123");
        assert!(!back.is_bot);
    }

    #[test]
    fn postable_message_serde_roundtrip() {
        let msg = PostableMessage::Text("hello".into());
        let json = serde_json::to_string(&msg).expect("serialize");
        let back: PostableMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, PostableMessage::Text(t) if t == "hello"));
    }

    #[test]
    fn sent_message_debug() {
        let sm = SentMessage {
            id: "msg-1".into(),
            thread_id: "t-1".into(),
            adapter_name: "slack".into(),
            raw: None,
        };
        let dbg = format!("{sm:?}");
        assert!(dbg.contains("msg-1"));
    }

    #[test]
    fn ephemeral_message_serde() {
        let em =
            EphemeralMessage { id: "e-1".into(), thread_id: "t-1".into(), used_fallback: true };
        let json = serde_json::to_string(&em).expect("serialize");
        assert!(json.contains("used_fallback"));
    }

    #[test]
    fn incoming_message_debug() {
        let msg = IncomingMessage {
            id: "m-1".into(),
            text: "hi bot".into(),
            author: Author {
                user_id: "U1".into(),
                user_name: "bob".into(),
                full_name: "Bob".into(),
                is_bot: false,
            },
            attachments: vec![],
            is_mention: true,
            thread_id: "t-1".into(),
            timestamp: Some("2026-03-03T12:00:00Z".into()),
        };
        let dbg = format!("{msg:?}");
        assert!(dbg.contains("hi bot"));
    }
}
