# Design Document: Bob Chat Channels

| Metadata | Details |
| :--- | :--- |
| **Author** | pb-plan agent |
| **Status** | Draft |
| **Created** | 2026-03-03 |
| **Reviewers** | N/A |
| **Related Issues** | N/A |

## 1. Executive Summary

**Problem:** The current `bob-core::channel::Channel` trait provides only bare-minimum `recv`/`send` text I/O. Real chat platforms (CLI REPL, Telegram, Discord) need rich interaction primitives — streaming responses, interactive cards, button/dropdown actions, modals, slash commands, emoji, file uploads, direct messages, and ephemeral messages. There is no unified abstraction for these capabilities, so each future adapter would need to reinvent them.

**Solution:** Introduce a new library crate `crates/bob-chat` that defines a rich, Rust-native chat abstraction layer inspired by the vercel/chat SDK architecture. The crate extends the existing `Channel` concept in `bob-core` with new traits and types covering all 10 feature areas. Platform adapters (CLI REPL, Telegram, Discord) implement these traits in `bob-adapters` (or dedicated adapter crates). The design uses trait-based capability detection so adapters can declare which features they support, with graceful fallbacks for unsupported capabilities.

---

## 2. Requirements & Goals

### 2.1 Problem Statement

The existing `Channel` trait in `bob-core` only supports:

- `recv(&mut self) -> Option<ChannelMessage>` — receive plain text
- `send(&self, output: ChannelOutput) -> Result<(), ChannelError>` — send plain text with error flag

This is insufficient for building real chat experiences. There is no concept of threads, reactions, interactive UI elements, streaming, file attachments, or ephemeral messaging. Each chat platform (Telegram, Discord, CLI) has different capabilities, and without a unified abstraction layer, adapter authors must design ad-hoc solutions.

### 2.2 Functional Goals

1. **Event Handlers**: Define trait-based event handler registration for mentions, messages, reactions, button clicks, slash commands, and modal submissions.
2. **AI Streaming**: Support streaming LLM responses via `AsyncIterable<String>` with native streaming where available and post+edit fallback for platforms without native streaming.
3. **Cards**: Define a Rust-native card model (similar to Block Kit / Adaptive Cards) with sections, buttons, images, fields, dividers, and text elements. Platform adapters render cards to their native format.
4. **Actions**: Handle button clicks and dropdown selections with typed event payloads and action ID matching.
5. **Modals**: Define form dialog types with text inputs, selects, radio selects, and validation. Adapters open platform-native modals where supported.
6. **Slash Commands**: Handle `/command` invocations with command name, arguments text, and user context.
7. **Emoji**: Provide a type-safe, cross-platform emoji system with well-known emoji mappings and custom emoji support per platform.
8. **File Uploads**: Send and receive file attachments with typed metadata (filename, MIME type, size, binary data).
9. **Direct Messages**: Initiate DM conversations programmatically via adapter methods.
10. **Ephemeral Messages**: Send user-only visible messages with DM fallback when the platform lacks native ephemeral support.

### 2.3 Non-Functional Goals

- **Performance:** Card and modal types should be allocation-efficient; avoid unnecessary cloning. Streaming must be zero-copy for text deltas.
- **Extensibility:** All abstractions should be trait-based so new adapters can be added without modifying `bob-chat`.
- **Type Safety:** Use enums for well-known emoji, typed card/modal element trees, and strongly-typed event payloads.
- **Backward Compatibility:** The existing `Channel` trait in `bob-core` must remain intact. `bob-chat` extends it with new supertraits or companion traits; existing `Channel` impls need not change.

### 2.4 Out of Scope

- **Slack adapter**: Only CLI REPL, Telegram, and Discord are in scope.
- **Microsoft Teams / Google Chat / GitHub / Linear adapters**: Not in scope.
- **State adapter (Redis, etc.)**: Thread subscription state is in-memory for now; persistent state adapters are a follow-up.
- **JSX runtime**: The vercel/chat JSX card builder pattern is TypeScript-specific. We use a Rust builder pattern instead.
- **Webhook infrastructure**: Telegram/Discord adapters handle their own transport; no generic webhook server is provided by `bob-chat`.
- **Message history pagination**: Complex cursor-based message fetching is deferred; initial implementation provides a simpler recent-messages model.
- **Markdown AST rendering**: Platform-specific markdown conversion is adapter-internal; `bob-chat` defines a `PostableMessage` enum that includes markdown as a variant.

