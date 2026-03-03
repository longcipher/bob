# Bob Chat Channels — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-03-03-01-bob-chat-channels/design.md |
| **Owner** | N/A |
| **Start Date** | 2026-03-03 |
| **Target Date** | 2026-03-14 |
| **Status** | Planning |

## Summary & Phasing

Create the `bob-chat` library crate with a rich, trait-based chat abstraction layer inspired by vercel/chat. Foundation → core types and traits → orchestrator with event routing → tests and documentation.

- **Phase 1: Foundation & Scaffolding** — Crate setup, error types, emoji system, basic message types
- **Phase 2: Core Logic** — ChatAdapter trait, card/modal element types, event types, streaming helpers
- **Phase 3: Integration & Features** — ChatBot orchestrator, ThreadHandle, event dispatch loop, handler registration
- **Phase 4: Polish, QA & Docs** — Unit tests, integration tests, MockAdapter, documentation, lint/format pass

---

## Phase 1: Foundation & Scaffolding

### Task 1.1: Create bob-chat Crate Skeleton

> **Context:** The new crate needs to exist in the workspace before any types can be defined. Must follow workspace conventions: `workspace = true` for version/edition, `cargo add` for dependencies. The crate lives at `crates/bob-chat/` alongside `bob-core`, `bob-runtime`, and `bob-adapters`.
> **Verification:** `cargo check -p bob-chat` succeeds with an empty lib.

- **Priority:** P0
- **Scope:** Crate scaffolding
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/Cargo.toml` with `[package]` using `version.workspace = true`, `edition.workspace = true`, and `[lints] workspace = true`.
- [x] **Step 2:** Create `crates/bob-chat/src/lib.rs` with a crate-level doc comment and empty module declarations.
- [x] **Step 3:** Add `bob-chat` to root `Cargo.toml` workspace dependencies: `bob-chat = { path = "crates/bob-chat", version = "0.2.0" }`.
- [x] **Step 4:** Add workspace dependencies to `bob-chat` using `cargo add`: `bob-core`, `thiserror`, `serde`, `serde_json`, `async-trait`, `tokio`, `tokio-stream`, `futures-core`, `tracing`, `bytes`.
- [x] **Step 5:** Create `crates/bob-chat/README.md` with a short crate description.
- [x] **Verification:** `cargo check -p bob-chat` compiles cleanly.

---

### Task 1.2: Define ChatError Types

> **Context:** All subsequent modules need error types. Following AGENTS.md: library crate uses `thiserror`. Mirrors patterns from `bob-core::error`.
> **Verification:** Error types compile, `From` conversions work, `Display` output is readable.

- **Priority:** P0
- **Scope:** Error module
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/error.rs` with `ChatError` enum: `Adapter(String)`, `NotSupported(String)`, `MessageNotFound(String)`, `SendFailed(String)`, `Modal(String)`, `Closed`, `Internal(Box<dyn Error + Send + Sync>)`.
- [x] **Step 2:** Add `#[derive(Debug, thiserror::Error)]` and appropriate `#[error(...)]` attributes.
- [x] **Step 3:** Export from `lib.rs`.
- [x] **Step 4:** Add unit tests for `Display` formatting and `From` conversion.
- [x] **Verification:** `cargo test -p bob-chat` passes error tests.

---

### Task 1.3: Define Core Message Types

> **Context:** `PostableMessage`, `AdapterPostableMessage`, `SentMessage`, `EphemeralMessage`, `IncomingMessage`, `Author`, `Attachment`, `FileUpload` are foundational types used by all other modules.
> **Verification:** Types compile, derive `Serialize`/`Deserialize` where appropriate, and have `Debug`.

