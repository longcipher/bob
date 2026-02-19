# Implement v1 Agent Loop — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-02-19-02-implement-v1-agent-loop/design.md |
| **Owner** | Project maintainers |
| **Start Date** | 2026-02-19 |
| **Target Date** | 2026-03-12 |
| **Status** | Planning |

## Summary & Phasing

Implement the Bob agent framework's v1 turn loop, adapters, and CLI in 4 phases. The scaffolded crates (`bob-core`, `bob-runtime`, `bob-adapters`, `bin/cli-agent`) already compile with domain types, port traits, and error types. This plan fills in the runtime behavior.

- **Phase 1: Scheduler Foundation** — Build the turn loop internals: loop guard, action parser, prompt builder, and the 6-state scheduler FSM with mock port tests.
- **Phase 2: Adapters** — Implement concrete adapters: GenAi LLM, rmcp MCP tools, in-memory session store, tracing event sink.
- **Phase 3: CLI Integration** — Wire adapters into the CLI binary with TOML config loading and a stdin/stdout REPL loop.
- **Phase 4: Polish & Hardening** — Edge cases, comprehensive tests, documentation, lint/format pass.

---

## Phase 1: Scheduler Foundation

### Task 1.1: Implement Loop Guard

> **Context:** The `LoopGuard` is the safety net that guarantees turn termination. It tracks `steps`, `tool_calls`, `consecutive_errors`, and elapsed time against `TurnPolicy` limits. Already defined types: `TurnPolicy`, `GuardReason` in `crates/bob-core/src/types.rs`.
> **Verification:** Unit tests cover every guard limit (max_steps, max_tool_calls, max_consecutive_errors, timeout) and the `can_continue()` / `reason()` API.

- **Priority:** P0
- **Scope:** Scheduler internals — `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-runtime/src/scheduler.rs` with `LoopGuard` struct containing `policy: TurnPolicy`, counters, and `start: tokio::time::Instant`.
- [x] **Step 2:** Implement `LoopGuard::new(policy)`, `can_continue() -> bool`, `record_step()`, `record_tool_call()`, `record_error()`, `reset_errors()`, `reason() -> GuardReason`, `timed_out() -> bool`.
- [x] **Step 3:** Add `mod scheduler;` to `crates/bob-runtime/src/lib.rs`.
- [x] **Step 4:** Write unit tests: guard trips on `max_steps`, trips on `max_tool_calls`, trips on `max_consecutive_errors`, timeout detection, `reset_errors` clears counter.
- [x] **Verification:** `cargo test -p bob-runtime -- scheduler` — all guard tests pass.

---

### Task 1.2: Implement Action Parser

> **Context:** The `JsonActionMode` parser extracts `AgentAction` from raw LLM text output. The LLM is instructed to output exactly one JSON object matching the `AgentAction` schema. Parser must handle: valid JSON of each variant, malformed JSON, missing required fields, unknown `type` values. Existing type: `AgentAction` in `crates/bob-core/src/types.rs` with serde `tag = "type"`.
> **Verification:** Unit tests for every `AgentAction` variant, every error case, and edge cases (extra whitespace, markdown code fences around JSON).

- **Priority:** P0
- **Scope:** Scheduler internals — `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-runtime/src/action.rs` with `parse_action(content: &str) -> Result<AgentAction, ActionParseError>`.
- [x] **Step 2:** Define `ActionParseError` enum with `InvalidJson`, `MissingField`, `UnknownType` variants (use `thiserror`). Add `thiserror` to `bob-runtime` dependencies.
- [x] **Step 3:** Implement parsing logic: strip optional markdown code fences (```` ```json ... ``` ````), parse as `serde_json::Value`, validate `type` field, deserialize to `AgentAction`.
- [x] **Step 4:** Add `mod action;` to `crates/bob-runtime/src/lib.rs`.
- [x] **Step 5:** Write unit tests: parse `Final`, `ToolCall`, `AskUser` variants; reject missing `type`; reject unknown `type`; handle wrapped-in-code-fences JSON; handle extra whitespace; reject non-JSON text.
- [x] **Verification:** `cargo test -p bob-runtime -- action` — all parser tests pass.

---

### Task 1.3: Implement Prompt Builder

