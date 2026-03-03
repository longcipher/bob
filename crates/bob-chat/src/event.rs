//! Chat event types for adapter-to-bot communication.
//!
//! Events represent incoming payloads from chat adapters: messages, mentions,
//! reactions, interactive actions, slash commands, and modal interactions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    emoji::EmojiValue,
    message::{Author, IncomingMessage},
};

// ---------------------------------------------------------------------------
// ActionEvent
// ---------------------------------------------------------------------------

/// An interactive element (button, menu, etc.) was activated by a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEvent {
    /// Identifier of the action element that was triggered.
    pub action_id: String,
    /// Thread where the action originated.
    pub thread_id: String,
    /// Message containing the interactive element.
    pub message_id: String,
    /// The user who performed the action.
    pub user: Author,
    /// Optional value associated with the action element.
    pub value: Option<String>,
    /// Trigger identifier for opening modals or other follow-ups.
    pub trigger_id: Option<String>,
    /// Name of the adapter that delivered this event.
    pub adapter_name: String,
}

// ---------------------------------------------------------------------------
// ReactionEvent
// ---------------------------------------------------------------------------

/// A reaction was added to or removed from a message.
#[derive(Debug, Clone)]
pub struct ReactionEvent {
    /// Thread containing the reacted message.
    pub thread_id: String,
    /// The message that was reacted to.
    pub message_id: String,
    /// The user who added or removed the reaction.
    pub user: Author,
    /// The emoji used for the reaction.
    pub emoji: EmojiValue,
    /// `true` when the reaction was added, `false` when removed.
    pub added: bool,
    /// Name of the adapter that delivered this event.
    pub adapter_name: String,
}

// ---------------------------------------------------------------------------
// SlashCommandEvent
// ---------------------------------------------------------------------------

/// A slash command was invoked by a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommandEvent {
    /// The command name (e.g. `/deploy`).
    pub command: String,
    /// Any text following the command name.
    pub text: String,
    /// Channel where the command was issued.
    pub channel_id: String,
    /// The user who invoked the command.
    pub user: Author,
    /// Trigger identifier for opening modals or other follow-ups.
    pub trigger_id: Option<String>,
    /// Name of the adapter that delivered this event.
    pub adapter_name: String,
}

// ---------------------------------------------------------------------------
// ModalSubmitEvent
// ---------------------------------------------------------------------------

/// A modal view was submitted by a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModalSubmitEvent {
    /// Callback identifier used to route this submission.
    pub callback_id: String,
    /// Platform-assigned view identifier.
    pub view_id: String,
    /// The user who submitted the modal.
    pub user: Author,
    /// Key-value pairs from the modal's input elements.
    pub values: HashMap<String, String>,
    /// Opaque metadata string passed through from the modal open call.
    pub private_metadata: Option<String>,
    /// Name of the adapter that delivered this event.
    pub adapter_name: String,
}

// ---------------------------------------------------------------------------
// ModalCloseEvent
// ---------------------------------------------------------------------------

/// A modal view was closed (dismissed) by a user without submitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModalCloseEvent {
    /// Callback identifier used to route this close event.
    pub callback_id: String,
    /// Platform-assigned view identifier.
    pub view_id: String,
    /// The user who closed the modal.
    pub user: Author,
    /// Name of the adapter that delivered this event.
    pub adapter_name: String,
}

// ---------------------------------------------------------------------------
// ChatEvent
// ---------------------------------------------------------------------------

/// Top-level event dispatched from a chat adapter to the bot.
///
/// Covers all supported interaction types: plain messages, mentions,
/// reactions, interactive actions, slash commands, and modal lifecycle.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// A regular message was posted in a channel or thread.
    Message {
        /// Thread the message belongs to.
        thread_id: String,
        /// The received message.
        message: IncomingMessage,
    },
    /// The bot was explicitly mentioned in a message.
    Mention {
        /// Thread the message belongs to.
        thread_id: String,
        /// The received message containing the mention.
        message: IncomingMessage,
    },
    /// A reaction was added or removed.
    Reaction(ReactionEvent),
    /// An interactive element was activated.
    Action(ActionEvent),
    /// A slash command was invoked.
    SlashCommand(SlashCommandEvent),
    /// A modal was submitted.
    ModalSubmit(ModalSubmitEvent),
    /// A modal was closed without submitting.
    ModalClose(ModalCloseEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_author() -> Author {
        Author {
            user_id: "u1".into(),
            user_name: "alice".into(),
            full_name: "Alice".into(),
            is_bot: false,
        }
    }

    fn sample_incoming() -> IncomingMessage {
        IncomingMessage {
            id: "m1".into(),
            text: "hello".into(),
            author: sample_author(),
            attachments: vec![],
            is_mention: false,
            thread_id: "t1".into(),
            timestamp: None,
        }
    }

    #[test]
    fn chat_event_message_variant() {
        let event = ChatEvent::Message { thread_id: "t1".into(), message: sample_incoming() };
        assert!(matches!(event, ChatEvent::Message { .. }));
    }

    #[test]
    fn chat_event_mention_variant() {
        let event = ChatEvent::Mention { thread_id: "t1".into(), message: sample_incoming() };
        assert!(matches!(event, ChatEvent::Mention { .. }));
    }

    #[test]
    fn chat_event_action_variant() {
        let event = ChatEvent::Action(ActionEvent {
            action_id: "btn_approve".into(),
            thread_id: "t1".into(),
            message_id: "m1".into(),
            user: sample_author(),
            value: Some("yes".into()),
            trigger_id: None,
            adapter_name: "slack".into(),
        });
        assert!(matches!(event, ChatEvent::Action(_)));
    }

    #[test]
    fn chat_event_slash_command_variant() {
        let event = ChatEvent::SlashCommand(SlashCommandEvent {
            command: "/deploy".into(),
            text: "prod".into(),
            channel_id: "c1".into(),
            user: sample_author(),
            trigger_id: None,
            adapter_name: "slack".into(),
        });
        assert!(matches!(event, ChatEvent::SlashCommand(_)));
    }

    #[test]
    fn chat_event_modal_submit_variant() {
        let event = ChatEvent::ModalSubmit(ModalSubmitEvent {
            callback_id: "feedback".into(),
            view_id: "v1".into(),
            user: sample_author(),
            values: HashMap::from([("rating".into(), "5".into())]),
            private_metadata: None,
            adapter_name: "slack".into(),
        });
        assert!(matches!(event, ChatEvent::ModalSubmit(_)));
    }

    #[test]
    fn chat_event_modal_close_variant() {
        let event = ChatEvent::ModalClose(ModalCloseEvent {
            callback_id: "feedback".into(),
            view_id: "v1".into(),
            user: sample_author(),
            adapter_name: "slack".into(),
        });
        assert!(matches!(event, ChatEvent::ModalClose(_)));
    }

    #[test]
    fn chat_event_clone_and_debug() {
        let event = ChatEvent::Action(ActionEvent {
            action_id: "a".into(),
            thread_id: "t".into(),
            message_id: "m".into(),
            user: sample_author(),
            value: None,
            trigger_id: None,
            adapter_name: "test".into(),
        });
        let cloned = event.clone();
        // Verify Debug output is non-empty.
        let dbg = format!("{cloned:?}");
        assert!(!dbg.is_empty());
    }
}