- **Priority:** P0
- **Scope:** Message and file types
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/message.rs` with `PostableMessage`, `AdapterPostableMessage`, `SentMessage`, `EphemeralMessage`, `IncomingMessage`, `Author` types as specified in design §4.2.1 and §4.2.5.
- [x] **Step 2:** Create `crates/bob-chat/src/file.rs` with `Attachment`, `AttachmentKind`, `FileUpload` types as specified in design §4.2.1.
- [x] **Step 3:** Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all appropriate types. `SentMessage.raw` field uses `Option<Box<dyn Any + Send + Sync>>` and skips serde.
- [x] **Step 4:** Export from `lib.rs`.
- [x] **Verification:** `cargo check -p bob-chat` compiles cleanly.

---

### Task 1.4: Define Emoji System

> **Context:** The emoji module provides type-safe, cross-platform emoji with well-known mappings and custom emoji support. Used by reactions, message posting, and handler filtering. No dependency on card or event types.
> **Verification:** `WellKnownEmoji` resolves to correct strings for discord/telegram/cli. Custom emoji resolver trait works.

- **Priority:** P1
- **Scope:** Emoji module
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/emoji.rs` with `WellKnownEmoji` enum (at least 30 common emoji: thumbs_up/down, heart, smile, laugh, thinking, fire, star, sparkles, check, x, warning, rocket, eyes, wave, clap, etc.).
- [x] **Step 2:** Define `EmojiFormats` struct with `discord: String, telegram: String, cli: String` fields.
- [x] **Step 3:** Define `EmojiValue` struct and implement `Display` (returns placeholder `:{name}:`) and `PartialEq`/`Eq`/`Hash`.
- [x] **Step 4:** Build `DEFAULT_EMOJI_MAP: &[(WellKnownEmoji, EmojiFormats)]` with platform mappings.
- [x] **Step 5:** Define `EmojiResolver` trait with `fn resolve(&self, name: &str, platform: &str) -> Option<String>`.
- [x] **Step 6:** Implement `DefaultEmojiResolver` that uses `DEFAULT_EMOJI_MAP`.
- [x] **Step 7:** Export from `lib.rs`.
- [x] **Step 8:** Add unit tests: resolve known emoji for each platform, unknown emoji returns None, Display formatting.
- [x] **Verification:** `cargo test -p bob-chat` passes emoji tests.

---

## Phase 2: Core Logic

### Task 2.1: Define Card Element Types and Builders

> **Context:** Cards are interactive rich messages (buttons, sections, images, fields). Platform adapters render them to native format. The types are used by `ChatAdapter::render_card` and `PostableMessage::Card`. Design §4.2.2.
> **Verification:** Card element tree can be constructed via builders, `Debug`/`Clone`/`Serialize` work.

- **Priority:** P0
- **Scope:** Card types and builder API
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/card.rs` with all card types: `CardElement`, `CardChild`, `SectionElement`, `ActionsElement`, `ActionElement`, `ButtonElement`, `ButtonStyle`, `ImageElement`, `FieldsElement`, `FieldElement`, `TextElement`, `TextStyle`.
- [x] **Step 2:** Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all types.
- [x] **Step 3:** Add builder methods on `CardElement`: `CardElement::new()`, `.title()`, `.section()`, `.actions()`, `.divider()`, `.image()`, `.fields()`, `.text()`. Use method chaining pattern.
- [x] **Step 4:** Add builder methods on `ButtonElement`: `ButtonElement::new(id, text)`, `.value()`, `.style()`, `.url()`.
- [x] **Step 5:** Implement `render_card_as_text(card: &CardElement) -> String` helper that produces a plain-text fallback representation.
- [x] **Step 6:** Export from `lib.rs`.
- [x] **Step 7:** Add unit tests for builder API and text fallback rendering.
- [x] **Verification:** `cargo test -p bob-chat` passes card tests.

---

### Task 2.2: Define Modal Element Types and Builders

> **Context:** Modals are form dialogs with text inputs, selects, radio selects. Used by `ChatAdapter::open_modal` and modal event handlers. Design §4.2.3.
> **Verification:** Modal element tree can be constructed, serialize/deserialize roundtrips.

- **Priority:** P0
- **Scope:** Modal types and builder API
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/modal.rs` with all modal types: `ModalElement`, `ModalChild`, `TextInputElement`, `SelectElement`, `SelectOption`, `RadioSelectElement`.
- [x] **Step 2:** Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all types.
- [x] **Step 3:** Add builder methods: `ModalElement::new(callback_id, title)`, `.submit_label()`, `.text_input()`, `.select()`, `.radio_select()`, `.private_metadata()`, `.notify_on_close()`.
- [x] **Step 4:** Add builder methods for input elements: `TextInputElement::new(id, label)`, `.placeholder()`, `.initial_value()`, `.multiline()`, `.optional()`.
- [x] **Step 5:** Export from `lib.rs`.
- [x] **Step 6:** Add unit tests for builder API and serde roundtrip.
- [x] **Verification:** `cargo test -p bob-chat` passes modal tests.

