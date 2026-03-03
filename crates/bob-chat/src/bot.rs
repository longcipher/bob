//! Central orchestrator for chat event handling.
//!
//! [`ChatBot`] is the top-level coordinator.  Adapters are registered on it,
//! and event handlers are added via type-safe builder methods.  When
//! [`ChatBot::run`] is called, incoming events from all adapters are polled
//! concurrently and dispatched to the matching handlers.

use std::{future::Future, pin::Pin, sync::Arc};

use tracing::{Instrument as _, debug, info};

use crate::{
    adapter::ChatAdapter,
    emoji::EmojiValue,
    error::ChatError,
    event::{
        ActionEvent, ChatEvent, ModalCloseEvent, ModalSubmitEvent, ReactionEvent, SlashCommandEvent,
    },
    message::IncomingMessage,
    thread::ThreadHandle,
};

// ---------------------------------------------------------------------------
// Handler type aliases
// ---------------------------------------------------------------------------

/// A boxed, type-erased async handler that receives a thread handle and an
/// incoming message.
pub(crate) type MentionHandler = Box<
    dyn Fn(ThreadHandle, IncomingMessage) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// Message handler paired with an optional substring pattern.
pub(crate) type MessageHandler = (
    Option<String>,
    Box<
        dyn Fn(ThreadHandle, IncomingMessage) -> Pin<Box<dyn Future<Output = ()> + Send>>
            + Send
            + Sync,
    >,
);

/// Handler for messages in subscribed threads.
pub(crate) type SubscribedMessageHandler = Box<
    dyn Fn(ThreadHandle, IncomingMessage) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// Action handler paired with an optional set of action-id filters.
pub(crate) type ActionHandler = (
    Option<Vec<String>>,
    Box<
        dyn Fn(ActionEvent, ThreadHandle) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
    >,
);