### 2.5 Assumptions

- **Assumption A1:** Telegram and Discord adapters will be implemented in `bob-adapters` behind feature flags, not as separate crates. This keeps the workspace compact.
- **Assumption A2:** The CLI REPL adapter will have limited support for rich features — no native cards, modals, or reactions — so it will use text-based fallbacks for most features.
- **Assumption A3:** Thread/channel subscription state (for `onSubscribedMessage`) will be kept in-memory using `scc::HashMap` per AGENTS.md concurrency preferences. Persistence is a follow-up.
- **Assumption A4:** The vercel/chat `Adapter` interface is the reference model, but we translate it to idiomatic Rust: traits instead of interfaces, `async_trait` instead of Promises, enums instead of union types.
- **Assumption A5:** Streaming uses `tokio_stream` / `futures_core::Stream` — consistent with the existing `LlmStream` pattern in `bob-core`.

---

## 3. Architecture Overview

### 3.1 System Context

```text
┌─────────────────────────────────────────────────────────────┐
│                      bob-chat (new crate)                   │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │  Types    │  │  Traits  │  │  Cards   │  │  Modals    │  │
│  │  (events, │  │(ChatAdap-│  │  (card   │  │ (modal     │  │
│  │   emoji,  │  │ ter,     │  │  element │  │  element   │  │
│  │   files)  │  │ Thread,  │  │  builder)│  │  builder)  │  │
│  │          │  │ Channel) │  │          │  │            │  │
│  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │
│                                                             │
│  ┌──────────────┐  ┌────────────────┐  ┌────────────────┐   │
│  │  ChatBot      │  │  EventRouter   │  │  Emoji         │  │
│  │  (orchestrator│  │  (dispatch     │  │  (well-known   │  │
│  │   + handler   │  │   handlers)    │  │   map + custom)│  │
│  │   registry)   │  │                │  │                │  │
│  └──────────────┘  └────────────────┘  └────────────────┘   │
└───────────────────────────┬─────────────────────────────────┘
                            │ depends on
                    ┌───────▼───────┐
                    │   bob-core    │
                    │  (Channel,    │
                    │   types,      │
                    │   errors)     │
                    └───────────────┘
                            │
                    ┌───────▼───────┐
                    │  bob-adapters │
                    │  + adapter    │
                    │  impls        │
                    │  (cli_repl,   │
                    │   telegram,   │
                    │   discord)    │
                    └───────────────┘
```

The `bob-chat` crate sits between `bob-core` (domain types) and `bob-adapters` (concrete impls). It:

1. Re-exports and extends `bob-core::channel::Channel`
2. Defines rich chat traits (`ChatAdapter`, `ChatThread`, `ChatChannel`)
3. Provides the `ChatBot` orchestrator that wires event handlers to adapters
4. Contains card/modal builders, emoji system, and all event/message types

### 3.2 Key Design Principles

1. **Trait-based capability detection:** Adapters implement traits for the features they support. The `ChatAdapter` trait has required methods (post, recv) and optional methods with default no-op/fallback implementations (stream, openModal, postEphemeral, openDM, addReaction).
2. **Platform-agnostic types, platform-specific rendering:** Card and modal elements are platform-neutral enums. Each adapter converts them to native format (Discord embeds, Telegram inline keyboards, CLI text tables).
3. **Handler-first ergonomics:** Users register handlers (`on_mention`, `on_message`, `on_action`, `on_slash_command`, `on_modal_submit`, `on_reaction`) on the `ChatBot` orchestrator. The bot dispatches incoming events to matching handlers.
4. **Streaming as first-class:** `PostableMessage` includes an `AsyncIterable<String>` variant. Adapters that support native streaming consume it directly; others use a post+edit loop with configurable update intervals.
5. **Graceful degradation:** Every rich feature has a text fallback for CLI. Ephemeral messages fall back to DM. Cards fall back to formatted text. Modals fall back to sequential prompts.

### 3.3 Existing Components to Reuse