> **Context:** The prompt builder assembles a complete `LlmRequest` from session state, tool descriptors, and system instructions. It must inject the `JsonActionMode` schema contract and render tool schemas so the LLM knows what tools are available. Existing types: `LlmRequest`, `Message`, `Role`, `ToolDescriptor`, `SessionState` in `bob-core`.
> **Verification:** Unit tests confirm correct message ordering (system → history → user), tool schema injection, and history truncation at the 50-message boundary.

- **Priority:** P0
- **Scope:** Scheduler internals — `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-runtime/src/prompt.rs`.
- [x] **Step 2:** Implement `action_schema_prompt() -> String` — returns the JSON action schema contract text per design doc §8.3.1.
- [x] **Step 3:** Implement `tool_schema_block(tools: &[ToolDescriptor]) -> String` — renders tool names, descriptions, and input schemas as a text block.
- [x] **Step 4:** Implement `build_llm_request(model: &str, session: &SessionState, tools: &[ToolDescriptor], system_instructions: &str) -> LlmRequest` — assembles messages: system (core instructions + action schema + tool schemas), then session history (truncated to most recent 50 non-system messages), returns `LlmRequest`.
- [x] **Step 5:** Implement `truncate_history(messages: &[Message], max: usize) -> Vec<Message>` — drops oldest non-system messages when history exceeds limit.
- [x] **Step 6:** Add `mod prompt;` to `crates/bob-runtime/src/lib.rs`. Add `serde_json` as a dependency of `bob-runtime`.
- [x] **Step 7:** Write unit tests: empty session produces system + no history; tool schema block renders correctly; history truncation keeps most recent messages; system messages are never truncated.
- [x] **Verification:** `cargo test -p bob-runtime -- prompt` — all tests pass.

---

### Task 1.4: Implement Scheduler Turn Loop

> **Context:** This is the core of the framework — the 6-state FSM that orchestrates a single agent turn. It wires together `LoopGuard`, `prompt::build_llm_request`, `action::parse_action`, and the 4 port traits. The existing `DefaultAgentRuntime::run()` stub must be replaced with this implementation. Reuse existing mock ports from `bob-runtime/src/lib.rs` tests and `bob-core/src/ports.rs` tests.
> **Verification:** Unit tests with mock ports cover: simple `Final` response, `ToolCall` → `Final` chain, guard exhaustion, cancellation, parse error re-prompt, double parse failure, tool error handling.

- **Priority:** P0
- **Scope:** Core scheduler — `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** In `crates/bob-runtime/src/scheduler.rs`, implement `pub(crate) async fn run_turn(llm, tools, store, events, req, policy) -> Result<AgentRunResult, AgentError>` following the 6-state FSM from design §4.4.
- [x] **Step 2:** Implement Start state: load session from `SessionStore`, list tools from `ToolPort`, init `LoopGuard`, emit `TurnStarted`, append user message to session.
- [x] **Step 3:** Implement the main loop: check cancellation via `tokio::select!`, check guard, build prompt, call `LlmPort::complete`, record step, emit LLM events, append assistant message.
- [x] **Step 4:** Implement ParseAction: call `action::parse_action`. On failure, append re-prompt message asking for valid JSON and loop (once). On second failure, return `AgentError::Internal`.
- [x] **Step 5:** Implement CallTool branch: emit `ToolCallStarted`, call `ToolPort::call_tool` with `tokio::time::timeout`, emit `ToolCallCompleted`, append tool result to session, record in guard. On tool error, create `ToolResult { is_error: true }` and increment `consecutive_errors`.
- [x] **Step 6:** Implement Done state: save session, emit `TurnCompleted`, build `AgentResponse` from accumulated data.
- [x] **Step 7:** Replace the stub in `DefaultAgentRuntime::run()` with a call to `scheduler::run_turn(...)`, passing the 4 ports and request.
- [x] **Step 8:** Write comprehensive tests using mock ports:
  - TC-01: Simple `Final` response.
  - TC-02: `ToolCall` → `Final` chain.
  - TC-03: Parse error → re-prompt → success.
  - TC-04: Double parse error → `AgentError`.
  - TC-05: `max_steps` exhaustion → `GuardExceeded`.
  - TC-06: Cancellation mid-turn → `Cancelled`.
  - TC-07: Tool error → `is_error` result → LLM sees error → `Final`.
- [x] **Verification:** `cargo test -p bob-runtime` — all scheduler tests pass. `just lint` clean.

---

## Phase 2: Adapters

### Task 2.1: Implement GenAi LLM Adapter

> **Context:** The `GenAiLlmAdapter` implements `LlmPort` using the `genai` crate (`=0.6.0-beta.1`). It maps `LlmRequest` to `genai::ChatRequest`, calls `Client::exec_chat`, and normalizes the response to `LlmResponse`. Non-streaming first; streaming can be scaffolded. Feature-gated behind `llm-genai`.
> **Verification:** Contract test with mock expectations for request mapping. Integration test with real API call (gated `#[ignore]`).