---

### Task 2.3: Define Event Types

> **Context:** Events are the incoming payloads from adapters: messages, mentions, reactions, actions, slash commands, modal submits/closes. Used by `ChatBot` event routing and handler dispatch. Design §4.2.5.
> **Verification:** All event types compile with `Debug`/`Clone`, `ChatEvent` enum covers all variants.

- **Priority:** P0
- **Scope:** Event types
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/event.rs` with `ChatEvent` enum and all event structs: `ActionEvent`, `ReactionEvent`, `SlashCommandEvent`, `ModalSubmitEvent`, `ModalCloseEvent` as specified in design §4.2.5.
- [x] **Step 2:** Derive `Debug`, `Clone` on all types. `Serialize`/`Deserialize` on value types only (not those containing `Any` or platform-specific data).
- [x] **Step 3:** Export from `lib.rs`.
- [x] **Verification:** `cargo check -p bob-chat` compiles cleanly.

---

### Task 2.4: Define ChatAdapter Trait

> **Context:** The `ChatAdapter` trait is the core extension point. Each platform implements it. Required methods: `name`, `post_message`, `edit_message`, `delete_message`, `render_card`, `render_message`, `recv_event`. Optional methods with defaults: `stream`, `add_reaction`, `remove_reaction`, `open_dm`, `post_ephemeral`, `open_modal`, `start_typing`, `upload_file`. Design §4.3.1.
> **Verification:** Trait compiles, can be used as `dyn ChatAdapter`, default method implementations are correct.

- **Priority:** P0
- **Scope:** Core adapter trait
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/adapter.rs` with the `ChatAdapter` trait using `#[async_trait]`.
- [x] **Step 2:** Define all required methods: `name`, `post_message`, `edit_message`, `delete_message`, `render_card`, `render_message`, `recv_event`.
- [x] **Step 3:** Define all optional methods with default implementations: `stream` (post+edit fallback stub), `add_reaction`, `remove_reaction`, `open_dm`, `post_ephemeral`, `open_modal`, `start_typing`, `upload_file`.
- [x] **Step 4:** Ensure trait is object-safe (`dyn ChatAdapter` compiles).
- [x] **Step 5:** Export from `lib.rs`.
- [x] **Verification:** `cargo check -p bob-chat` compiles. A trivial mock impl compiles against the trait.

---

### Task 2.5: Implement Streaming Helpers

> **Context:** Streaming LLM responses is a core feature. The `TextStream` type wraps `Pin<Box<dyn Stream<Item = String> + Send>>`. The fallback streaming logic (post+edit loop) is provided as a default implementation and a standalone helper function. Design §4.4.
> **Verification:** Fallback streaming logic collects chunks and calls edit at configured intervals.

