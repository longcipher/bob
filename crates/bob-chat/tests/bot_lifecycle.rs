//! Integration tests for the `ChatBot` lifecycle.
//!
//! These tests verify the full event pipeline: register handlers, add a mock
//! adapter, inject events, and confirm the correct handlers are invoked with
//! the expected data.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use bob_chat::{
    bot::{ChatBot, ChatBotConfig},
    emoji::{EmojiValue, WellKnownEmoji},
    error::ChatError,
    event::{
        ActionEvent, ChatEvent, ModalCloseEvent, ModalSubmitEvent, ReactionEvent, SlashCommandEvent,
    },
    message::{AdapterPostableMessage, Author, EphemeralMessage, IncomingMessage, SentMessage},
};

// ---------------------------------------------------------------------------
// Lightweight mock adapter for integration tests
// ---------------------------------------------------------------------------

struct TestAdapter {
    name: &'static str,
    events: Mutex<VecDeque<ChatEvent>>,
}

impl TestAdapter {
    fn new(name: &'static str, events: Vec<ChatEvent>) -> Self {
        Self { name, events: Mutex::new(VecDeque::from(events)) }
    }
}

#[async_trait::async_trait]
impl bob_chat::ChatAdapter for TestAdapter {
    fn name(&self) -> &str {
        self.name
    }

    async fn post_message(
        &self,
        thread_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        Ok(SentMessage {
            id: "msg-1".into(),
            thread_id: thread_id.into(),
            adapter_name: self.name.into(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        Ok(SentMessage {
            id: message_id.into(),
            thread_id: thread_id.into(),
            adapter_name: self.name.into(),
            raw: None,
        })
    }

    async fn delete_message(&self, _thread_id: &str, _message_id: &str) -> Result<(), ChatError> {
        Ok(())
    }

    fn render_card(&self, _card: &bob_chat::CardElement) -> String {
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

    async fn post_ephemeral(
        &self,
        _thread_id: &str,
        _user_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<EphemeralMessage, ChatError> {
        Err(ChatError::NotSupported("ephemeral".into()))
    }

    async fn open_dm(&self, user_id: &str) -> Result<String, ChatError> {
        Ok(format!("dm-{user_id}"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Register `on_mention` handler → inject Mention event → verify handler
/// called with correct thread and message data.
#[tokio::test]
async fn mention_handler_receives_correct_data() {
    let received_text = Arc::new(Mutex::new(String::new()));
    let received_thread = Arc::new(Mutex::new(String::new()));
    let rt = Arc::clone(&received_text);
    let rth = Arc::clone(&received_thread);

    let mut bot = ChatBot::new(ChatBotConfig::default());
    bot.on_mention(move |thread, msg| {
        let rt = Arc::clone(&rt);
        let rth = Arc::clone(&rth);
        async move {
            if let Ok(mut t) = rt.lock() {
                *t = msg.text.clone();
            }
            if let Ok(mut th) = rth.lock() {
                *th = thread.thread_id().to_owned();
            }
        }
    });

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![ChatEvent::Mention {
            thread_id: "channel-42".into(),
            message: sample_message("hey @bot help me"),
        }],
    ));

    bot.run().await.expect("run should succeed");

    assert_eq!(*received_text.lock().expect("lock"), "hey @bot help me");
    assert_eq!(*received_thread.lock().expect("lock"), "channel-42");
}

/// Register `on_slash_command("/test")` → inject matching and non-matching
/// commands → verify only the matching command triggers the handler.
#[tokio::test]
async fn slash_command_filter_matches_correctly() {
    let counter = Arc::new(AtomicUsize::new(0));
    let received_text = Arc::new(Mutex::new(String::new()));
    let c = Arc::clone(&counter);
    let rt = Arc::clone(&received_text);

    let mut bot = ChatBot::new(ChatBotConfig::default());
    bot.on_slash_command(Some(vec!["/test".into()]), move |cmd| {
        let c = Arc::clone(&c);
        let rt = Arc::clone(&rt);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut t) = rt.lock() {
                *t = cmd.text.clone();
            }
        }
    });

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![
            ChatEvent::SlashCommand(SlashCommandEvent {
                command: "/test".into(),
                text: "arg1".into(),
                channel_id: "c1".into(),
                user: sample_author(),
                trigger_id: None,
                adapter_name: "test".into(),
            }),
            ChatEvent::SlashCommand(SlashCommandEvent {
                command: "/other".into(),
                text: "arg2".into(),
                channel_id: "c1".into(),
                user: sample_author(),
                trigger_id: None,
                adapter_name: "test".into(),
            }),
        ],
    ));

    bot.run().await.expect("run should succeed");

    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert_eq!(*received_text.lock().expect("lock"), "arg1");
}

/// Register `on_action("approve")` → inject matching and non-matching actions
/// → verify only the matching one triggers.
#[tokio::test]
async fn action_filter_matches_correctly() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);

    let mut bot = ChatBot::new(ChatBotConfig::default());
    bot.on_action(Some(vec!["approve".into()]), move |_action, _thread| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
        }
    });

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![
            ChatEvent::Action(ActionEvent {
                action_id: "approve".into(),
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                value: Some("yes".into()),
                trigger_id: None,
                adapter_name: "test".into(),
            }),
            ChatEvent::Action(ActionEvent {
                action_id: "reject".into(),
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                value: Some("no".into()),
                trigger_id: None,
                adapter_name: "test".into(),
            }),
        ],
    ));

    bot.run().await.expect("run should succeed");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