/// Reaction handler paired with an optional set of emoji filters.
pub(crate) type ReactionHandler = (
    Option<Vec<EmojiValue>>,
    Box<dyn Fn(ReactionEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
);

/// Slash-command handler paired with an optional set of command filters.
pub(crate) type SlashCommandHandler = (
    Option<Vec<String>>,
    Box<dyn Fn(SlashCommandEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
);

/// Modal-submit handler paired with an optional set of callback-id filters.
pub(crate) type ModalSubmitHandler = (
    Option<Vec<String>>,
    Box<dyn Fn(ModalSubmitEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
);

/// Handler invoked when a modal is dismissed without submitting.
pub(crate) type ModalCloseHandler =
    Box<dyn Fn(ModalCloseEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

// ---------------------------------------------------------------------------
// ChatBotConfig
// ---------------------------------------------------------------------------

/// Configuration knobs for [`ChatBot`].
#[derive(Debug, Clone)]
pub struct ChatBotConfig {
    /// Minimum interval (ms) between streaming edits.
    pub streaming_update_interval_ms: u64,
    /// Placeholder text shown while a stream is initialising.
    pub fallback_streaming_placeholder: String,
}

impl Default for ChatBotConfig {
    fn default() -> Self {
        Self { streaming_update_interval_ms: 500, fallback_streaming_placeholder: "...".into() }
    }
}

// ---------------------------------------------------------------------------
// ChatBot
// ---------------------------------------------------------------------------

/// Central hub that connects chat adapters to handler logic.
///
/// # Construction
///
/// ```rust,ignore
/// let bot = ChatBot::new(ChatBotConfig::default());
/// ```
///
/// Handlers are registered via the `on_*` builder methods.  Each method
/// accepts an async closure (returning a `Future`) and stores it for later
/// dispatch.
pub struct ChatBot {
    /// Registered chat platform adapters.
    pub(crate) adapters: Vec<Box<dyn ChatAdapter>>,
    /// Configuration.
    pub(crate) config: ChatBotConfig,

    // -- handler vecs -----------------------------------------------------
    pub(crate) mention_handlers: Vec<MentionHandler>,
    pub(crate) message_handlers: Vec<MessageHandler>,
    pub(crate) subscribed_message_handlers: Vec<SubscribedMessageHandler>,
    pub(crate) action_handlers: Vec<ActionHandler>,
    pub(crate) reaction_handlers: Vec<ReactionHandler>,
    pub(crate) slash_command_handlers: Vec<SlashCommandHandler>,
    pub(crate) modal_submit_handlers: Vec<ModalSubmitHandler>,
    pub(crate) modal_close_handlers: Vec<ModalCloseHandler>,

    /// Threads that the bot is actively subscribed to for follow-up messages.
    pub(crate) subscriptions: Arc<scc::HashMap<String, ()>>,
}

impl ChatBot {
    /// Create a new `ChatBot` with the given configuration.
    #[must_use]
    pub fn new(config: ChatBotConfig) -> Self {
        Self {
            adapters: Vec::new(),
            config,
            mention_handlers: Vec::new(),
            message_handlers: Vec::new(),
            subscribed_message_handlers: Vec::new(),
            action_handlers: Vec::new(),
            reaction_handlers: Vec::new(),
            slash_command_handlers: Vec::new(),
            modal_submit_handlers: Vec::new(),
            modal_close_handlers: Vec::new(),
            subscriptions: Arc::new(scc::HashMap::new()),
        }
    }

    // -----------------------------------------------------------------
    // Adapter registration
    // -----------------------------------------------------------------

    /// Add a chat platform adapter.
    pub fn add_adapter(&mut self, adapter: impl ChatAdapter + 'static) {
        self.adapters.push(Box::new(adapter));
    }

    // -----------------------------------------------------------------
    // Handler registration
    // -----------------------------------------------------------------

    /// Register a handler invoked when the bot is mentioned.
    pub fn on_mention<F, Fut>(&mut self, handler: F)
    where
        F: Fn(ThreadHandle, IncomingMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.mention_handlers.push(Box::new(move |t, m| Box::pin(handler(t, m))));
    }

    /// Register a handler for incoming messages.
    ///
    /// If `pattern` is `Some`, only messages whose text contains the
    /// substring will trigger the handler.
    pub fn on_message<F, Fut>(&mut self, pattern: Option<String>, handler: F)
    where
        F: Fn(ThreadHandle, IncomingMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.message_handlers.push((pattern, Box::new(move |t, m| Box::pin(handler(t, m)))));
    }

    /// Register a handler for messages in threads the bot has subscribed to.
    pub fn on_subscribed_message<F, Fut>(&mut self, handler: F)
    where
        F: Fn(ThreadHandle, IncomingMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.subscribed_message_handlers.push(Box::new(move |t, m| Box::pin(handler(t, m))));
    }

    /// Register a handler for interactive action events.
    ///
    /// If `action_ids` is `Some`, only actions whose `action_id` is in the
    /// list will trigger the handler.
    pub fn on_action<F, Fut>(&mut self, action_ids: Option<Vec<String>>, handler: F)
    where
        F: Fn(ActionEvent, ThreadHandle) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.action_handlers.push((action_ids, Box::new(move |a, t| Box::pin(handler(a, t)))));
    }

    /// Register a handler for reaction events.
    ///
    /// If `emojis` is `Some`, only reactions matching one of the listed
    /// emoji values will trigger the handler.
    pub fn on_reaction<F, Fut>(&mut self, emojis: Option<Vec<EmojiValue>>, handler: F)
    where
        F: Fn(ReactionEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.reaction_handlers.push((emojis, Box::new(move |r| Box::pin(handler(r)))));
    }

    /// Register a handler for slash-command events.
    ///
    /// If `commands` is `Some`, only commands whose name matches one of the
    /// listed strings will trigger the handler.
    pub fn on_slash_command<F, Fut>(&mut self, commands: Option<Vec<String>>, handler: F)
    where
        F: Fn(SlashCommandEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.slash_command_handlers.push((commands, Box::new(move |s| Box::pin(handler(s)))));
    }

    /// Register a handler for modal-submit events.
    ///
    /// If `callback_ids` is `Some`, only submissions whose `callback_id`
    /// matches one of the listed strings will trigger the handler.
    pub fn on_modal_submit<F, Fut>(&mut self, callback_ids: Option<Vec<String>>, handler: F)
    where
        F: Fn(ModalSubmitEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.modal_submit_handlers.push((callback_ids, Box::new(move |ms| Box::pin(handler(ms)))));
    }

    /// Register a handler for modal-close (dismiss) events.
    pub fn on_modal_close<F, Fut>(&mut self, handler: F)
    where
        F: Fn(ModalCloseEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.modal_close_handlers.push(Box::new(move |mc| Box::pin(handler(mc))));
    }

    // -----------------------------------------------------------------
    // Event dispatch loop
    // -----------------------------------------------------------------

    /// Start the event dispatch loop.
    ///
    /// This method takes ownership of the stored adapters via `&mut self`,
    /// polls each adapter's [`recv_event`](ChatAdapter::recv_event) in a
    /// round-robin fashion using `futures_util::stream::select_all`, and
    /// dispatches every incoming [`ChatEvent`] to the registered handlers.
    ///
    /// The loop terminates when **all** adapters return `None` (i.e. their
    /// event sources are exhausted).
    ///
    /// # Errors
    ///
    /// Returns `Err(ChatError::Closed)` if no adapters have been registered.
    pub async fn run(&mut self) -> Result<(), ChatError> {
        use futures_util::stream::{self, StreamExt as _};

        if self.adapters.is_empty() {
            return Err(ChatError::Closed);
        }

        // Take ownership of adapters so we can get `&mut` refs for
        // `recv_event`.  Each adapter becomes a stream of events.
        let adapters = std::mem::take(&mut self.adapters);

        // Build one stream per adapter using `futures_util::stream::unfold`.
        // Each stream is boxed so it satisfies the `Unpin` bound required
        // by `select_all`.
        let streams: Vec<_> = adapters
            .into_iter()
            .map(|adapter| {
                Box::pin(stream::unfold(adapter, |mut a| async move {
                    let event = a.recv_event().await;
                    event.map(|e| (e, a))
                }))
            })
            .collect();

        let mut merged = stream::select_all(streams);

        info!("ChatBot event loop started");

        while let Some(event) = merged.next().await {
            self.dispatch_event(event).await;
        }

        info!("ChatBot event loop finished — all adapters exhausted");
        Ok(())
    }

    // -----------------------------------------------------------------
    // Internal dispatch
    // -----------------------------------------------------------------

    /// Route a single [`ChatEvent`] to the appropriate handlers.
    async fn dispatch_event(&self, event: ChatEvent) {
        match event {
            ChatEvent::Mention { thread_id, message } => {
                let span = tracing::info_span!("dispatch_mention", thread_id = %thread_id);
                async {
                    let is_subscribed = self.subscriptions.contains_async(&thread_id).await;

                    if is_subscribed {
                        debug!(
                            thread_id = %thread_id,
                            "mention in subscribed thread — routing to subscribed handlers"
                        );
                        self.dispatch_subscribed_message(thread_id, message).await;
                    } else {
                        debug!(
                            thread_id = %thread_id,
                            handler_count = self.mention_handlers.len(),
                            "dispatching mention"
                        );
                        for handler in &self.mention_handlers {
                            let handle = self.make_thread_handle(&thread_id);
                            handler(handle, message.clone()).await;
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::Message { thread_id, message } => {
                let span = tracing::info_span!("dispatch_message", thread_id = %thread_id);
                async {
                    let is_subscribed = self.subscriptions.contains_async(&thread_id).await;

                    if is_subscribed {
                        debug!(
                            thread_id = %thread_id,
                            "message in subscribed thread — routing to subscribed handlers"
                        );
                        self.dispatch_subscribed_message(thread_id, message).await;
                    } else {
                        self.dispatch_message_handlers(&thread_id, message).await;
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::Reaction(reaction) => {
                let span = tracing::info_span!(
                    "dispatch_reaction",
                    thread_id = %reaction.thread_id,
                    emoji = %reaction.emoji
                );
                async {
                    for (filter, handler) in &self.reaction_handlers {
                        let should_fire = match filter {
                            Some(emojis) => emojis.contains(&reaction.emoji),
                            None => true,
                        };
                        if should_fire {
                            handler(reaction.clone()).await;
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::Action(action) => {
                let span = tracing::info_span!(
                    "dispatch_action",
                    action_id = %action.action_id,
                    thread_id = %action.thread_id
                );
                async {
                    for (filter, handler) in &self.action_handlers {
                        let should_fire = match filter {
                            Some(ids) => ids.contains(&action.action_id),
                            None => true,
                        };
                        if should_fire {
                            let handle = self.make_thread_handle(&action.thread_id);
                            handler(action.clone(), handle).await;
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::SlashCommand(cmd) => {
                let span = tracing::info_span!(
                    "dispatch_slash_command",
                    command = %cmd.command,
                    channel_id = %cmd.channel_id
                );
                async {
                    for (filter, handler) in &self.slash_command_handlers {
                        let should_fire = match filter {
                            Some(commands) => commands.contains(&cmd.command),
                            None => true,
                        };
                        if should_fire {
                            handler(cmd.clone()).await;
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::ModalSubmit(submit) => {
                let span = tracing::info_span!(
                    "dispatch_modal_submit",
                    callback_id = %submit.callback_id
                );
                async {
                    for (filter, handler) in &self.modal_submit_handlers {
                        let should_fire = match filter {
                            Some(ids) => ids.contains(&submit.callback_id),
                            None => true,
                        };
                        if should_fire {
                            handler(submit.clone()).await;
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            ChatEvent::ModalClose(close) => {
                let span = tracing::info_span!(
                    "dispatch_modal_close",
                    callback_id = %close.callback_id
                );
                async {
                    for handler in &self.modal_close_handlers {
                        handler(close.clone()).await;
                    }
                }
                .instrument(span)
                .await;
            }
        }
    }

    /// Dispatch to all subscribed-message handlers for the given thread.
    async fn dispatch_subscribed_message(&self, thread_id: String, message: IncomingMessage) {
        debug!(
            thread_id = %thread_id,
            handler_count = self.subscribed_message_handlers.len(),
            "dispatching subscribed message"
        );
        for handler in &self.subscribed_message_handlers {
            let handle = self.make_thread_handle(&thread_id);
            handler(handle, message.clone()).await;
        }
    }

    /// Dispatch to matching `on_message` handlers (substring filter).
    async fn dispatch_message_handlers(&self, thread_id: &str, message: IncomingMessage) {
        debug!(
            thread_id = %thread_id,
            handler_count = self.message_handlers.len(),
            "dispatching message"
        );
        for (pattern, handler) in &self.message_handlers {
            let should_fire = match pattern {
                Some(pat) => message.text.contains(pat.as_str()),
                None => true,
            };
            if should_fire {
                let handle = self.make_thread_handle(thread_id);
                handler(handle, message.clone()).await;
            }
        }
    }

    /// Build a [`ThreadHandle`] for the given thread id.
    ///
    /// **Note:** This requires at least one adapter to be present. During
    /// `run()`, adapters have been moved out, so this method uses a
    /// placeholder. The `ThreadHandle` itself is designed to work with
    /// an `Arc<dyn ChatAdapter>` which is populated properly by the
    /// dispatch layer in Task 3.3.
    ///
    /// For now the handle is created with the adapter field unset — this
    /// will be resolved when `ThreadHandle` gets its method
    /// implementations.
    fn make_thread_handle(&self, thread_id: &str) -> ThreadHandle {
        ThreadHandle {
            thread_id: thread_id.to_owned(),
            adapter: Arc::new(NullAdapter),
            subscriptions: Arc::clone(&self.subscriptions),
        }
    }
}

// ---------------------------------------------------------------------------
// NullAdapter — placeholder for ThreadHandle when adapter ref is unavailable
// ---------------------------------------------------------------------------

/// A minimal no-op adapter used as a placeholder in thread handles when
/// the real adapter reference is not yet available.
struct NullAdapter;

#[async_trait::async_trait]
impl ChatAdapter for NullAdapter {
    fn name(&self) -> &'static str {
        "null"
    }

    async fn post_message(
        &self,
        _thread_id: &str,
        _message: &crate::message::AdapterPostableMessage,
    ) -> Result<crate::message::SentMessage, ChatError> {
        Err(ChatError::Closed)
    }

    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _message: &crate::message::AdapterPostableMessage,
    ) -> Result<crate::message::SentMessage, ChatError> {
        Err(ChatError::Closed)
    }

    async fn delete_message(&self, _thread_id: &str, _message_id: &str) -> Result<(), ChatError> {
        Err(ChatError::Closed)
    }

    fn render_card(&self, _card: &crate::card::CardElement) -> String {
        String::new()
    }

    fn render_message(&self, _message: &crate::message::AdapterPostableMessage) -> String {
        String::new()
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        None
    }
}

impl std::fmt::Debug for ChatBot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatBot")
            .field("adapters", &self.adapters.len())
            .field("config", &self.config)
            .field("mention_handlers", &self.mention_handlers.len())
            .field("message_handlers", &self.message_handlers.len())
            .field("subscribed_message_handlers", &self.subscribed_message_handlers.len())
            .field("action_handlers", &self.action_handlers.len())
            .field("reaction_handlers", &self.reaction_handlers.len())
            .field("slash_command_handlers", &self.slash_command_handlers.len())
            .field("modal_submit_handlers", &self.modal_submit_handlers.len())
            .field("modal_close_handlers", &self.modal_close_handlers.len())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Static assertions
// ---------------------------------------------------------------------------

// Ensure `ChatBot` is Send + Sync so it can be shared across async tasks.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ChatBot>();
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;
    use crate::message::{AdapterPostableMessage, Author, IncomingMessage, SentMessage};

    // ---------------------------------------------------------------
    // Shared test MockAdapter
    // ---------------------------------------------------------------

    /// A mock adapter that yields a pre-loaded queue of events.
    struct MockAdapter {
        name: &'static str,
        events: Mutex<VecDeque<ChatEvent>>,
    }

    impl MockAdapter {
        fn new(name: &'static str, events: Vec<ChatEvent>) -> Self {
            Self { name, events: Mutex::new(VecDeque::from(events)) }
        }
    }

    #[async_trait::async_trait]
    impl ChatAdapter for MockAdapter {
        fn name(&self) -> &str {
            self.name
        }

        async fn post_message(
            &self,
            _thread_id: &str,
            _message: &AdapterPostableMessage,
        ) -> Result<SentMessage, ChatError> {
            Ok(SentMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                adapter_name: self.name.into(),
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
                adapter_name: self.name.into(),
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

        fn render_card(&self, _card: &crate::card::CardElement) -> String {
            String::new()
        }

        fn render_message(&self, _message: &AdapterPostableMessage) -> String {
            String::new()
        }

        async fn recv_event(&mut self) -> Option<ChatEvent> {
            let Ok(mut q) = self.events.lock() else {
                return None;
            };
            q.pop_front()
        }
    }

    // ---------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------

    fn sample_author() -> Author {
        Author {
            user_id: "u1".into(),
            user_name: "alice".into(),
            full_name: "Alice".into(),
            is_bot: false,
        }
    }

    fn sample_message(text: &str) -> IncomingMessage {
        IncomingMessage {
            id: "m1".into(),
            text: text.into(),
            author: sample_author(),
            attachments: vec![],
            is_mention: false,
            thread_id: "t1".into(),
            timestamp: None,
        }
    }

    // ---------------------------------------------------------------
    // Registration tests (existing)
    // ---------------------------------------------------------------

    /// `ChatBot` must be `Send + Sync`.
    #[test]
    fn chatbot_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ChatBot>();
        assert_sync::<ChatBot>();
    }

    #[test]
    fn default_config() {
        let cfg = ChatBotConfig::default();
        assert_eq!(cfg.streaming_update_interval_ms, 500);
        assert_eq!(cfg.fallback_streaming_placeholder, "...");
    }

    #[test]
    fn new_chatbot_has_empty_handlers() {
        let bot = ChatBot::new(ChatBotConfig::default());
        assert!(bot.adapters.is_empty());
        assert!(bot.mention_handlers.is_empty());
        assert!(bot.message_handlers.is_empty());
        assert!(bot.subscribed_message_handlers.is_empty());
        assert!(bot.action_handlers.is_empty());
        assert!(bot.reaction_handlers.is_empty());
        assert!(bot.slash_command_handlers.is_empty());
        assert!(bot.modal_submit_handlers.is_empty());
        assert!(bot.modal_close_handlers.is_empty());
    }

    #[test]
    fn register_mention_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_mention(|_thread, _msg| async {});
        assert_eq!(bot.mention_handlers.len(), 1);
    }

    #[test]
    fn register_message_handler_with_pattern() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_message(Some("hello".into()), |_thread, _msg| async {});
        assert_eq!(bot.message_handlers.len(), 1);
        assert_eq!(bot.message_handlers[0].0.as_deref(), Some("hello"));
    }

    #[test]
    fn register_message_handler_without_pattern() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_message(None, |_thread, _msg| async {});
        assert_eq!(bot.message_handlers.len(), 1);
        assert!(bot.message_handlers[0].0.is_none());
    }

    #[test]
    fn register_subscribed_message_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_subscribed_message(|_thread, _msg| async {});
        assert_eq!(bot.subscribed_message_handlers.len(), 1);
    }

    #[test]
    fn register_action_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_action(Some(vec!["btn_ok".into()]), |_action, _thread| async {});
        assert_eq!(bot.action_handlers.len(), 1);
    }

    #[test]
    fn register_reaction_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_reaction(None, |_reaction| async {});
        assert_eq!(bot.reaction_handlers.len(), 1);
    }

    #[test]
    fn register_slash_command_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_slash_command(Some(vec!["/deploy".into()]), |_cmd| async {});
        assert_eq!(bot.slash_command_handlers.len(), 1);
    }

    #[test]
    fn register_modal_submit_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_modal_submit(Some(vec!["feedback".into()]), |_submit| async {});
        assert_eq!(bot.modal_submit_handlers.len(), 1);
    }

    #[test]
    fn register_modal_close_handler() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_modal_close(|_close| async {});
        assert_eq!(bot.modal_close_handlers.len(), 1);
    }

    #[test]
    fn register_multiple_handlers() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_mention(|_t, _m| async {});
        bot.on_mention(|_t, _m| async {});
        bot.on_message(None, |_t, _m| async {});
        bot.on_action(None, |_a, _t| async {});
        assert_eq!(bot.mention_handlers.len(), 2);
        assert_eq!(bot.message_handlers.len(), 1);
        assert_eq!(bot.action_handlers.len(), 1);
    }

    // ---------------------------------------------------------------
    // Dispatch loop tests
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn run_returns_closed_when_no_adapters() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        let result = bot.run().await;
        assert!(matches!(result, Err(ChatError::Closed)));
    }

    #[tokio::test]
    async fn run_returns_ok_when_adapter_exhausted() {
        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.add_adapter(MockAdapter::new("mock", vec![]));
        let result = bot.run().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatch_mention_event() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_mention(move |_thread, _msg| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![ChatEvent::Mention {
                thread_id: "t1".into(),
                message: sample_message("hey @bot"),
            }],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_message_with_pattern() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_message(Some("hello".into()), move |_thread, _msg| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::Message {
                    thread_id: "t1".into(),
                    message: sample_message("hello world"),
                },
                ChatEvent::Message {
                    thread_id: "t1".into(),
                    message: sample_message("goodbye world"),
                },
            ],
        ));

        bot.run().await.expect("run should succeed");
        // Only the first message matches "hello"
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_message_no_pattern_catches_all() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_message(None, move |_thread, _msg| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::Message { thread_id: "t1".into(), message: sample_message("anything") },
                ChatEvent::Message { thread_id: "t1".into(), message: sample_message("else") },
            ],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn subscribed_thread_routes_to_subscribed_handlers() {
        let sub_counter = Arc::new(AtomicUsize::new(0));
        let msg_counter = Arc::new(AtomicUsize::new(0));

        let sc = Arc::clone(&sub_counter);
        let mc = Arc::clone(&msg_counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());

        // Pre-subscribe to thread "t1"
        let _ = bot.subscriptions.insert_async("t1".into(), ()).await;

        bot.on_subscribed_message(move |_thread, _msg| {
            let sc = Arc::clone(&sc);
            async move {
                sc.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.on_message(None, move |_thread, _msg| {
            let mc = Arc::clone(&mc);
            async move {
                mc.fetch_add(1, Ordering::SeqCst);
            }
        });

        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::Message {
                    thread_id: "t1".into(),
                    message: sample_message("in subscribed thread"),
                },
                ChatEvent::Message {
                    thread_id: "t2".into(),
                    message: sample_message("in non-subscribed thread"),
                },
            ],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(sub_counter.load(Ordering::SeqCst), 1);
        assert_eq!(msg_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_action_with_filter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_action(Some(vec!["approve".into()]), move |_action, _thread| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::Action(ActionEvent {
                    action_id: "approve".into(),
                    thread_id: "t1".into(),
                    message_id: "m1".into(),
                    user: sample_author(),
                    value: None,
                    trigger_id: None,
                    adapter_name: "mock".into(),
                }),
                ChatEvent::Action(ActionEvent {
                    action_id: "reject".into(),
                    thread_id: "t1".into(),
                    message_id: "m1".into(),
                    user: sample_author(),
                    value: None,
                    trigger_id: None,
                    adapter_name: "mock".into(),
                }),
            ],
        ));

        bot.run().await.expect("run should succeed");
        // Only "approve" should fire
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_reaction_with_emoji_filter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let thumbs_up = EmojiValue::from_well_known(crate::emoji::WellKnownEmoji::ThumbsUp);
        let thumbs_down = EmojiValue::from_well_known(crate::emoji::WellKnownEmoji::ThumbsDown);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_reaction(Some(vec![thumbs_up.clone()]), move |_reaction| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });

        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::Reaction(ReactionEvent {
                    thread_id: "t1".into(),
                    message_id: "m1".into(),
                    user: sample_author(),
                    emoji: thumbs_up,
                    added: true,
                    adapter_name: "mock".into(),
                }),
                ChatEvent::Reaction(ReactionEvent {
                    thread_id: "t1".into(),
                    message_id: "m1".into(),
                    user: sample_author(),
                    emoji: thumbs_down,
                    added: true,
                    adapter_name: "mock".into(),
                }),
            ],
        ));

        bot.run().await.expect("run should succeed");
        // Only thumbs_up should fire
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_slash_command_with_filter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_slash_command(Some(vec!["/deploy".into()]), move |_cmd| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::SlashCommand(SlashCommandEvent {
                    command: "/deploy".into(),
                    text: "prod".into(),
                    channel_id: "c1".into(),
                    user: sample_author(),
                    trigger_id: None,
                    adapter_name: "mock".into(),
                }),
                ChatEvent::SlashCommand(SlashCommandEvent {
                    command: "/help".into(),
                    text: "".into(),
                    channel_id: "c1".into(),
                    user: sample_author(),
                    trigger_id: None,
                    adapter_name: "mock".into(),
                }),
            ],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_modal_submit_with_filter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_modal_submit(Some(vec!["feedback".into()]), move |_submit| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![
                ChatEvent::ModalSubmit(ModalSubmitEvent {
                    callback_id: "feedback".into(),
                    view_id: "v1".into(),
                    user: sample_author(),
                    values: std::collections::HashMap::new(),
                    private_metadata: None,
                    adapter_name: "mock".into(),
                }),
                ChatEvent::ModalSubmit(ModalSubmitEvent {
                    callback_id: "other".into(),
                    view_id: "v2".into(),
                    user: sample_author(),
                    values: std::collections::HashMap::new(),
                    private_metadata: None,
                    adapter_name: "mock".into(),
                }),
            ],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_modal_close() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_modal_close(move |_close| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![ChatEvent::ModalClose(ModalCloseEvent {
                callback_id: "feedback".into(),
                view_id: "v1".into(),
                user: sample_author(),
                adapter_name: "mock".into(),
            })],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mention_in_subscribed_thread_routes_to_subscribed_handler() {
        let mention_counter = Arc::new(AtomicUsize::new(0));
        let sub_counter = Arc::new(AtomicUsize::new(0));

        let mc = Arc::clone(&mention_counter);
        let sc = Arc::clone(&sub_counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());

        // Pre-subscribe to thread "t1"
        let _ = bot.subscriptions.insert_async("t1".into(), ()).await;

        bot.on_mention(move |_thread, _msg| {
            let mc = Arc::clone(&mc);
            async move {
                mc.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.on_subscribed_message(move |_thread, _msg| {
            let sc = Arc::clone(&sc);
            async move {
                sc.fetch_add(1, Ordering::SeqCst);
            }
        });

        bot.add_adapter(MockAdapter::new(
            "mock",
            vec![ChatEvent::Mention {
                thread_id: "t1".into(),
                message: sample_message("hey @bot"),
            }],
        ));

        bot.run().await.expect("run should succeed");
        // Should route to subscribed handler, not mention handler
        assert_eq!(mention_counter.load(Ordering::SeqCst), 0);
        assert_eq!(sub_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn multiple_adapters_all_events_processed() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let mut bot = ChatBot::new(ChatBotConfig::default());
        bot.on_message(None, move |_thread, _msg| {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
            }
        });
        bot.add_adapter(MockAdapter::new(
            "adapter-a",
            vec![ChatEvent::Message { thread_id: "t1".into(), message: sample_message("from A") }],
        ));
        bot.add_adapter(MockAdapter::new(
            "adapter-b",
            vec![ChatEvent::Message { thread_id: "t2".into(), message: sample_message("from B") }],
        ));

        bot.run().await.expect("run should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