- **Priority:** P0
- **Scope:** Streaming module
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/stream.rs` with `TextStream` type alias: `Pin<Box<dyn Stream<Item = String> + Send>>`.
- [x] **Step 2:** Define `StreamOptions` struct: `update_interval_ms: u64` (default 500), `placeholder_text: Option<String>` (default `Some("...".into())`).
- [x] **Step 3:** Implement `pub async fn fallback_stream(adapter: &dyn ChatAdapter, thread_id: &str, text_stream: TextStream, options: &StreamOptions) -> Result<SentMessage, ChatError>` that: (a) posts initial placeholder, (b) collects chunks with interval-based edits, (c) final edit with complete text.
- [x] **Step 4:** Wire `fallback_stream` as the default `ChatAdapter::stream` implementation.
- [x] **Step 5:** Export from `lib.rs`.
- [x] **Step 6:** Add unit test using a mock adapter that tracks post/edit call counts and validates timing behavior (use `tokio::time::pause()`).
- [x] **Verification:** `cargo test -p bob-chat` passes streaming tests.

---

## Phase 3: Integration & Features

### Task 3.1: Implement ChatBot Orchestrator — Handler Registration

> **Context:** `ChatBot` is the central hub. Users register event handlers on it. This task covers construction and handler registration methods only (dispatch comes in Task 3.2). Design §4.3.3. Uses `scc::HashMap` for subscriptions per AGENTS.md concurrency preferences.
> **Verification:** Handlers can be registered via type-safe methods. `ChatBot` compiles as `Send + Sync`.

- **Priority:** P0
- **Scope:** ChatBot struct and registration API
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/bot.rs` with `ChatBot` struct holding adapter vec, handler vecs, and `Arc<scc::HashMap<String, ()>>` for subscriptions.
- [x] **Step 2:** Define `ChatBotConfig` struct with `streaming_update_interval_ms` and `fallback_streaming_placeholder`.
- [x] **Step 3:** Implement `ChatBot::new(config: ChatBotConfig) -> Self`.
- [x] **Step 4:** Implement `add_adapter(&mut self, adapter: impl ChatAdapter + 'static)`.
- [x] **Step 5:** Implement handler registration methods: `on_mention`, `on_message` (with optional regex), `on_subscribed_message`, `on_action` (with optional action_id filter), `on_reaction` (with optional emoji filter), `on_slash_command` (with optional command filter), `on_modal_submit` (with optional callback_id filter), `on_modal_close`.
- [x] **Step 6:** Export from `lib.rs`.
- [x] **Verification:** `cargo check -p bob-chat` compiles. `ChatBot` is `Send + Sync` (static assert).

---

### Task 3.2: Implement ChatBot Event Dispatch Loop

> **Context:** The `run` method polls all adapters for events and dispatches to registered handlers. Uses `tokio::select!` or `futures::select_all` to poll adapters concurrently. Checks subscription state to route messages to correct handlers. Design §4.4.
> **Verification:** Events from mock adapters are dispatched to correct handlers.

- **Priority:** P0
- **Scope:** Event dispatch loop
- **Status:** � DONE

- [x] **Step 1:** Implement `ChatBot::run(&mut self) -> Result<(), ChatError>` that loops, polling `recv_event` from all adapters concurrently.
- [x] **Step 2:** Implement event routing: `ChatEvent::Mention` → check subscriptions → dispatch to `on_mention` or `on_subscribed_message` handlers.
- [x] **Step 3:** Implement `ChatEvent::Message` routing: check subscriptions → `on_subscribed_message` or regex-match → `on_message`.
- [x] **Step 4:** Implement `ChatEvent::Reaction` routing: filter by emoji list → dispatch `on_reaction`.
- [x] **Step 5:** Implement `ChatEvent::Action` routing: filter by action_id → dispatch `on_action`.
- [x] **Step 6:** Implement `ChatEvent::SlashCommand` routing: filter by command name → dispatch `on_slash_command`.
- [x] **Step 7:** Implement `ChatEvent::ModalSubmit` / `ChatEvent::ModalClose` routing: filter by callback_id → dispatch.
- [x] **Step 8:** Add `tracing` spans for each event dispatch for observability.
- [x] **Verification:** Integration test with MockAdapter: inject events, verify correct handlers called.

---

### Task 3.3: Implement ThreadHandle

> **Context:** `ThreadHandle` is the high-level API passed to handlers, wrapping adapter calls with convenience methods. Handles the full `PostableMessage` including streams, ephemeral with DM fallback, subscriptions. Design §4.3.4.
> **Verification:** `ThreadHandle::post` with stream triggers fallback, ephemeral with fallback triggers `open_dm`.