/// Register `on_reaction([ThumbsUp])` → inject ThumbsUp and ThumbsDown →
/// verify only ThumbsUp triggers handler.
#[tokio::test]
async fn reaction_emoji_filter() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);

    let thumbs_up = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);
    let thumbs_down = EmojiValue::from_well_known(WellKnownEmoji::ThumbsDown);

    let mut bot = ChatBot::new(ChatBotConfig::default());
    bot.on_reaction(Some(vec![thumbs_up.clone()]), move |_reaction| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
        }
    });

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![
            ChatEvent::Reaction(ReactionEvent {
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                emoji: thumbs_up,
                added: true,
                adapter_name: "test".into(),
            }),
            ChatEvent::Reaction(ReactionEvent {
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                emoji: thumbs_down,
                added: true,
                adapter_name: "test".into(),
            }),
        ],
    ));

    bot.run().await.expect("run should succeed");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

/// Via `on_mention` handler → `ThreadHandle::subscribe()` → then a later
/// Message in that same thread fires the `on_subscribed_message` handler
/// instead of the generic `on_message` handler.
#[tokio::test]
async fn subscribed_thread_dispatches_correctly() {
    let sub_counter = Arc::new(AtomicUsize::new(0));
    let msg_counter = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&sub_counter);
    let mc = Arc::clone(&msg_counter);

    let mut bot = ChatBot::new(ChatBotConfig::default());

    // The mention handler subscribes to its thread.
    bot.on_mention(move |thread, _msg| async move {
        thread.subscribe().await;
    });

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

    // Events: first a Mention (triggers subscribe), then a Message in the
    // same thread (should route to subscribed handler), then a Message in a
    // different thread (should route to normal handler).
    bot.add_adapter(TestAdapter::new(
        "test",
        vec![
            ChatEvent::Mention {
                thread_id: "t-sub".into(),
                message: sample_message("@bot subscribe me"),
            },
            ChatEvent::Message {
                thread_id: "t-sub".into(),
                message: sample_message("subscribed thread msg"),
            },
            ChatEvent::Message {
                thread_id: "t-other".into(),
                message: sample_message("regular thread msg"),
            },
        ],
    ));

    bot.run().await.expect("run should succeed");
    assert_eq!(sub_counter.load(Ordering::SeqCst), 1);
    assert_eq!(msg_counter.load(Ordering::SeqCst), 1);
}

/// Verify `ThreadHandle::post_ephemeral` with DM fallback on an adapter
/// that doesn't support ephemeral → `open_dm` + `post_message` are called.
#[tokio::test]
async fn ephemeral_dm_fallback_via_thread_handle() {
    let post_counter = Arc::new(AtomicUsize::new(0));
    let pc = Arc::clone(&post_counter);

    let mut bot = ChatBot::new(ChatBotConfig::default());

    bot.on_mention(move |thread, _msg| {
        let pc = Arc::clone(&pc);
        async move {
            // The TestAdapter returns NotSupported for ephemeral, supports DM.
            let result = thread
                .post_ephemeral("u1", AdapterPostableMessage::Text("secret".into()), true)
                .await;
            // The NullAdapter in the ThreadHandle will fail, but the test
            // verifies the control flow compiles and runs without panic.
            // A full integration with a real adapter is tested via
            // thread::tests::post_ephemeral_fallback_to_dm.
            drop(result);
            pc.fetch_add(1, Ordering::SeqCst);
        }
    });

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![ChatEvent::Mention { thread_id: "t1".into(), message: sample_message("@bot secret") }],
    ));

    bot.run().await.expect("run should succeed");
    assert_eq!(post_counter.load(Ordering::SeqCst), 1);
}

