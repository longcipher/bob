# bob-chat

Platform-agnostic chat abstractions for the [Bob Agent Framework](https://github.com/longcipher/bob).

## Overview

`bob-chat` provides the structured chat layer that sits between chat platform APIs (Slack, Discord, Telegram, CLI, etc.) and the agent loop. It ships a trait-based adapter model, a central event-driven orchestrator (`ChatBot`), and rich message primitives.

## Features

1. **`ChatAdapter` trait** — implement once per platform; covers posting, editing, deleting, reactions, ephemeral messages, DMs, modals, file uploads, and event streaming.
2. **`ChatBot` orchestrator** — register adapters and typed handlers, then call `ChatBot::run()` to poll all adapters concurrently and dispatch events.
3. **`ChatEvent` enum** — seven event variants: Message, Mention, Reaction, Action, SlashCommand, ModalSubmit, ModalClose.
4. **`ThreadHandle`** — scoped handle passed to handlers for replying, subscribing to threads, and managing ephemeral messages with DM fallback.
5. **Rich messages (Cards)** — `CardElement` with buttons, sections, images, and fields, plus a plain-text fallback renderer.
6. **Modal dialogs** — `ModalElement` with text inputs, selects, and radio selects.
7. **Emoji system** — 35 `WellKnownEmoji` variants with platform-specific format maps and custom-emoji support.
8. **Streaming** — `TextStream` and `fallback_stream` for progressive message delivery via post-then-edit.
9. **Attachments** — `Attachment` and `FileUpload` types for file handling.
10. **Error types** — `ChatError` with adapter, transport, rate-limit, auth, not-found, not-supported, and closed variants.

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│                     ChatBot                         │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐         │
│  │ Adapter A│  │ Adapter B│  │ Adapter C│  ...     │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘         │
│       │              │              │               │
│       └──────┬───────┴──────┬───────┘               │
│              │ select_all   │                       │
│              ▼              ▼                       │
│         ┌─────────────────────┐                     │
│         │   Event Dispatch    │                     │
│         │  ┌───────────────┐  │                     │
│         │  │ Mention       │──│──► MentionHandler   │
│         │  │ Message       │──│──► MessageHandler   │
│         │  │ Reaction      │──│──► ReactionHandler  │
│         │  │ Action        │──│──► ActionHandler    │
│         │  │ SlashCommand  │──│──► CommandHandler   │
│         │  │ ModalSubmit   │──│──► SubmitHandler    │
│         │  │ ModalClose    │──│──► CloseHandler     │
│         │  └───────────────┘  │                     │
│         └─────────────────────┘                     │
│                                                     │
│  Handlers receive ThreadHandle for replies,         │
│  subscriptions, and streaming.                      │
└─────────────────────────────────────────────────────┘
```

## Usage

```rust,no_run
use bob_chat::{ChatBot, ChatBotConfig};

#[tokio::main]
async fn main() -> Result<(), bob_chat::ChatError> {
    let mut bot = ChatBot::new(ChatBotConfig::default());

    // React to mentions.
    bot.on_mention(|thread, message| async move {
        let _ = thread.post("Hello! You said: ".to_owned() + &message.text).await;
    });

    // React to messages containing "help".
    bot.on_message(Some("help".into()), |thread, _message| async move {
        let _ = thread.post("How can I help you?").await;
    });

    // Add platform adapters and start.
    // bot.add_adapter(my_slack_adapter);
    // bot.run().await?;

    Ok(())
}
```

## License

See [LICENSE.md](../../LICENSE.md) in the repository root.
