//! # Bob Chat
//!
//! Platform-agnostic chat abstractions for the
//! [Bob Agent Framework](https://github.com/longcipher/bob).
//!
//! ## Overview
//!
//! `bob-chat` provides the structured chat layer that sits between chat
//! platform APIs (Slack, Discord, Telegram, CLI, etc.) and the agent loop.
//! It ships a trait-based adapter model, a central event-driven orchestrator
//! ([`ChatBot`]), and rich message primitives.
//!
//! ## Features
//!
//! - **[`ChatAdapter`] trait** — implement once per platform; covers posting, editing, deleting,
//!   reactions, ephemeral messages, DMs, modals, file uploads, and event streaming.
//! - **[`ChatBot`] orchestrator** — register adapters and typed handlers, then call
//!   [`ChatBot::run`] to poll all adapters concurrently and dispatch events.
//! - **[`ChatEvent`]** — seven event variants (Message, Mention, Reaction, Action, SlashCommand,
//!   ModalSubmit, ModalClose).
//! - **[`ThreadHandle`]** — scoped handle passed to handlers for replying, subscribing to threads,
//!   and managing ephemeral messages with DM fallback.
//! - **Rich messages** — [`CardElement`] (buttons, sections, images, fields) with a plain-text
//!   fallback renderer.
//! - **Modals** — [`ModalElement`] with text inputs, selects, and radio selects.
//! - **Emoji** — [`WellKnownEmoji`] (35 variants) with platform-specific format maps and a
//!   custom-emoji escape hatch.
//! - **Streaming** — [`TextStream`] and [`fallback_stream`] for progressive message delivery via
//!   post-then-edit.
//! - **Attachments** — [`Attachment`], [`FileUpload`] for file handling.
//! - **Error types** — [`ChatError`] with adapter, transport, rate-limit, auth, not-found,
//!   not-supported, and closed variants.
//!
//! ## Module Map
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`adapter`] | `ChatAdapter` trait definition |
//! | [`bot`] | `ChatBot` orchestrator and handler registration |
//! | [`card`] | Card/rich-message element types |
//! | [`emoji`] | Emoji types and well-known mappings |
//! | [`error`] | `ChatError` enum |
//! | [`event`] | `ChatEvent` variants and event structs |
//! | [`file`](mod@file) | Attachment and file upload types |
//! | [`message`] | Message types (postable, incoming, sent, ephemeral) |
//! | [`modal`] | Modal form dialog types |
//! | [`stream`] | Streaming text types |
//! | [`thread`] | `ThreadHandle` for in-handler interaction |
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use bob_chat::{ChatBot, ChatBotConfig};
//!
//! # async fn example() -> Result<(), bob_chat::ChatError> {
//! let mut bot = ChatBot::new(ChatBotConfig::default());
//!
//! // Register a mention handler.
//! bot.on_mention(|thread, message| async move {
//!     let _ = thread.post("Hello! You said: ".to_owned() + &message.text).await;
//! });
//!
//! // Add platform adapters and run.
//! // bot.add_adapter(my_slack_adapter);
//! // bot.run().await?;
//! # Ok(())
//! # }
//! ```

pub mod adapter;
pub mod bot;
pub mod card;
pub mod emoji;
pub mod error;
pub mod event;
pub mod file;
pub mod message;
pub mod modal;
pub mod stream;
pub mod thread;

#[cfg(test)]
pub(crate) mod test_utils;

pub use adapter::ChatAdapter;
/// Re-export channel types from `bob_core` for convenience.
pub use bob_core::channel::{Channel, ChannelError, ChannelMessage, ChannelOutput};
pub use bot::{ChatBot, ChatBotConfig};
pub use card::{
    ActionElement, ActionsElement, ButtonElement, ButtonStyle, CardChild, CardElement,
    FieldElement, FieldsElement, ImageElement, SectionElement, TextElement, TextStyle,
    render_card_as_text,
};
pub use emoji::{DefaultEmojiResolver, EmojiFormats, EmojiResolver, EmojiValue, WellKnownEmoji};
pub use error::ChatError;
pub use event::{
    ActionEvent, ChatEvent, ModalCloseEvent, ModalSubmitEvent, ReactionEvent, SlashCommandEvent,
};
pub use file::{Attachment, AttachmentKind, FileUpload};
pub use message::{
    AdapterPostableMessage, Author, EphemeralMessage, IncomingMessage, PostableMessage, SentMessage,
};
pub use modal::{
    ModalChild, ModalElement, RadioSelectElement, SelectElement, SelectOption, TextInputElement,
};
pub use stream::{StreamOptions, TextStream, fallback_stream};
pub use thread::ThreadHandle;