- **Priority:** P0
- **Scope:** LLM adapter — `bob-adapters`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-adapters/src/llm_genai.rs`. Define `GenAiLlmAdapter` struct with `genai::Client` field.
- [x] **Step 2:** Implement `GenAiLlmAdapter::new(client: genai::Client) -> Self`.
- [x] **Step 3:** Implement `LlmPort::complete`: map `LlmRequest.messages` to `genai` `ChatMessage` types (system, user, assistant, tool roles), build `ChatRequest` with model, call `client.exec_chat(model, request)`, extract content and usage from `ChatResponse`, build `LlmResponse`.
- [x] **Step 4:** Implement `LlmPort::complete_stream`: scaffold with `client.exec_chat_stream`, forward chunks as `LlmStreamChunk::TextDelta`, emit `Done` on stream end. (Best-effort — non-streaming path is priority.)
- [x] **Step 5:** Handle error mapping: `genai` errors → `LlmError::Provider`, rate limit → `LlmError::RateLimited`, context length → `LlmError::ContextLengthExceeded`.
- [x] **Step 6:** Add `pub mod llm_genai;` to `crates/bob-adapters/src/lib.rs` gated on `#[cfg(feature = "llm-genai")]`.
- [x] **Step 7:** Write unit test verifying the adapter constructs correctly and implements `LlmPort` as `Arc<dyn LlmPort>`.
- [x] **Step 8:** Write integration test (`#[ignore]`) calling a real LLM with a simple prompt and asserting a non-empty response.
- [x] **Verification:** `cargo test -p bob-adapters --features llm-genai` — unit tests pass. `cargo test -p bob-adapters --features llm-genai -- --ignored` with `OPENAI_API_KEY` set — integration test passes.

---

### Task 2.2: Implement MCP Tool Adapter

> **Context:** The `McpToolAdapter` implements `ToolPort` via `rmcp` (v0.16). It manages connections to MCP servers (stdio transport), discovers tools via `tools/list`, and executes tool calls via `tools/call`. The `rmcp` `ClientHandler`/`ServerHandler` traits are not dyn-compatible (RPITIT) — the adapter uses concrete `RunningService` types internally and exposes `Arc<dyn ToolPort>`. Feature-gated behind `mcp-rmcp`.
> **Verification:** Contract test against a local MCP server (e.g., `@modelcontextprotocol/server-filesystem`). Gated behind `#[ignore]`.

- **Priority:** P0
- **Scope:** Tool adapter — `bob-adapters`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-adapters/src/mcp_rmcp.rs`. Define `McpToolAdapter` struct.
- [x] **Step 2:** Implement `McpToolAdapter::connect_stdio(command, args, env) -> Result<Self, ToolError>` — spawn the MCP server child process via `rmcp` client transport, establish connection.
- [x] **Step 3:** Implement `ToolPort::list_tools`: call `tools/list` on the MCP connection, map `rmcp::model::Tool` to `ToolDescriptor` with `ToolSource::Mcp { server }` and namespace prefix `mcp/<server_id>/`.
- [x] **Step 4:** Implement `ToolPort::call_tool`: resolve tool name, call `tools/call` with arguments, map response to `ToolResult`. Handle errors as `ToolResult { is_error: true }`.
- [x] **Step 5:** Implement `Drop` or explicit `shutdown()` for process cleanup — kill child process on drop.
- [x] **Step 6:** Add `pub mod mcp_rmcp;` to `crates/bob-adapters/src/lib.rs` gated on `#[cfg(feature = "mcp-rmcp")]`.
- [x] **Step 7:** Add `serde_json` to `bob-adapters` dependencies (needed for `ToolCall.arguments`).
- [x] **Step 8:** Write integration test (`#[ignore]`): spawn `npx -y @modelcontextprotocol/server-filesystem .`, list tools, call `read_file` tool on a known file, verify output.
- [x] **Verification:** `cargo test -p bob-adapters --features mcp-rmcp -- --ignored` with Node.js and npx available — integration test passes.

