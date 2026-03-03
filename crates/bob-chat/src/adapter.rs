//! The core [`ChatAdapter`] trait that each chat platform implements.

use crate::{
    card::CardElement,
    emoji::EmojiValue,
    error::ChatError,
    event::ChatEvent,
    file::FileUpload,
    message::{AdapterPostableMessage, EphemeralMessage, SentMessage},
    modal::ModalElement,
    stream::{StreamOptions, TextStream},
};

/// Extension point for chat platforms (Slack, Discord, CLI, etc.).
///
/// Each platform provides a concrete implementation.  Required methods
/// must be implemented; optional methods have sensible defaults that
/// return [`ChatError::NotSupported`].
///
/// # Object safety
///
/// The trait is object-safe and can be used as `Box<dyn ChatAdapter>`.
/// Note that [`recv_event`](Self::recv_event) takes `&mut self`, so a
/// shared reference (`&dyn ChatAdapter`) is insufficient for receiving
/// events.
#[async_trait::async_trait]
pub trait ChatAdapter: Send + Sync {
    // -----------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------

    /// Short, unique name of this adapter (e.g. `"slack"`, `"discord"`).
    fn name(&self) -> &str;

    // -----------------------------------------------------------------
    // Required – message lifecycle
    // -----------------------------------------------------------------

