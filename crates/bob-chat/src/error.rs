//! Chat-specific error types.

/// Errors that can occur in the chat channel layer.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// An adapter-level failure (e.g. transport or protocol mismatch).
    #[error("adapter error: {0}")]
    Adapter(String),

    /// The requested feature is not supported by the current adapter.
    #[error("feature not supported: {0}")]
    NotSupported(String),

    /// A message with the given identifier was not found.
    #[error("message not found: {0}")]
    MessageNotFound(String),

    /// Sending a message failed.
    #[error("send failed: {0}")]
    SendFailed(String),

    /// A modal interaction error.
    #[error("modal error: {0}")]
    Modal(String),

    /// The underlying channel has been closed.
    #[error("channel closed")]
    Closed,

    /// An opaque internal error from a lower layer.
    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    #[test]
    fn display_adapter() {
        let err = ChatError::Adapter("timeout".into());
        assert_eq!(err.to_string(), "adapter error: timeout");
    }

    #[test]
    fn display_not_supported() {
        let err = ChatError::NotSupported("reactions".into());
        assert_eq!(err.to_string(), "feature not supported: reactions");
    }

    #[test]
    fn display_message_not_found() {
        let err = ChatError::MessageNotFound("msg-42".into());
        assert_eq!(err.to_string(), "message not found: msg-42");
    }

    #[test]
    fn display_send_failed() {
        let err = ChatError::SendFailed("queue full".into());
        assert_eq!(err.to_string(), "send failed: queue full");
    }

    #[test]
    fn display_modal() {
        let err = ChatError::Modal("missing field".into());
        assert_eq!(err.to_string(), "modal error: missing field");
    }

    #[test]
    fn display_closed() {
        let err = ChatError::Closed;
        assert_eq!(err.to_string(), "channel closed");
    }

    #[test]
    fn display_internal_transparent() {
        let inner: Box<dyn std::error::Error + Send + Sync> = "boom".into();
        let err = ChatError::Internal(inner);
        assert_eq!(err.to_string(), "boom");
    }

    #[test]
    fn from_boxed_error() {
        let inner: Box<dyn std::error::Error + Send + Sync> = "something broke".into();
        let err: ChatError = inner.into();
        let mut buf = String::new();
        write!(buf, "{err}").expect("write failed");
        assert_eq!(buf, "something broke");
    }
}