- **Priority:** P0
- **Scope:** ThreadHandle wrapper
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/src/thread.rs` with `ThreadHandle` struct.
- [x] **Step 2:** Implement `post(message: impl Into<PostableMessage>)`: dispatch to `adapter.post_message` for text/markdown/card, `adapter.stream` for streams.
- [x] **Step 3:** Implement `post_ephemeral(user_id, message, fallback_to_dm)`: try `adapter.post_ephemeral`, on `NotSupported` + `fallback_to_dm=true` → `adapter.open_dm(user_id)` then `adapter.post_message(dm_thread)`.
- [x] **Step 4:** Implement `start_typing`, `subscribe`, `unsubscribe` (using `scc::HashMap`), `mention_user`.
- [x] **Step 5:** Export from `lib.rs`.
- [x] **Verification:** Unit tests for post routing, ephemeral fallback logic, subscribe/unsubscribe state.

---

### Task 3.4: Extend bob-core Channel Trait (Minimal)

> **Context:** Add a new `ChatChannel` marker trait or extend `ChannelError` with a `NotSupported` variant so adapters can indicate unsupported features cleanly. The existing `Channel` trait is NOT modified.
> **Verification:** Existing `Channel` impls still compile. New error variant available.

- **Priority:** P1
- **Scope:** bob-core minimal extension
- **Status:** � DONE

- [x] **Step 1:** Add `NotSupported(String)` variant to `ChannelError` in `crates/bob-core/src/channel.rs`.
- [x] **Step 2:** Verify all existing `Channel` impls still compile.
- [x] **Step 3:** Add re-export in `bob-chat::lib.rs` for `bob_core::channel::*`.
- [x] **Verification:** `cargo check --all-targets --all-features` compiles cleanly.

---

## Phase 4: Polish, QA & Docs

### Task 4.1: Implement MockChatAdapter for Testing

> **Context:** A mock adapter is essential for testing the ChatBot orchestrator. It records all method calls and can be configured to return canned events. Should be behind `#[cfg(test)]` or a `test-utils` feature.
> **Verification:** Mock adapter can inject events and record post/edit/delete calls.

- **Priority:** P1
- **Scope:** Test utilities
- **Status:** � DONE

- [x] **Step 1:** Create a `MockChatAdapter` struct in test module of `bot.rs` (or a `test_utils` module gated by `#[cfg(test)]`).
- [x] **Step 2:** Implement `ChatAdapter` for `MockChatAdapter` with: configurable event queue (injected via `Arc<Mutex<VecDeque<ChatEvent>>>`), call recording (post_message calls, edit calls, delete calls, etc.).
- [x] **Step 3:** Add helper methods: `inject_event`, `posted_messages`, `edited_messages`, `deleted_messages`.
- [x] **Verification:** MockAdapter is usable in integration tests.

---

### Task 4.2: Write Unit Tests for All Modules

> **Context:** Each module needs colocated unit tests per AGENTS.md testing requirements. Cover edge cases and error paths.
> **Verification:** `cargo test -p bob-chat` passes all tests.

- **Priority:** P1
- **Scope:** Unit tests across all modules
- **Status:** � DONE

- [x] **Step 1:** Add tests to `error.rs`: Display output, From conversions.
- [x] **Step 2:** Add tests to `card.rs`: builder patterns, text fallback rendering, serde roundtrip.
- [x] **Step 3:** Add tests to `modal.rs`: builder patterns, serde roundtrip, empty modal.
- [x] **Step 4:** Add tests to `emoji.rs`: well-known resolution per platform, unknown emoji, Display.
- [x] **Step 5:** Add tests to `message.rs`: PostableMessage variants, IncomingMessage construction.
- [x] **Step 6:** Add tests to `event.rs`: ChatEvent variant construction and pattern matching.
- [x] **Step 7:** Add tests to `stream.rs`: fallback stream with mock adapter, timing assertions.
- [x] **Step 8:** Add tests to `adapter.rs`: default method behavior (NotSupported returns).
- [x] **Verification:** `cargo test -p bob-chat` — all pass with zero warnings.

---