---

### Task 2.3: Implement In-Memory Session Store

> **Context:** `InMemorySessionStore` implements `SessionStore` for v1. Thread-safe storage of `SessionState` keyed by `SessionId`. Use `scc::HashMap` per project guidelines (preferred over `Arc<Mutex<HashMap>>`). Feature-gated behind `store-memory`.
> **Verification:** Unit tests for save/load roundtrip, missing session returns `None`, concurrent access safety.

- **Priority:** P0
- **Scope:** Store adapter — `bob-adapters`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-adapters/src/store_memory.rs`. Define `InMemorySessionStore` with `scc::HashMap<SessionId, SessionState>`.
- [x] **Step 2:** Add `scc` to workspace `Cargo.toml` and `bob-adapters/Cargo.toml` (not feature-gated — small dep, always useful).
- [x] **Step 3:** Implement `SessionStore::load` — lookup by key, clone and return.
- [x] **Step 4:** Implement `SessionStore::save` — insert or update by key.
- [x] **Step 5:** Add `pub mod store_memory;` to `crates/bob-adapters/src/lib.rs` (always compiled — `store-memory` feature is default-on).
- [x] **Step 6:** Write unit tests: roundtrip save/load, load missing returns `None`, overwrite existing session, verify `Arc<dyn SessionStore>` works.
- [x] **Verification:** `cargo test -p bob-adapters -- store_memory` — all tests pass.

---

### Task 2.4: Implement Tracing Event Sink

> **Context:** `TracingEventSink` implements `EventSink` by emitting `AgentEvent` variants as structured `tracing` events with appropriate levels and fields. Feature-gated behind `observe-tracing`.
> **Verification:** Unit test verifying all event variants are handled without panic. Optionally verify tracing output with `tracing-test` subscriber.

- **Priority:** P1
- **Scope:** Observability adapter — `bob-adapters`
- **Status:** � DONE

- [x] **Step 1:** Create `crates/bob-adapters/src/observe.rs`. Define `TracingEventSink` (unit struct).
- [x] **Step 2:** Implement `EventSink::emit` — match on each `AgentEvent` variant and emit a `tracing::info!` / `tracing::error!` with structured fields (session_id, model, tool name, etc.).
- [x] **Step 3:** Add `pub mod observe;` to `crates/bob-adapters/src/lib.rs` gated on `#[cfg(feature = "observe-tracing")]`.
- [x] **Step 4:** Write unit test: create `TracingEventSink`, emit every `AgentEvent` variant, assert no panic.
- [x] **Verification:** `cargo test -p bob-adapters --features observe-tracing -- observe` — test passes.

---

## Phase 3: CLI Integration

### Task 3.1: Implement Config Loading

> **Context:** The CLI binary loads `agent.toml` to configure the runtime. Config struct maps to the TOML schema defined in design §4.5. Use the `config` crate (already in workspace) for file loading with environment variable override support. Existing dependency: `config`, `serde`, `clap` in `bin/cli-agent/Cargo.toml`.
> **Verification:** Unit test parsing a sample TOML config into the struct. CLI `--help` shows config path option.

- **Priority:** P0
- **Scope:** CLI binary — `bin/cli-agent`
- **Status:** � DONE