| Component | Location | How to Reuse |
| :--- | :--- | :--- |
| `Channel` trait | `crates/bob-core/src/channel.rs` | Base transport trait; `ChatAdapter` extends the recv/send concept |
| `ChannelMessage` / `ChannelOutput` | `crates/bob-core/src/channel.rs` | Forward-compatible wrappers; `PostableMessage` is the rich superset |
| `ChannelError` | `crates/bob-core/src/channel.rs` | Extend with new variants for chat-specific errors (e.g., `ModalNotSupported`) |
| `AgentEvent` / `AgentStreamEvent` | `crates/bob-core/src/types.rs` | Reference pattern for streaming events; `ChatStreamEvent` follows the same model |
| `LlmStream` type alias | `crates/bob-core/src/types.rs` | Model for `TextStream` type in streaming |
| `SlashCommand` router | `crates/bob-runtime/src/router.rs` | Reference for command parsing; `bob-chat` defines its own `SlashCommandEvent` |
| `AgentLoop` | `crates/bob-runtime/src/agent_loop.rs` | Integration point; `ChatBot` will interact with `AgentLoop` for LLM processing |
| `scc` concurrent map | workspace dependency | Thread subscription state storage |
| `tokio_stream` | workspace dependency | Streaming primitives |
| `thiserror` | workspace dependency | Error types (library crate) |
| `serde` / `serde_json` | workspace dependency | Serialization for events, cards, modals |

---

## 4. Detailed Design

### 4.1 Module Structure

```text
crates/bob-chat/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs              # Crate root, re-exports
    ├── adapter.rs          # ChatAdapter trait
    ├── bot.rs              # ChatBot orchestrator + handler registry
    ├── card.rs             # Card element types + builders
    ├── emoji.rs            # Emoji system (well-known + custom)
    ├── error.rs            # ChatError types (thiserror)
    ├── event.rs            # Event types (Action, Reaction, SlashCommand, Modal, etc.)
    ├── file.rs             # FileUpload, Attachment types
    ├── message.rs          # PostableMessage, SentMessage, EphemeralMessage
    ├── modal.rs            # Modal element types + builders
    ├── stream.rs           # TextStream type, StreamOptions, fallback helpers
    ├── thread.rs           # ChatThread trait
    └── channel_ext.rs      # ChatChannel trait (extends Channel concept)
```

### 4.2 Data Structures & Types

#### 4.2.1 Core Message Types

```rust
/// A message that can be posted to a thread/channel.
pub enum PostableMessage {
    /// Raw text passed through as-is.
    Text(String),
    /// Markdown text, converted to platform format by adapter.
    Markdown(String),
    /// Rich card element.
    Card(CardElement),
    /// Streaming text from an async source (e.g., LLM).
    Stream(TextStream),
}

/// Sent message with edit/delete/react capabilities.
pub struct SentMessage {
    pub id: String,
    pub thread_id: String,
    pub adapter_name: String,
    // Additional raw platform data can be boxed:
    pub raw: Option<Box<dyn std::any::Any + Send + Sync>>,
}

/// Result of posting an ephemeral message.
pub struct EphemeralMessage {
    pub id: String,
    pub thread_id: String,
    pub used_fallback: bool,
}

/// File attachment metadata.
pub struct Attachment {
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
    pub data: Option<bytes::Bytes>,
    pub kind: AttachmentKind,
}

pub enum AttachmentKind {
    Image,
    File,
    Video,
    Audio,
}

/// File to upload with a message.
pub struct FileUpload {
    pub filename: String,
    pub mime_type: Option<String>,
    pub data: bytes::Bytes,
}
```

#### 4.2.2 Card Elements

```rust
/// Top-level card container.
pub struct CardElement {
    pub title: Option<String>,
    pub children: Vec<CardChild>,
    pub fallback_text: Option<String>,
}

pub enum CardChild {
    Section(SectionElement),
    Actions(ActionsElement),
    Divider,
    Image(ImageElement),
    Fields(FieldsElement),
    Text(TextElement),
}

pub struct SectionElement {
    pub text: Option<String>,
    pub accessory: Option<Box<CardChild>>,
}

pub struct ActionsElement {
    pub elements: Vec<ActionElement>,
}

pub enum ActionElement {
    Button(ButtonElement),
    // Future: Select, DatePicker, etc.
}

pub struct ButtonElement {
    pub id: String,
    pub text: String,
    pub value: Option<String>,
    pub style: ButtonStyle,
    pub url: Option<String>,
}

pub enum ButtonStyle { Default, Primary, Danger }

pub struct ImageElement {
    pub url: String,
    pub alt_text: String,
}

pub struct FieldsElement {
    pub fields: Vec<FieldElement>,
}

pub struct FieldElement {
    pub label: String,
    pub value: String,
}

pub struct TextElement {
    pub text: String,
    pub style: TextStyle,
}

pub enum TextStyle { Plain, Markdown }
```

