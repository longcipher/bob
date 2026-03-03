//! Test utilities for the `bob-chat` crate.
//!
//! Provides [`MockChatAdapter`], a configurable mock adapter that records
//! method calls and can replay a pre-loaded event queue.

#![allow(dead_code)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    adapter::ChatAdapter,
    card::CardElement,
    emoji::EmojiValue,
    error::ChatError,
    event::ChatEvent,
    file::FileUpload,
    message::{AdapterPostableMessage, EphemeralMessage, SentMessage},
    modal::ModalElement,
};

// ---------------------------------------------------------------------------
// Recorded call types
// ---------------------------------------------------------------------------

/// A recorded method call on the mock adapter.
#[derive(Debug, Clone)]
pub(crate) enum MockCall {
    /// `post_message` was called.
    PostMessage { thread_id: String, text: String },
    /// `edit_message` was called.
    EditMessage { thread_id: String, message_id: String, text: String },
    /// `delete_message` was called.
    DeleteMessage { thread_id: String, message_id: String },
    /// `add_reaction` was called.
    AddReaction { thread_id: String, message_id: String, emoji_name: String },
    /// `remove_reaction` was called.
    RemoveReaction { thread_id: String, message_id: String, emoji_name: String },
    /// `open_dm` was called.
    OpenDm { user_id: String },
    /// `post_ephemeral` was called.
    PostEphemeral { thread_id: String, user_id: String, text: String },
    /// `open_modal` was called.
    OpenModal { trigger_id: String, callback_id: String },
    /// `start_typing` was called.
    StartTyping { thread_id: String, status: Option<String> },
    /// `upload_file` was called.
    UploadFile { thread_id: String, filename: String },
}

// ---------------------------------------------------------------------------
// MockChatAdapter
// ---------------------------------------------------------------------------

/// A mock adapter for testing [`ChatBot`](crate::bot::ChatBot) event dispatch
/// and handler behavior.
///
/// # Features
///
/// - **Event injection:** Pre-load events via [`inject_event`](Self::inject_event) or the
///   constructor.
/// - **Call recording:** Every adapter method call is recorded and can be inspected via
///   [`calls`](Self::calls), [`posted_messages`](Self::posted_messages), etc.
/// - **Configurable behavior:** `support_ephemeral` and `support_dm` flags control whether
///   ephemeral and DM features are available.
pub(crate) struct MockChatAdapter {
    name: String,
    events: Mutex<VecDeque<ChatEvent>>,
    recorded: Arc<Mutex<Vec<MockCall>>>,
    next_id: Mutex<u64>,
    /// When `false`, `post_ephemeral` returns `NotSupported`.
    pub support_ephemeral: bool,
    /// When `false`, `open_dm` returns `NotSupported`.
    pub support_dm: bool,
}

impl MockChatAdapter {
    /// Create a new mock adapter with the given name and pre-loaded events.
    pub(crate) fn new(name: impl Into<String>, events: Vec<ChatEvent>) -> Self {
        Self {
            name: name.into(),
            events: Mutex::new(VecDeque::from(events)),
            recorded: Arc::new(Mutex::new(Vec::new())),
            next_id: Mutex::new(0),
            support_ephemeral: true,
            support_dm: true,
        }
    }

    /// Inject an additional event into the adapter's queue.
    pub(crate) fn inject_event(&self, event: ChatEvent) {
        if let Ok(mut q) = self.events.lock() {
            q.push_back(event);
        }
    }

    /// Return a clone of the recorded-calls `Arc` for assertions.
    #[must_use]
    pub(crate) fn recorded_handle(&self) -> Arc<Mutex<Vec<MockCall>>> {
        Arc::clone(&self.recorded)
    }