- [x] **Step 1:** Add `serde = { workspace = true, features = ["derive"] }` and `serde_json = { workspace = true }` to `bin/cli-agent/Cargo.toml`.
- [x] **Step 2:** Define config structs in `bin/cli-agent/src/main.rs` (or a new `config.rs` module): `AgentConfig`, `RuntimeConfig`, `McpConfig`, `McpServerEntry`, etc., all deriving `serde::Deserialize`.
- [x] **Step 3:** Define CLI args with `clap::Parser`: `--config <path>` (default `./agent.toml`).
- [x] **Step 4:** Implement `load_config(path) -> Result<AgentConfig, eyre::Report>` using the `config` crate to read TOML file.
- [x] **Step 5:** Create a sample `agent.toml` in the project root with documented fields.
- [x] **Step 6:** Write unit test parsing a hardcoded TOML string into `AgentConfig`.
- [x] **Verification:** `cargo run --bin cli-agent -- --help` shows `--config` option. Config parsing test passes.

---

### Task 3.2: Wire Adapters into Runtime

> **Context:** The CLI binary is the composition root that instantiates adapters and wires them into `DefaultAgentRuntime`. It creates `GenAiLlmAdapter`, optionally `McpToolAdapter` for each configured MCP server, `InMemorySessionStore`, and `TracingEventSink`, then constructs `DefaultAgentRuntime`. Reuse `McpServerConfig` from `bob-runtime`.
> **Verification:** Binary starts without error when given a valid config with at least `default_model` set.

- **Priority:** P0
- **Scope:** CLI composition root — `bin/cli-agent`
- **Status:** � DONE

- [x] **Step 1:** Implement `async fn build_runtime(config: &AgentConfig) -> Result<Arc<dyn AgentRuntime>, eyre::Report>`:
  - Create `genai::Client` (default config, reads API keys from env).
  - Create `GenAiLlmAdapter::new(client)` → `Arc<dyn LlmPort>`.
  - For each MCP server in config: connect `McpToolAdapter::connect_stdio(...)` → collect into a composite `ToolPort` (or a `Vec`-based aggregator that delegates to multiple adapters).
  - Create `InMemorySessionStore::new()` → `Arc<dyn SessionStore>`.
  - Create `TracingEventSink` → `Arc<dyn EventSink>`.
  - Construct `DefaultAgentRuntime { llm, tools, store, events }`.
- [x] **Step 2:** If no MCP servers configured, use a `NoOpToolPort` that returns an empty tool list and errors on `call_tool`.
- [x] **Step 3:** Initialize `tracing_subscriber` for structured console logging.
- [x] **Step 4:** Wire it all in `#[tokio::main] async fn main()`.
- [x] **Verification:** `cargo run --bin cli-agent -- --config agent.toml` starts without panic (requires valid API key env var).

---

### Task 3.3: Implement REPL Loop

> **Context:** The CLI presents a simple stdin/stdout REPL: read user input, send as `AgentRequest` to runtime, print response. Session ID is fixed per process (single-session mode). Uses `tokio::io` for async stdin reading.
> **Verification:** Manual test: start CLI, type a message, receive LLM-generated response on stdout.

- **Priority:** P0
- **Scope:** CLI UX — `bin/cli-agent`
- **Status:** � DONE

- [x] **Step 1:** Implement REPL loop in `main()` after runtime construction:
  - Generate a session ID (UUID or fixed string for v1).
  - Print welcome banner with model name.
  - Loop: print prompt `>`, read line from stdin, skip empty lines, handle `/quit` command.
  - Construct `AgentRequest { input, session_id, model: None, metadata: {}, cancel_token: None }`.
  - Call `runtime.run(req).await`.
  - Print response content to stdout.
  - On error, print error message and continue loop.
- [x] **Step 2:** Handle Ctrl+C gracefully (break loop, print goodbye).
- [x] **Step 3:** Add `uuid` or use a simple counter for session IDs. (Or just use a fixed `"cli-session"` string for v1.)
- [x] **Verification:** Manual end-to-end test: `cargo run --bin cli-agent -- --config agent.toml`, type "Hello", receive response, type `/quit`, process exits.

---

## Phase 4: Polish & Hardening

### Task 4.1: Composite Tool Port for Multi-Server

> **Context:** When multiple MCP servers are configured, the CLI needs a composite `ToolPort` that aggregates tools from all servers and routes `call_tool` to the correct adapter. This is needed for the composition root.
> **Verification:** Unit test with two mock tool ports, composite lists tools from both, routes calls correctly.

