//! Thread handle for interacting with a conversation thread.
//!
//! [`ThreadHandle`] provides a scoped reference to a specific thread within a
//! chat adapter.  Handler functions receive a `ThreadHandle` so they can post
//! replies, manage subscriptions, and stream responses back to the originating
//! thread.

use std::sync::Arc;

use crate::{
    adapter::ChatAdapter,
    error::ChatError,
    message::{AdapterPostableMessage, EphemeralMessage, PostableMessage, SentMessage},
};

/// A scoped handle to a conversation thread.
///
/// Holds a reference to the originating adapter, the thread identifier, and the
/// shared subscription map so handlers can interact with the thread without
/// needing access to the full [`ChatBot`](crate::bot::ChatBot).
pub struct ThreadHandle {
    /// Platform-specific thread/channel identifier.
    pub(crate) thread_id: String,
    /// The adapter that owns this thread.
    pub(crate) adapter: Arc<dyn ChatAdapter>,
    /// Shared subscription map (thread_id → ()).
    pub(crate) subscriptions: Arc<scc::HashMap<String, ()>>,
}

impl ThreadHandle {
    /// Return the thread identifier.
    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Return the name of the adapter backing this handle.
    #[must_use]
    pub fn adapter_name(&self) -> &str {
        self.adapter.name()
    }

    /// Post a message to this thread.
    ///
    /// For `PostableMessage::Text` and `PostableMessage::Markdown`, the message
    /// is forwarded to the adapter via [`ChatAdapter::post_message`].
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter fails to post.
    pub async fn post(
        &self,
        message: impl Into<PostableMessage>,
    ) -> Result<SentMessage, ChatError> {
        let postable = message.into();
        let adapter_msg = match postable {
            PostableMessage::Text(t) => AdapterPostableMessage::Text(t),
            PostableMessage::Markdown(m) => AdapterPostableMessage::Markdown(m),
        };
        self.adapter.post_message(&self.thread_id, &adapter_msg).await
    }