#### 4.2.3 Modal Elements

```rust
pub struct ModalElement {
    pub callback_id: String,
    pub title: String,
    pub submit_label: Option<String>,
    pub children: Vec<ModalChild>,
    pub private_metadata: Option<String>,
    pub notify_on_close: bool,
}

pub enum ModalChild {
    TextInput(TextInputElement),
    Select(SelectElement),
    RadioSelect(RadioSelectElement),
}

pub struct TextInputElement {
    pub id: String,
    pub label: String,
    pub placeholder: Option<String>,
    pub initial_value: Option<String>,
    pub multiline: bool,
    pub optional: bool,
}

pub struct SelectElement {
    pub id: String,
    pub label: String,
    pub placeholder: Option<String>,
    pub options: Vec<SelectOption>,
    pub initial_option: Option<String>,
}

pub struct SelectOption {
    pub label: String,
    pub value: String,
}

pub struct RadioSelectElement {
    pub id: String,
    pub label: String,
    pub options: Vec<SelectOption>,
    pub initial_option: Option<String>,
}
```

#### 4.2.4 Emoji System

```rust
/// Well-known emoji that map across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WellKnownEmoji {
    ThumbsUp, ThumbsDown, Heart, Smile, Laugh,
    Thinking, Fire, Star, Sparkles, Check, X,
    Warning, Rocket, Eyes, Wave, Clap,
    // ... (complete list)
}

/// Platform-specific emoji format mappings.
pub struct EmojiFormats {
    pub discord: String,    // e.g. "👍" or ":thumbsup:"
    pub telegram: String,   // e.g. "👍"
    pub cli: String,        // e.g. "[thumbs_up]"
}

/// Emoji value object with platform resolution.
#[derive(Debug, Clone)]
pub struct EmojiValue {
    pub name: String,
    formats: EmojiFormats,
}

/// Emoji resolver trait for custom emoji support.
pub trait EmojiResolver: Send + Sync {
    fn resolve(&self, name: &str, platform: &str) -> Option<String>;
}
```

#### 4.2.5 Event Types

```rust
/// Author information common to all events.
pub struct Author {
    pub user_id: String,
    pub user_name: String,
    pub full_name: String,
    pub is_bot: bool,
}

/// Action event (button click / dropdown selection).
pub struct ActionEvent {
    pub action_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub user: Author,
    pub value: Option<String>,
    pub trigger_id: Option<String>,
    pub adapter_name: String,
}

/// Reaction event.
pub struct ReactionEvent {
    pub thread_id: String,
    pub message_id: String,
    pub user: Author,
    pub emoji: EmojiValue,
    pub added: bool,
    pub adapter_name: String,
}

/// Slash command event.
pub struct SlashCommandEvent {
    pub command: String,
    pub text: String,
    pub channel_id: String,
    pub user: Author,
    pub trigger_id: Option<String>,
    pub adapter_name: String,
}

/// Modal submit event.
pub struct ModalSubmitEvent {
    pub callback_id: String,
    pub view_id: String,
    pub user: Author,
    pub values: std::collections::HashMap<String, String>,
    pub private_metadata: Option<String>,
    pub adapter_name: String,
}

/// Modal close event.
pub struct ModalCloseEvent {
    pub callback_id: String,
    pub view_id: String,
    pub user: Author,
    pub adapter_name: String,
}

/// Enum covering all incoming chat events.
pub enum ChatEvent {
    Message { thread_id: String, message: IncomingMessage },
    Mention { thread_id: String, message: IncomingMessage },
    Reaction(ReactionEvent),
    Action(ActionEvent),
    SlashCommand(SlashCommandEvent),
    ModalSubmit(ModalSubmitEvent),
    ModalClose(ModalCloseEvent),
}

/// Parsed incoming message from a platform.
pub struct IncomingMessage {
    pub id: String,
    pub text: String,
    pub author: Author,
    pub attachments: Vec<Attachment>,
    pub is_mention: bool,
    pub thread_id: String,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}
```

### 4.3 Interface Design