### Task 4.3: Write Integration Tests for ChatBot Lifecycle

> **Context:** Full lifecycle tests: create ChatBot, register handlers, add MockAdapter, inject events, verify handlers called with correct data. Place in `crates/bob-chat/tests/` per AGENTS.md.
> **Verification:** Integration tests exercise the full event pipeline.

- **Priority:** P1
- **Scope:** Integration test suite
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-chat/tests/bot_lifecycle.rs`.
- [x] **Step 2:** Test: register `on_mention` handler → inject Mention event → verify handler called with correct Thread and Message data.
- [x] **Step 3:** Test: register `on_slash_command("/test")` → inject SlashCommand event → verify handler called, non-matching commands ignored.
- [x] **Step 4:** Test: register `on_action("approve")` → inject Action event → verify handler called, non-matching action_ids ignored.
- [x] **Step 5:** Test: register `on_reaction([ThumbsUp])` → inject ThumbsUp and ThumbsDown → verify only ThumbsUp triggers handler.
- [x] **Step 6:** Test: subscribe to thread → inject Message to subscribed thread → verify `on_subscribed_message` handler called.
- [ ] **Step 7:** Test: inject `PostableMessage::Stream` → verify fallback streaming (post + N edits). *(Skipped: streaming is tested via unit tests in stream.rs; integration test adapter doesn't support real streaming)*
- [x] **Step 8:** Test: `post_ephemeral` with DM fallback on unsupporting adapter → verify `open_dm` + `post_message` called.
- [x] **Verification:** `cargo test -p bob-chat --test bot_lifecycle` — 7 passed, 0 failed.

---

### Task 4.4: Documentation and README

> **Context:** Crate documentation including README, module-level docs, and public API documentation. `cargo doc` must build without warnings per VP-05.
> **Verification:** `cargo doc --no-deps -p bob-chat` builds without warnings.

- **Priority:** P2
- **Scope:** Documentation
- **Status:** � DONE

- [x] **Step 1:** Write `crates/bob-chat/README.md` with: overview, feature list (10 features), example usage snippet (ChatBot setup with mock adapter), architecture diagram (text-based).
- [x] **Step 2:** Add crate-level `//!` documentation in `lib.rs` covering architecture, usage, and module overview.
- [x] **Step 3:** Add module-level `//!` docs for each source file.
- [x] **Step 4:** Add doc comments on all public types, traits, and methods.
- [x] **Verification:** `cargo doc --no-deps -p bob-chat` — no warnings.

---

### Task 4.5: Final Lint, Format, and CI Verification

> **Context:** Per AGENTS.md development workflow, all features must pass format/lint/test before completion. Uses `just format`, `just lint`, `just test`.
> **Verification:** All three commands pass cleanly.

- **Priority:** P0
- **Scope:** CI readiness
- **Status:** � DONE

- [x] **Step 1:** Run `just format` — fix any formatting issues.
- [x] **Step 2:** Run `just lint` — fix any clippy warnings (pedantic + nursery per workspace config), typos, markdown lint.
- [x] **Step 3:** Run `just test` — verify all tests pass including the new `bob-chat` tests.
- [x] **Step 4:** Run `cargo check --all-targets --all-features` — verify full compilation.
- [x] **Verification:** All four commands succeed with zero errors/warnings.

---

## Summary & Timeline

| Phase | Tasks | Target Date |
| :--- | :---: | :--- |
| **1. Foundation** | 4 | 03-05 |
| **2. Core Logic** | 5 | 03-08 |
| **3. Integration** | 4 | 03-11 |
| **4. Polish** | 5 | 03-14 |
| **Total** | **18** | |

## Definition of Done

1. [x] **Linted:** `just lint` passes with no errors.
2. [x] **Tested:** Unit tests covering all modules, integration tests for ChatBot lifecycle.
3. [x] **Formatted:** `just format` applied, no diffs.
4. [x] **Verified:** `cargo check --all-targets --all-features` and `cargo test --all-features` pass.
5. [x] **Documented:** `cargo doc --no-deps -p bob-chat` builds without warnings.