    /// Snapshot of all recorded calls.
    #[must_use]
    pub(crate) fn calls(&self) -> Vec<MockCall> {
        self.recorded.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Filter recorded calls to only `PostMessage` entries.
    #[must_use]
    pub(crate) fn posted_messages(&self) -> Vec<MockCall> {
        self.calls().into_iter().filter(|c| matches!(c, MockCall::PostMessage { .. })).collect()
    }

    /// Filter recorded calls to only `EditMessage` entries.
    #[must_use]
    pub(crate) fn edited_messages(&self) -> Vec<MockCall> {
        self.calls().into_iter().filter(|c| matches!(c, MockCall::EditMessage { .. })).collect()
    }

    /// Filter recorded calls to only `DeleteMessage` entries.
    #[must_use]
    pub(crate) fn deleted_messages(&self) -> Vec<MockCall> {
        self.calls().into_iter().filter(|c| matches!(c, MockCall::DeleteMessage { .. })).collect()
    }

    fn record(&self, call: MockCall) {
        if let Ok(mut r) = self.recorded.lock() {
            r.push(call);
        }
    }

    fn next_msg_id(&self) -> String {
        let Ok(mut id) = self.next_id.lock() else {
            return "msg-err".into();
        };
        *id += 1;
        format!("msg-{id}")
    }

    fn extract_text(msg: &AdapterPostableMessage) -> String {
        match msg {
            AdapterPostableMessage::Text(t) | AdapterPostableMessage::Markdown(t) => t.clone(),
        }
    }
}

impl std::fmt::Debug for MockChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockChatAdapter")
            .field("name", &self.name)
            .field("pending_events", &self.events.lock().map(|q| q.len()).unwrap_or(0))
            .field("recorded_calls", &self.recorded.lock().map(|r| r.len()).unwrap_or(0))
            .finish()
    }
}