#### 4.3.1 `ChatAdapter` Trait

```rust
/// Core adapter trait — the main extension point for platform integrations.
/// Each platform (CLI, Telegram, Discord) implements this.
#[async_trait::async_trait]
pub trait ChatAdapter: Send + Sync {
    /// Unique adapter name (e.g. "cli_repl", "telegram", "discord").
    fn name(&self) -> &str;

    // ── Required Methods ────────────────────

    /// Post a message to a thread.
    async fn post_message(
        &self,
        thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError>;

    /// Edit an existing message.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError>;

    /// Delete a message.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> Result<(), ChatError>;

    /// Render a card to platform-native format (text fallback by default).
    fn render_card(&self, card: &CardElement) -> String;

    /// Render a PostableMessage to adapter-native form (text extraction).
    fn render_message(&self, message: &AdapterPostableMessage) -> String;

    /// Poll/receive next incoming event from this adapter.
    async fn recv_event(&mut self) -> Option<ChatEvent>;

    // ── Optional Methods (with defaults) ────

    /// Stream text to a thread using platform-native streaming.
    /// Default: post + edit fallback loop.
    async fn stream(
        &self,
        thread_id: &str,
        text_stream: TextStream,
        options: &StreamOptions,
    ) -> Result<SentMessage, ChatError> {
        // Default implementation: post+edit fallback
        // (provided by bob-chat)
    }

    /// Add a reaction to a message.
    async fn add_reaction(
        &self, _thread_id: &str, _message_id: &str, _emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("reactions".into()))
    }

    /// Remove a reaction from a message.
    async fn remove_reaction(
        &self, _thread_id: &str, _message_id: &str, _emoji: &EmojiValue,
    ) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("reactions".into()))
    }

    /// Open a DM conversation with a user.
    async fn open_dm(&self, _user_id: &str) -> Result<String, ChatError> {
        Err(ChatError::NotSupported("direct messages".into()))
    }

    /// Post an ephemeral message.
    async fn post_ephemeral(
        &self, _thread_id: &str, _user_id: &str, _message: &AdapterPostableMessage,
    ) -> Result<EphemeralMessage, ChatError> {
        Err(ChatError::NotSupported("ephemeral messages".into()))
    }

    /// Open a modal dialog.
    async fn open_modal(
        &self, _trigger_id: &str, _modal: &ModalElement, _context_id: Option<&str>,
    ) -> Result<String, ChatError> {
        Err(ChatError::NotSupported("modals".into()))
    }

    /// Show a typing indicator.
    async fn start_typing(&self, _thread_id: &str, _status: Option<&str>) -> Result<(), ChatError> {
        Ok(()) // no-op default
    }

    /// Upload a file to a thread.
    async fn upload_file(
        &self, _thread_id: &str, _file: &FileUpload,
    ) -> Result<SentMessage, ChatError> {
        Err(ChatError::NotSupported("file uploads".into()))
    }
}
```

#### 4.3.2 `AdapterPostableMessage` (non-streaming subset)

```rust
/// Synchronous message content for adapter methods (excludes streams).
pub enum AdapterPostableMessage {
    Text(String),
    Markdown(String),
    Card(CardElement),
    WithAttachments {
        content: Box<AdapterPostableMessage>,
        attachments: Vec<Attachment>,
        files: Vec<FileUpload>,
    },
}
```

#### 4.3.3 `ChatBot` Orchestrator

```rust
/// The central hub that wires adapters to event handlers.
pub struct ChatBot {
    adapters: Vec<Box<dyn ChatAdapter>>,
    mention_handlers: Vec<Box<dyn Fn(ThreadHandle, IncomingMessage) -> BoxFuture<'static, ()> + Send + Sync>>,
    message_handlers: Vec<(Option<regex::Regex>, Box<dyn Fn(ThreadHandle, IncomingMessage) -> BoxFuture<'static, ()> + Send + Sync>)>,
    subscribed_message_handlers: Vec<Box<dyn Fn(ThreadHandle, IncomingMessage) -> BoxFuture<'static, ()> + Send + Sync>>,
    action_handlers: Vec<(Option<Vec<String>>, Box<dyn Fn(ActionEvent, ThreadHandle) -> BoxFuture<'static, ()> + Send + Sync>)>,
    reaction_handlers: Vec<(Option<Vec<EmojiValue>>, Box<dyn Fn(ReactionEvent) -> BoxFuture<'static, ()> + Send + Sync>)>,
    slash_command_handlers: Vec<(Option<Vec<String>>, Box<dyn Fn(SlashCommandEvent) -> BoxFuture<'static, ()> + Send + Sync>)>,
    modal_submit_handlers: Vec<(Option<Vec<String>>, Box<dyn Fn(ModalSubmitEvent) -> BoxFuture<'static, ()> + Send + Sync>)>,
    modal_close_handlers: Vec<Box<dyn Fn(ModalCloseEvent) -> BoxFuture<'static, ()> + Send + Sync>>,
    subscriptions: Arc<scc::HashMap<String, ()>>,
}
```