    /// Post an ephemeral message visible only to the specified user.
    ///
    /// If the adapter does not support ephemeral messages and
    /// `fallback_to_dm` is `true`, falls back to opening a direct-message
    /// channel with the user and posting there.
    ///
    /// # Errors
    ///
    /// Returns an error if both the ephemeral post and DM fallback fail.
    pub async fn post_ephemeral(
        &self,
        user_id: &str,
        message: impl Into<AdapterPostableMessage>,
        fallback_to_dm: bool,
    ) -> Result<Option<EphemeralMessage>, ChatError> {
        let msg = message.into();
        match self.adapter.post_ephemeral(&self.thread_id, user_id, &msg).await {
            Ok(eph) => Ok(Some(eph)),
            Err(ChatError::NotSupported(_)) if fallback_to_dm => {
                let dm_thread = self.adapter.open_dm(user_id).await?;
                self.adapter.post_message(&dm_thread, &msg).await?;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Show a typing / status indicator in this thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter fails.
    pub async fn start_typing(&self, status: Option<&str>) -> Result<(), ChatError> {
        self.adapter.start_typing(&self.thread_id, status).await
    }

    /// Subscribe to follow-up messages in this thread.
    ///
    /// Subsequent `ChatEvent::Message` events for this thread will be
    /// routed to `on_subscribed_message` handlers instead of `on_message`.
    pub async fn subscribe(&self) {
        let _ = self.subscriptions.insert_async(self.thread_id.clone(), ()).await;
    }

    /// Unsubscribe from this thread.
    pub async fn unsubscribe(&self) {
        let _ = self.subscriptions.remove_async(&self.thread_id).await;
    }

    /// Format a platform-agnostic mention string for a user.
    ///
    /// This is a simple helper that returns `<@user_id>`.  Adapters that
    /// need a different format should handle it at the rendering layer.
    #[must_use]
    pub fn mention_user(&self, user_id: &str) -> String {
        format!("<@{user_id}>")
    }
}

impl std::fmt::Debug for ThreadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadHandle")
            .field("thread_id", &self.thread_id)
            .field("adapter", &self.adapter.name())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{card::CardElement, event::ChatEvent};

    // -----------------------------------------------------------------
    // Mock adapter for thread handle tests
    // -----------------------------------------------------------------

    struct MockThreadAdapter {
        post_count: Arc<AtomicUsize>,
        edit_count: Arc<AtomicUsize>,
        ephemeral_supported: bool,
        dm_opened: Arc<AtomicUsize>,
    }

    impl MockThreadAdapter {
        fn new(ephemeral_supported: bool) -> Self {
            Self {
                post_count: Arc::new(AtomicUsize::new(0)),
                edit_count: Arc::new(AtomicUsize::new(0)),
                ephemeral_supported,
                dm_opened: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl ChatAdapter for MockThreadAdapter {
        fn name(&self) -> &str {
            "mock-thread"
        }

        async fn post_message(
            &self,
            _thread_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            self.post_count.fetch_add(1, Ordering::SeqCst);
            Ok(SentMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                adapter_name: "mock-thread".into(),
                raw: None,
            })
        }

        async fn edit_message(
            &self,
            _thread_id: &str,
            _message_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            self.edit_count.fetch_add(1, Ordering::SeqCst);
            Ok(SentMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                adapter_name: "mock-thread".into(),
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

        async fn post_ephemeral(
            &self,
            _thread_id: &str,
            _user_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<EphemeralMessage, ChatError> {
            if self.ephemeral_supported {
                Ok(EphemeralMessage {
                    id: "e1".into(),
                    thread_id: "t1".into(),
                    used_fallback: false,
                })
            } else {
                Err(ChatError::NotSupported("ephemeral messages".into()))
            }
        }

        async fn open_dm(&self, _user_id: &str) -> Result<String, ChatError> {
            self.dm_opened.fetch_add(1, Ordering::SeqCst);
            Ok("dm-thread".into())
        }
    }

    fn make_handle(adapter: MockThreadAdapter) -> ThreadHandle {
        ThreadHandle {
            thread_id: "t1".into(),
            adapter: Arc::new(adapter),
            subscriptions: Arc::new(scc::HashMap::new()),
        }
    }

    // -----------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------

    #[test]
    fn thread_id_accessor() {
        let handle = make_handle(MockThreadAdapter::new(true));
        assert_eq!(handle.thread_id(), "t1");
    }

    #[test]
    fn adapter_name_accessor() {
        let handle = make_handle(MockThreadAdapter::new(true));
        assert_eq!(handle.adapter_name(), "mock-thread");
    }

    #[tokio::test]
    async fn post_text_message() {
        let adapter = MockThreadAdapter::new(true);
        let post_count = Arc::clone(&adapter.post_count);
        let handle = make_handle(adapter);

        let result = handle.post(PostableMessage::Text("hello".into())).await;
        assert!(result.is_ok());
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_markdown_message() {
        let adapter = MockThreadAdapter::new(true);
        let post_count = Arc::clone(&adapter.post_count);
        let handle = make_handle(adapter);

        let result = handle.post(PostableMessage::Markdown("**bold**".into())).await;
        assert!(result.is_ok());
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_ephemeral_supported() {
        let handle = make_handle(MockThreadAdapter::new(true));
        let result =
            handle.post_ephemeral("u1", AdapterPostableMessage::Text("secret".into()), false).await;
        assert!(result.is_ok());
        let eph = result.expect("ephemeral msg");
        assert!(eph.is_some());
    }

    #[tokio::test]
    async fn post_ephemeral_fallback_to_dm() {
        let adapter = MockThreadAdapter::new(false);
        let dm_count = Arc::clone(&adapter.dm_opened);
        let post_count = Arc::clone(&adapter.post_count);
        let handle = make_handle(adapter);

        let result =
            handle.post_ephemeral("u1", AdapterPostableMessage::Text("secret".into()), true).await;
        assert!(result.is_ok());
        // Should have opened a DM and posted there
        assert_eq!(dm_count.load(Ordering::SeqCst), 1);
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
        // Returns None when fallback was used
        assert!(result.expect("should be Ok").is_none());
    }

    #[tokio::test]
    async fn post_ephemeral_no_fallback_returns_error() {
        let handle = make_handle(MockThreadAdapter::new(false));
        let result =
            handle.post_ephemeral("u1", AdapterPostableMessage::Text("secret".into()), false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe() {
        let handle = make_handle(MockThreadAdapter::new(true));

        assert!(!handle.subscriptions.contains_sync("t1"));
        handle.subscribe().await;
        assert!(handle.subscriptions.contains_sync("t1"));
        handle.unsubscribe().await;
        assert!(!handle.subscriptions.contains_sync("t1"));
    }

    #[test]
    fn mention_user_formatting() {
        let handle = make_handle(MockThreadAdapter::new(true));
        assert_eq!(handle.mention_user("U123"), "<@U123>");
    }

    #[test]
    fn debug_impl() {
        let handle = make_handle(MockThreadAdapter::new(true));
        let dbg = format!("{handle:?}");
        assert!(dbg.contains("t1"));
        assert!(dbg.contains("mock-thread"));
    }
}