- **Priority:** P1
- **Scope:** Adapter utility — `bob-adapters` or `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** Create `CompositeToolPort` struct holding `Vec<(String, Arc<dyn ToolPort>)>` (server_id, port pairs).
- [x] **Step 2:** Implement `ToolPort::list_tools` — aggregate tools from all inner ports, prefix tool IDs with server namespace.
- [x] **Step 3:** Implement `ToolPort::call_tool` — route by tool name prefix to the correct inner port.
- [x] **Step 4:** Write unit tests with mock tool ports.
- [x] **Verification:** `cargo test` — composite tool port tests pass.

---

### Task 4.2: History Truncation Edge Cases

> **Context:** The prompt builder's `truncate_history` must handle edge cases: empty history, history exactly at limit, all-system-message history, interleaved system messages.
> **Verification:** Unit tests for every edge case.

- **Priority:** P1
- **Scope:** Scheduler internals — `bob-runtime`
- **Status:** � DONE

- [x] **Step 1:** Review `truncate_history` implementation from Task 1.3.
- [x] **Step 2:** Add tests: empty history, history at exactly `max` messages, history with interleaved system messages (system messages must not be dropped), single-message history.
- [x] **Step 3:** Fix any issues discovered.
- [x] **Verification:** `cargo test -p bob-runtime -- prompt::truncate` — all edge case tests pass.

---

### Task 4.3: Comprehensive Lint & Format Pass

> **Context:** Per project quality checklist: `just format`, `just lint`, `just test` must all pass. English-only comments. No forbidden crates. Clippy pedantic clean.
> **Verification:** `just ci` passes (lint + test + build).

- **Priority:** P1
- **Scope:** Workspace-wide
- **Status:** � DONE

- [x] **Step 1:** Run `just format` and fix any formatting issues.
- [x] **Step 2:** Run `just lint` and fix all clippy warnings.
- [x] **Step 3:** Run `just test` and fix any test failures.
- [x] **Step 4:** Verify no Chinese characters: `just check-cn`.
- [x] **Step 5:** Verify no forbidden crates (`anyhow`, `log`, `reqwest`, `dashmap`): `cargo machete` + manual review.
- [x] **Verification:** `just ci` passes with zero warnings and zero errors.

---

### Task 4.4: Update Documentation

> **Context:** Update `README.md` with quickstart instructions, update `docs/design.md` Section 21 (Immediate Next Steps) to reflect completed work, and ensure inline doc comments in new modules meet Rust doc standards.
> **Verification:** `cargo doc --workspace --no-deps` builds without warnings. README has accurate quickstart.

- **Priority:** P2
- **Scope:** Documentation
- **Status:** � DONE

- [x] **Step 1:** Add module-level doc comments to all new files (`scheduler.rs`, `action.rs`, `prompt.rs`, `llm_genai.rs`, `mcp_rmcp.rs`, `store_memory.rs`, `observe.rs`).
- [x] **Step 2:** Update `README.md` with:
  - Quick start: install, configure `agent.toml`, run `cargo run --bin cli-agent`.
  - Architecture overview (one paragraph + link to `docs/design.md`).
  - Configuration reference (or link to example `agent.toml`).
- [x] **Step 3:** Update `docs/design.md` Section 21 to check off completed steps.
- [x] **Verification:** `cargo doc --workspace --no-deps` — no warnings. README matches reality.

---

## Summary & Timeline

| Phase | Tasks | Target Date |
| :--- | :---: | :--- |
| **1. Scheduler Foundation** | 4 | 02-25 |
| **2. Adapters** | 4 | 03-04 |
| **3. CLI Integration** | 3 | 03-08 |
| **4. Polish & Hardening** | 4 | 03-12 |
| **Total** | **15** | |

## Definition of Done

1. [x] **Linted:** `just lint` passes — no clippy warnings, no format issues.
2. [x] **Tested:** `just test` passes — all unit and integration tests green.
3. [x] **Formatted:** `just format` applied.
4. [x] **Verified:** Each task's specific Verification criterion met.
5. [x] **E2E demo:** `cargo run --bin cli-agent -- --config agent.toml` starts a REPL that can send a user message and receive an LLM response.
6. [x] **English only:** No Chinese in comments or docs.
7. [x] **No forbidden crates:** No `anyhow`, `log`, `reqwest`, `dashmap`, `trunk`.