Key methods on `ChatBot`:

- `on_mention(handler)` — register mention handler
- `on_message(pattern, handler)` — register regex-matched message handler
- `on_subscribed_message(handler)` — register subscribed thread handler
- `on_action(action_ids, handler)` — register action handler
- `on_reaction(emoji_list, handler)` — register reaction handler
- `on_slash_command(commands, handler)` — register slash command handler
- `on_modal_submit(callback_ids, handler)` — register modal submit handler
- `on_modal_close(handler)` — register modal close handler
- `run(&mut self)` — start the event loop (polls all adapters, dispatches events)

#### 4.3.4 `ThreadHandle`

A convenience wrapper passed to handlers, providing a high-level API:

```rust
/// Handle to a thread, providing post/stream/ephemeral/typing/subscribe APIs.
pub struct ThreadHandle {
    thread_id: String,
    adapter: Arc<dyn ChatAdapter>,
    subscriptions: Arc<scc::HashMap<String, ()>>,
}

impl ThreadHandle {
    pub async fn post(&self, message: impl Into<PostableMessage>) -> Result<SentMessage, ChatError>;
    pub async fn post_ephemeral(&self, user_id: &str, message: impl Into<AdapterPostableMessage>, fallback_to_dm: bool) -> Result<Option<EphemeralMessage>, ChatError>;
    pub async fn start_typing(&self, status: Option<&str>) -> Result<(), ChatError>;
    pub async fn subscribe(&self) -> Result<(), ChatError>;
    pub async fn unsubscribe(&self) -> Result<(), ChatError>;
    pub fn mention_user(&self, user_id: &str) -> String;
}
```

### 4.4 Logic Flow

#### Event Dispatch Loop

```text
ChatBot::run()
  │
  ├─ For each adapter in parallel (tokio::select! / futures::select_all):
  │    │
  │    └─ adapter.recv_event() → ChatEvent
  │         │
  │         ├─ ChatEvent::Mention → check subscriptions:
  │         │    ├─ subscribed? → dispatch to on_subscribed_message handlers
  │         │    └─ not subscribed? → dispatch to on_mention handlers
  │         │
  │         ├─ ChatEvent::Message → check subscriptions:
  │         │    ├─ subscribed? → dispatch to on_subscribed_message handlers
  │         │    └─ not subscribed? → match on_message regex patterns → dispatch
  │         │
  │         ├─ ChatEvent::Reaction → match emoji filter → dispatch on_reaction
  │         ├─ ChatEvent::Action → match action_id → dispatch on_action
  │         ├─ ChatEvent::SlashCommand → match command name → dispatch on_slash_command
  │         ├─ ChatEvent::ModalSubmit → match callback_id → dispatch on_modal_submit
  │         └─ ChatEvent::ModalClose → dispatch on_modal_close
```

#### Streaming Flow (Post + Edit Fallback)

```text
thread.post(PostableMessage::Stream(text_stream))
  │
  ├─ Adapter supports native streaming?
  │    └─ Yes → adapter.stream(thread_id, text_stream, options)
  │
  └─ No → Fallback:
       1. adapter.post_message(thread_id, placeholder "...")
       2. Loop: collect chunks from text_stream
       3. Every N ms (configurable): adapter.edit_message(thread_id, msg_id, accumulated_text)
       4. On stream complete: final edit_message with complete text
```

### 4.5 Configuration

No new configuration files. Adapter-specific config (bot tokens, etc.) is provided at adapter construction time by the binary crate. `ChatBot` itself takes:

- `ChatBotConfig`:
  - `streaming_update_interval_ms: u64` (default: 500)
  - `fallback_streaming_placeholder: Option<String>` (default: `Some("...".into())`)

