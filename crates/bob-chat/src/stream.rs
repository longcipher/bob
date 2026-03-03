//! Streaming text types for progressive message delivery.

use std::pin::Pin;

use futures_core::Stream;
use tokio_stream::StreamExt as _;

use crate::{
    adapter::ChatAdapter,
    error::ChatError,
    message::{AdapterPostableMessage, SentMessage},
};

/// A stream of text chunks from an async source (e.g. an LLM response).
pub type TextStream = Pin<Box<dyn Stream<Item = String> + Send>>;

/// Options that control how streamed text is posted and updated.
#[derive(Debug, Clone)]
pub struct StreamOptions {
    /// Minimum interval (in milliseconds) between edit calls while
    /// streaming accumulated text to a thread.
    pub update_interval_ms: u64,
    /// Placeholder text shown in the initial message before any chunks
    /// arrive.  `None` means no placeholder is sent until the first
    /// chunk is received.
    pub placeholder_text: Option<String>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self { update_interval_ms: 500, placeholder_text: Some("...".into()) }
    }
}

/// Stream text progressively using a post-then-edit loop.
///
/// This is the default fallback streaming strategy for adapters that do
/// not provide a native streaming mechanism:
///
/// 1. Post an initial placeholder message.
/// 2. Consume chunks from `text_stream`, accumulating them.
/// 3. Every `options.update_interval_ms` milliseconds, edit the message with the accumulated text
///    so far.
/// 4. After the stream ends, perform a final edit with the complete text.
pub async fn fallback_stream<A: ChatAdapter + ?Sized>(
    adapter: &A,
    thread_id: &str,
    text_stream: TextStream,
    options: &StreamOptions,
) -> Result<SentMessage, ChatError> {
    let placeholder = options.placeholder_text.clone().unwrap_or_else(|| String::from("\u{200B}")); // zero-width space

    let initial =
        adapter.post_message(thread_id, &AdapterPostableMessage::Text(placeholder)).await?;

    let message_id = initial.id.clone();
    let mut accumulated = String::new();
    let interval = tokio::time::Duration::from_millis(options.update_interval_ms);
    let mut last_edit = tokio::time::Instant::now();

    let mut stream = text_stream;

    while let Some(chunk) = stream.next().await {
        accumulated.push_str(&chunk);

        if last_edit.elapsed() >= interval {
            let _interim = adapter
                .edit_message(
                    thread_id,
                    &message_id,
                    &AdapterPostableMessage::Text(accumulated.clone()),
                )
                .await?;
            last_edit = tokio::time::Instant::now();
        }
    }

    // Always perform a final edit so the message contains the complete text.
    let final_sent = adapter
        .edit_message(thread_id, &message_id, &AdapterPostableMessage::Text(accumulated))
        .await?;

    Ok(final_sent)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{card::CardElement, event::ChatEvent};

    // -----------------------------------------------------------------
    // Mock adapter that records post/edit calls
    // -----------------------------------------------------------------

    #[derive(Debug, Clone)]
    #[expect(dead_code, reason = "fields read in test assertions via pattern matching")]
    enum Call {
        Post(String),
        Edit { message_id: String, text: String },
    }

    struct MockStreamAdapter {
        calls: Arc<Mutex<Vec<Call>>>,
        next_id: Arc<Mutex<u64>>,
    }

    impl MockStreamAdapter {
        fn new() -> Self {
            Self { calls: Arc::new(Mutex::new(Vec::new())), next_id: Arc::new(Mutex::new(0)) }
        }

        fn take_calls(&self) -> Vec<Call> {
            let Ok(mut guard) = self.calls.lock() else {
                return Vec::new();
            };
            std::mem::take(&mut *guard)
        }
    }

    #[async_trait::async_trait]
    impl ChatAdapter for MockStreamAdapter {
        fn name(&self) -> &'static str {
            "mock-stream"
        }

        async fn post_message(
            &self,
            _thread_id: &str,
            message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            let text = match message {
                AdapterPostableMessage::Text(t) | AdapterPostableMessage::Markdown(t) => t.clone(),
            };
            let id = {
                let Ok(mut id) = self.next_id.lock() else {
                    return Err(ChatError::Adapter("lock poisoned".into()));
                };
                *id += 1;
                format!("msg-{id}")
            };
            {
                let Ok(mut calls) = self.calls.lock() else {
                    return Err(ChatError::Adapter("lock poisoned".into()));
                };
                calls.push(Call::Post(text));
            }
            Ok(SentMessage {
                id,
                thread_id: "t1".into(),
                adapter_name: "mock-stream".into(),
                raw: None,
            })
        }

        async fn edit_message(
            &self,
            _thread_id: &str,
            message_id: &str,
            message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            let text = match message {
                AdapterPostableMessage::Text(t) | AdapterPostableMessage::Markdown(t) => t.clone(),
            };
            {
                let Ok(mut calls) = self.calls.lock() else {
                    return Err(ChatError::Adapter("lock poisoned".into()));
                };
                calls.push(Call::Edit { message_id: message_id.to_owned(), text });
            }
            Ok(SentMessage {
                id: message_id.to_owned(),
                thread_id: "t1".into(),
                adapter_name: "mock-stream".into(),
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

        fn render_message(&self, _msg: &AdapterPostableMessage) -> String {
            String::new()
        }

        async fn recv_event(&mut self) -> Option<ChatEvent> {
            None
        }
    }

    // -----------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------

    #[test]
    fn default_stream_options() {
        let opts = StreamOptions::default();
        assert_eq!(opts.update_interval_ms, 500);
        assert_eq!(opts.placeholder_text.as_deref(), Some("..."));
    }

    #[tokio::test]
    async fn fallback_stream_posts_then_edits() {
        tokio::time::pause();

        let adapter = MockStreamAdapter::new();
        let chunks = vec!["Hello".to_owned(), " ".into(), "world".into()];
        let stream: TextStream = Box::pin(tokio_stream::iter(chunks));

        let options =
            StreamOptions { update_interval_ms: 200, placeholder_text: Some("...".into()) };

        let result = fallback_stream(&adapter, "t1", stream, &options).await;
        assert!(result.is_ok());

        let calls = adapter.take_calls();

        // First call must be a Post (the placeholder).
        assert!(matches!(&calls[0], Call::Post(t) if t == "..."));

        // Last call must be an Edit containing the full text.
        let last = calls.last();
        assert!(matches!(last, Some(Call::Edit { text, .. }) if text == "Hello world"));
    }

    #[tokio::test]
    async fn fallback_stream_intermediate_edits_with_time_advance() {
        tokio::time::pause();

        let adapter = MockStreamAdapter::new();

        // Build a stream that yields chunks with delays between them so
        // the interval-based logic triggers intermediate edits.
        let stream: TextStream = Box::pin(async_stream::stream! {
            yield "A".to_owned();
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            yield "B".to_owned();
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            yield "C".to_owned();
        });

        let options =
            StreamOptions { update_interval_ms: 200, placeholder_text: Some("...".into()) };

        let result = fallback_stream(&adapter, "t1", stream, &options).await;
        assert!(result.is_ok());

        let calls = adapter.take_calls();

        // Expect: Post("..."), Edit("AB") [after 300ms ≥ 200ms interval],
        //         Edit("ABC") [after another 300ms], final Edit("ABC").
        // At minimum we should see 1 post + ≥2 edits.
        let post_count = calls.iter().filter(|c| matches!(c, Call::Post(_))).count();
        let edit_count = calls.iter().filter(|c| matches!(c, Call::Edit { .. })).count();

        assert_eq!(post_count, 1, "exactly one post_message call");
        assert!(
            edit_count >= 2,
            "at least two edit calls (intermediate + final), got {edit_count}"
        );

        // Final edit must contain the full accumulated text.
        let last = calls.last();
        assert!(matches!(last, Some(Call::Edit { text, .. }) if text == "ABC"));
    }

    #[tokio::test]
    async fn fallback_stream_empty_stream_still_edits() {
        tokio::time::pause();

        let adapter = MockStreamAdapter::new();
        let stream: TextStream = Box::pin(tokio_stream::iter(Vec::<String>::new()));

        let options = StreamOptions::default();

        let result = fallback_stream(&adapter, "t1", stream, &options).await;
        assert!(result.is_ok());

        let calls = adapter.take_calls();

        // 1 post + 1 final edit (with empty text).
        assert_eq!(calls.len(), 2);
        assert!(matches!(&calls[0], Call::Post(_)));
        assert!(matches!(&calls[1], Call::Edit { text, .. } if text.is_empty()));
    }
}