#[async_trait::async_trait]
impl ChatAdapter for MockChatAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    async fn post_message(
        &self,
        thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let text = Self::extract_text(message);
        self.record(MockCall::PostMessage { thread_id: thread_id.into(), text });
        Ok(SentMessage {
            id: self.next_msg_id(),
            thread_id: thread_id.into(),
            adapter_name: self.name.clone(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let text = Self::extract_text(message);
        self.record(MockCall::EditMessage {
            thread_id: thread_id.into(),
            message_id: message_id.into(),
            text,
        });
        Ok(SentMessage {
            id: message_id.into(),
            thread_id: thread_id.into(),
            adapter_name: self.name.clone(),
            raw: None,
        })
    }

    async fn delete_message(&self, thread_id: &str, message_id: &str) -> Result<(), ChatError> {
        self.record(MockCall::DeleteMessage {
            thread_id: thread_id.into(),
            message_id: message_id.into(),
        });
        Ok(())
    }

    fn render_card(&self, card: &CardElement) -> String {
        crate::card::render_card_as_text(card)
    }

    fn render_message(&self, message: &AdapterPostableMessage) -> String {
        Self::extract_text(message)
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        let Ok(mut q) = self.events.lock() else {
            return None;
        };
        q.pop_front()
    }

    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        self.record(MockCall::AddReaction {
            thread_id: thread_id.into(),
            message_id: message_id.into(),
            emoji_name: emoji.to_string(),
        });
        Ok(())
    }

    async fn remove_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        self.record(MockCall::RemoveReaction {
            thread_id: thread_id.into(),
            message_id: message_id.into(),
            emoji_name: emoji.to_string(),
        });
        Ok(())
    }

    async fn open_dm(&self, user_id: &str) -> Result<String, ChatError> {
        if !self.support_dm {
            return Err(ChatError::NotSupported("direct messages".into()));
        }
        self.record(MockCall::OpenDm { user_id: user_id.into() });
        Ok(format!("dm-{user_id}"))
    }

    async fn post_ephemeral(
        &self,
        thread_id: &str,
        user_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<EphemeralMessage, ChatError> {
        if !self.support_ephemeral {
            return Err(ChatError::NotSupported("ephemeral messages".into()));
        }
        let text = Self::extract_text(message);
        self.record(MockCall::PostEphemeral {
            thread_id: thread_id.into(),
            user_id: user_id.into(),
            text,
        });
        Ok(EphemeralMessage {
            id: self.next_msg_id(),
            thread_id: thread_id.into(),
            used_fallback: false,
        })
    }

    async fn open_modal(
        &self,
        trigger_id: &str,
        modal: &ModalElement,
        _context_id: Option<&str>,
    ) -> Result<String, ChatError> {
        self.record(MockCall::OpenModal {
            trigger_id: trigger_id.into(),
            callback_id: modal.callback_id.clone(),
        });
        Ok(format!("view-{}", modal.callback_id))
    }

    async fn start_typing(&self, thread_id: &str, status: Option<&str>) -> Result<(), ChatError> {
        self.record(MockCall::StartTyping {
            thread_id: thread_id.into(),
            status: status.map(String::from),
        });
        Ok(())
    }

    async fn upload_file(
        &self,
        thread_id: &str,
        file: &FileUpload,
    ) -> Result<SentMessage, ChatError> {
        self.record(MockCall::UploadFile {
            thread_id: thread_id.into(),
            filename: file.filename.clone(),
        });
        Ok(SentMessage {
            id: self.next_msg_id(),
            thread_id: thread_id.into(),
            adapter_name: self.name.clone(),
            raw: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Author;

    fn sample_author() -> Author {
        Author {
            user_id: "u1".into(),
            user_name: "alice".into(),
            full_name: "Alice".into(),
            is_bot: false,
        }
    }

    #[tokio::test]
    async fn mock_adapter_post_records_call() {
        let adapter = MockChatAdapter::new("test", vec![]);
        adapter
            .post_message("t1", &AdapterPostableMessage::Text("hello".into()))
            .await
            .expect("post should succeed");

        let calls = adapter.posted_messages();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], MockCall::PostMessage { text, .. } if text == "hello"));
    }

    #[tokio::test]
    async fn mock_adapter_edit_records_call() {
        let adapter = MockChatAdapter::new("test", vec![]);
        adapter
            .edit_message("t1", "m1", &AdapterPostableMessage::Text("edited".into()))
            .await
            .expect("edit should succeed");

        let calls = adapter.edited_messages();
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn mock_adapter_delete_records_call() {
        let adapter = MockChatAdapter::new("test", vec![]);
        adapter.delete_message("t1", "m1").await.expect("delete should succeed");

        let calls = adapter.deleted_messages();
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn mock_adapter_event_queue() {
        let msg = crate::message::IncomingMessage {
            id: "m1".into(),
            text: "hello".into(),
            author: sample_author(),
            attachments: vec![],
            is_mention: false,
            thread_id: "t1".into(),
            timestamp: None,
        };
        let mut adapter = MockChatAdapter::new(
            "test",
            vec![ChatEvent::Message { thread_id: "t1".into(), message: msg }],
        );

        let event = adapter.recv_event().await;
        assert!(event.is_some());

        let event = adapter.recv_event().await;
        assert!(event.is_none());
    }

    #[tokio::test]
    async fn mock_adapter_inject_event() {
        let mut adapter = MockChatAdapter::new("test", vec![]);
        assert!(adapter.recv_event().await.is_none());

        let msg = crate::message::IncomingMessage {
            id: "m2".into(),
            text: "injected".into(),
            author: sample_author(),
            attachments: vec![],
            is_mention: false,
            thread_id: "t2".into(),
            timestamp: None,
        };
        adapter.inject_event(ChatEvent::Message { thread_id: "t2".into(), message: msg });

        let event = adapter.recv_event().await;
        assert!(event.is_some());
    }

    #[tokio::test]
    async fn mock_adapter_ephemeral_support_toggle() {
        let adapter = MockChatAdapter::new("test", vec![]);
        let result =
            adapter.post_ephemeral("t1", "u1", &AdapterPostableMessage::Text("hi".into())).await;
        assert!(result.is_ok());

        let mut adapter2 = MockChatAdapter::new("test2", vec![]);
        adapter2.support_ephemeral = false;
        let result =
            adapter2.post_ephemeral("t1", "u1", &AdapterPostableMessage::Text("hi".into())).await;
        assert!(matches!(result, Err(ChatError::NotSupported(_))));
    }

    #[test]
    fn mock_adapter_debug() {
        let adapter = MockChatAdapter::new("test-debug", vec![]);
        let dbg = format!("{adapter:?}");
        assert!(dbg.contains("test-debug"));
    }
}