    /// Post a new message to the given thread.
    async fn post_message(
        &self,
        thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError>;

    /// Edit an existing message in the given thread.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError>;

    /// Delete a message from the given thread.
    async fn delete_message(&self, thread_id: &str, message_id: &str) -> Result<(), ChatError>;

    // -----------------------------------------------------------------
    // Required – rendering
    // -----------------------------------------------------------------

    /// Render a [`CardElement`] into the adapter's native format string.
    fn render_card(&self, card: &CardElement) -> String;

    /// Render an [`AdapterPostableMessage`] into a plain-text or
    /// markup representation suitable for the adapter.
    fn render_message(&self, message: &AdapterPostableMessage) -> String;

    // -----------------------------------------------------------------
    // Required – event reception
    // -----------------------------------------------------------------

    /// Wait for the next inbound [`ChatEvent`].
    ///
    /// Returns `None` when the event source is exhausted.
    async fn recv_event(&mut self) -> Option<ChatEvent>;

    // -----------------------------------------------------------------
    // Optional – streaming
    // -----------------------------------------------------------------

    /// Stream text progressively to a thread.
    ///
    /// The default implementation uses [`fallback_stream`](crate::stream::fallback_stream)
    /// which posts a placeholder and then repeatedly edits it as chunks
    /// arrive from the stream.
    async fn stream(
        &self,
        thread_id: &str,
        text_stream: TextStream,
        options: &StreamOptions,
    ) -> Result<SentMessage, ChatError> {
        crate::stream::fallback_stream(self, thread_id, text_stream, options).await
    }

    // -----------------------------------------------------------------
    // Optional – reactions
    // -----------------------------------------------------------------

    /// Add a reaction emoji to a message.
    async fn add_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("reactions".into()))
    }

    /// Remove a reaction emoji from a message.
    async fn remove_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("reactions".into()))
    }

    // -----------------------------------------------------------------
    // Optional – direct messages
    // -----------------------------------------------------------------

    /// Open (or return an existing) direct-message channel with a user.
    ///
    /// Returns the thread/channel ID of the DM.
    async fn open_dm(&self, _user_id: &str) -> Result<String, ChatError> {
        Err(ChatError::NotSupported("direct messages".into()))
    }

    // -----------------------------------------------------------------
    // Optional – ephemeral messages
    // -----------------------------------------------------------------

    /// Post a message visible only to the specified user.
    async fn post_ephemeral(
        &self,
        _thread_id: &str,
        _user_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<EphemeralMessage, ChatError> {
        Err(ChatError::NotSupported("ephemeral messages".into()))
    }

    // -----------------------------------------------------------------
    // Optional – modals
    // -----------------------------------------------------------------

    /// Open a modal dialog tied to the given trigger interaction.
    async fn open_modal(
        &self,
        _trigger_id: &str,
        _modal: &ModalElement,
        _context_id: Option<&str>,
    ) -> Result<String, ChatError> {
        Err(ChatError::NotSupported("modals".into()))
    }

    // -----------------------------------------------------------------
    // Optional – typing indicators
    // -----------------------------------------------------------------

    /// Show a typing / status indicator in the given thread.
    async fn start_typing(&self, _thread_id: &str, _status: Option<&str>) -> Result<(), ChatError> {
        Ok(())
    }

    // -----------------------------------------------------------------
    // Optional – file uploads
    // -----------------------------------------------------------------

    /// Upload a file to the given thread.
    async fn upload_file(
        &self,
        _thread_id: &str,
        _file: &FileUpload,
    ) -> Result<SentMessage, ChatError> {
        Err(ChatError::NotSupported("file uploads".into()))
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ChatEvent;

    /// Minimal adapter that implements only required methods.
    /// Verifies the trait is implementable and object-safe.
    struct StubAdapter;

    #[async_trait::async_trait]
    impl ChatAdapter for StubAdapter {
        fn name(&self) -> &str {
            "stub"
        }

        async fn post_message(
            &self,
            _thread_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            Ok(SentMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                adapter_name: "stub".into(),
                raw: None,
            })
        }

        async fn edit_message(
            &self,
            _thread_id: &str,
            _message_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            Ok(SentMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                adapter_name: "stub".into(),
                raw: None,
            })
        }

        async fn delete_message(
            &self,
            _thread_id: &str,
            _message_id: &str,
        ) -> Result<(), ChatError> {
            Ok(())
        }

        fn render_card(&self, _card: &CardElement) -> String {
            String::new()
        }

        fn render_message(&self, _message: &AdapterPostableMessage) -> String {
            String::new()
        }

        async fn recv_event(&mut self) -> Option<ChatEvent> {
            None
        }
    }

    /// `Box<dyn ChatAdapter>` compiles – trait is object-safe.
    #[test]
    fn object_safe() {
        let _adapter: Box<dyn ChatAdapter> = Box::new(StubAdapter);
    }

    /// Default optional methods return the expected errors / values.
    #[tokio::test]
    async fn default_optional_methods() {
        let adapter = StubAdapter;

        let emoji = EmojiValue::from_well_known(crate::emoji::WellKnownEmoji::ThumbsUp);

        let err = adapter.add_reaction("t1", "m1", &emoji).await.unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));

        let err = adapter.remove_reaction("t1", "m1", &emoji).await.unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));

        let err = adapter.open_dm("u1").await.unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));

        let err = adapter
            .post_ephemeral("t1", "u1", &AdapterPostableMessage::Text("hi".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));

        let err = adapter
            .open_modal(
                "trigger",
                &ModalElement {
                    callback_id: "cb".into(),
                    title: "test".into(),
                    submit_label: None,
                    children: vec![],
                    private_metadata: None,
                    notify_on_close: false,
                },
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));

        // start_typing defaults to Ok
        adapter.start_typing("t1", None).await.expect("start_typing should default to Ok");

        let err = adapter
            .upload_file(
                "t1",
                &crate::file::FileUpload {
                    filename: "f.txt".into(),
                    mime_type: None,
                    data: bytes::Bytes::new(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ChatError::NotSupported(_)));
    }

    /// Required methods work on the stub.
    #[tokio::test]
    async fn stub_required_methods() {
        let mut adapter = StubAdapter;

        let sent = adapter
            .post_message("t1", &AdapterPostableMessage::Text("hello".into()))
            .await
            .expect("post_message");
        assert_eq!(sent.id, "m1");

        let sent = adapter
            .edit_message("t1", "m1", &AdapterPostableMessage::Text("edited".into()))
            .await
            .expect("edit_message");
        assert_eq!(sent.id, "m1");

        adapter.delete_message("t1", "m1").await.expect("delete_message");

        assert_eq!(adapter.name(), "stub");
        assert!(
            adapter
                .render_card(&CardElement { title: None, children: vec![], fallback_text: None })
                .is_empty()
        );
        assert!(adapter.render_message(&AdapterPostableMessage::Text("x".into())).is_empty());

        let event = adapter.recv_event().await;
        assert!(event.is_none());
    }
}