### 4.6 Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("adapter error: {0}")]
    Adapter(String),

    #[error("feature not supported: {0}")]
    NotSupported(String),

    #[error("message not found: {0}")]
    MessageNotFound(String),

    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("modal error: {0}")]
    Modal(String),

    #[error("channel closed")]
    Closed,

    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}
```

Follows AGENTS.md: library crate uses `thiserror`.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Testing

- Card/modal builder output (element tree construction).
- Emoji resolution for each platform.
- Event routing logic (handler matching by action_id, command, regex).
- Streaming fallback accumulation logic.
- Message rendering (card → text fallback).
- All tests colocated with implementation (`#[cfg(test)]`).

### 5.2 Integration Testing

- `MockChatAdapter` (in `bob-chat` test support) that records all calls and returns canned responses.
- Full `ChatBot` lifecycle: register handlers → inject events → verify handler invocation and adapter calls.
- Streaming integration: mock adapter with post+edit tracking, verify edit cadence.

### 5.3 Critical Path Verification (The "Harness")

| Verification Step | Command | Success Criteria |
| :--- | :--- | :--- |
| **VP-01** | `cargo check --all-targets --all-features` | All targets compile cleanly |
| **VP-02** | `cargo test --all-features` | All tests pass |
| **VP-03** | `just lint` | No lint errors or warnings |
| **VP-04** | `just format` | Code is formatted |
| **VP-05** | `cargo doc --no-deps -p bob-chat` | Documentation builds without warnings |

### 5.4 Validation Rules

| Test Case ID | Action | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | Register `on_mention` handler, inject `ChatEvent::Mention` to MockAdapter | Handler is called with correct thread and message | Unit test assertion |
| **TC-02** | Post `PostableMessage::Card` to MockAdapter | `render_card` called, `post_message` receives rendered output | Unit test assertion on mock |
| **TC-03** | Post `PostableMessage::Stream` to non-streaming MockAdapter | Fallback: initial post + N edits + final edit | Count post/edit calls in mock |
| **TC-04** | Call `thread.post_ephemeral` on adapter without native support, `fallback_to_dm=true` | `open_dm` called, then `post_message` to DM thread | Mock call sequence assertion |
| **TC-05** | Register `on_slash_command("/test")`, inject SlashCommand with `/test arg1 arg2` | Handler receives command="/test", text="arg1 arg2" | Unit test assertion |
| **TC-06** | Register `on_action("approve")`, inject Action with action_id="approve" | Handler called with correct action event | Unit test assertion |
| **TC-07** | Emoji resolve `WellKnownEmoji::ThumbsUp` for discord/telegram/cli | Returns correct platform string | Unit test per platform |
| **TC-08** | Register `on_modal_submit("feedback")`, inject ModalSubmit | Handler called with values map | Unit test assertion |
| **TC-09** | Post file via `upload_file` on MockAdapter | `upload_file` called with correct FileUpload | Mock assertion |
| **TC-10** | Register `on_reaction([ThumbsUp])`, inject Reaction with ThumbsDown | Handler NOT called (emoji filter mismatch) | Unit test assertion |

---

## 6. Implementation Plan

- [ ] **Phase 1: Foundation** — Create `bob-chat` crate, core types, error types, emoji system
- [ ] **Phase 2: Core Logic** — `ChatAdapter` trait, card/modal types, event types, message types, streaming helpers
- [ ] **Phase 3: Integration** — `ChatBot` orchestrator, event routing, `ThreadHandle`, handler registration + dispatch
- [ ] **Phase 4: Polish** — Unit tests, integration tests, MockAdapter, documentation, lint/format verification

---

## 7. Cross-Functional Concerns

- **Backward Compatibility:** The existing `Channel` trait in `bob-core` is not modified. `bob-chat` introduces new abstractions alongside it. Migration from `Channel` to `ChatAdapter` in existing code (e.g., CLI REPL) is a separate follow-up task.
- **Security:** Bot tokens for Telegram/Discord are provided at construction time and never logged. The `tracing` instrumentation must not capture token values.
- **Observability:** Event dispatch and handler invocations should emit `tracing` spans for debuggability.
- **Thread Safety:** `ChatBot` and all adapters must be `Send + Sync`. Handler closures are `Send + Sync`. Subscription state uses `scc::HashMap` (lock-free).