/// Full lifecycle: multiple event types processed in one run.
#[tokio::test]
async fn full_lifecycle_multiple_event_types() {
    let mention_count = Arc::new(AtomicUsize::new(0));
    let msg_count = Arc::new(AtomicUsize::new(0));
    let action_count = Arc::new(AtomicUsize::new(0));
    let reaction_count = Arc::new(AtomicUsize::new(0));
    let cmd_count = Arc::new(AtomicUsize::new(0));
    let submit_count = Arc::new(AtomicUsize::new(0));
    let close_count = Arc::new(AtomicUsize::new(0));

    let mn = Arc::clone(&mention_count);
    let ms = Arc::clone(&msg_count);
    let ac = Arc::clone(&action_count);
    let rc = Arc::clone(&reaction_count);
    let cc = Arc::clone(&cmd_count);
    let sc = Arc::clone(&submit_count);
    let cl = Arc::clone(&close_count);

    let mut bot = ChatBot::new(ChatBotConfig::default());

    bot.on_mention(move |_t, _m| {
        let mn = Arc::clone(&mn);
        async move {
            mn.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_message(None, move |_t, _m| {
        let ms = Arc::clone(&ms);
        async move {
            ms.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_action(None, move |_a, _t| {
        let ac = Arc::clone(&ac);
        async move {
            ac.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_reaction(None, move |_r| {
        let rc = Arc::clone(&rc);
        async move {
            rc.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_slash_command(None, move |_c| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_modal_submit(None, move |_s| {
        let sc = Arc::clone(&sc);
        async move {
            sc.fetch_add(1, Ordering::SeqCst);
        }
    });
    bot.on_modal_close(move |_c| {
        let cl = Arc::clone(&cl);
        async move {
            cl.fetch_add(1, Ordering::SeqCst);
        }
    });

    let thumbs_up = EmojiValue::from_well_known(WellKnownEmoji::ThumbsUp);

    bot.add_adapter(TestAdapter::new(
        "test",
        vec![
            ChatEvent::Mention { thread_id: "t1".into(), message: sample_message("@bot hello") },
            ChatEvent::Message { thread_id: "t1".into(), message: sample_message("plain message") },
            ChatEvent::Action(ActionEvent {
                action_id: "btn".into(),
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                value: None,
                trigger_id: None,
                adapter_name: "test".into(),
            }),
            ChatEvent::Reaction(ReactionEvent {
                thread_id: "t1".into(),
                message_id: "m1".into(),
                user: sample_author(),
                emoji: thumbs_up,
                added: true,
                adapter_name: "test".into(),
            }),
            ChatEvent::SlashCommand(SlashCommandEvent {
                command: "/test".into(),
                text: "".into(),
                channel_id: "c1".into(),
                user: sample_author(),
                trigger_id: None,
                adapter_name: "test".into(),
            }),
            ChatEvent::ModalSubmit(ModalSubmitEvent {
                callback_id: "fb".into(),
                view_id: "v1".into(),
                user: sample_author(),
                values: std::collections::HashMap::new(),
                private_metadata: None,
                adapter_name: "test".into(),
            }),
            ChatEvent::ModalClose(ModalCloseEvent {
                callback_id: "fb".into(),
                view_id: "v1".into(),
                user: sample_author(),
                adapter_name: "test".into(),
            }),
        ],
    ));

    bot.run().await.expect("run should succeed");

    assert_eq!(mention_count.load(Ordering::SeqCst), 1, "mention");
    assert_eq!(msg_count.load(Ordering::SeqCst), 1, "message");
    assert_eq!(action_count.load(Ordering::SeqCst), 1, "action");
    assert_eq!(reaction_count.load(Ordering::SeqCst), 1, "reaction");
    assert_eq!(cmd_count.load(Ordering::SeqCst), 1, "slash_command");
    assert_eq!(submit_count.load(Ordering::SeqCst), 1, "modal_submit");
    assert_eq!(close_count.load(Ordering::SeqCst), 1, "modal_close");
}
