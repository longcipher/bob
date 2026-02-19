# Design Document: Implement v1 Agent Loop

| Metadata | Details |
| :--- | :--- |
| **Author** | pb-plan agent |
| **Status** | Draft |
| **Created** | 2026-02-19 |
| **Reviewers** | Project maintainers |
| **Related Issues** | N/A |

## 1. Executive Summary

**Problem:** The Bob agent framework has scaffolded crates (`bob-core`, `bob-runtime`, `bob-adapters`, `bin/cli-agent`) with domain types, port traits, and error types fully defined — but zero runtime behavior. `DefaultAgentRuntime::run()` returns a hardcoded "not yet implemented" string. No adapters exist. The CLI binary has an empty `main()`. The framework cannot execute a single agent turn.

**Solution:** Implement the complete v1 agent turn loop per `docs/design.md`: a 6-state scheduler FSM (Start → BuildPrompt → LlmInfer → ParseAction → CallTool → Done) with loop guards and cancellation, concrete adapters for `genai` (LLM), `rmcp` (MCP tools), in-memory session store, and `tracing` event sink, plus a CLI binary that wires everything together with TOML config and runs a REPL.

---

## 2. Requirements & Goals

### 2.1 Problem Statement

The codebase is a well-designed shell: hexagonal ports and domain types compile and pass unit tests, but the runtime does nothing. Every adapter module is empty. The CLI is a no-op. There is no way to send a user message through the framework and receive an LLM-driven response.

Key gaps:

1. **Scheduler:** `DefaultAgentRuntime::run()` is a stub — no turn loop, no state machine, no guard enforcement.
2. **LLM adapter:** No `GenAiLlmAdapter` implementing `LlmPort` via `genai`.
3. **Tool adapter:** No `McpToolAdapter` implementing `ToolPort` via `rmcp`.
4. **Store adapter:** No `InMemorySessionStore` implementing `SessionStore`.
5. **Event adapter:** No `TracingEventSink` implementing `EventSink`.
6. **CLI:** No config loading, no adapter wiring, no REPL loop.

### 2.2 Functional Goals

1. **Scheduler turn loop:** Implement the 6-state FSM with deterministic transitions, loop guards (`max_steps`, `max_tool_calls`, `max_consecutive_errors`, `turn_timeout_ms`), and cooperative cancellation via `CancellationToken`.
2. **Action parsing:** Implement `JsonActionMode` — parse `AgentAction` from LLM JSON output, with one re-prompt on malformed output.
3. **Prompt building:** Assemble system prompt + tool schemas + message history into `LlmRequest`.
4. **GenAi LLM adapter:** Map `LlmRequest` → `genai::Client::exec_chat` for non-streaming; map `LlmRequest` → `genai::Client::exec_chat_stream` for streaming. Handle provider errors, rate limits, and usage extraction.
5. **MCP tool adapter:** Connect to MCP servers via `rmcp` (stdio transport), discover tools (`tools/list`), execute tool calls (`tools/call`), and surface them through `ToolPort`.
6. **In-memory session store:** Thread-safe `HashMap<SessionId, SessionState>` behind `Arc<Mutex<>>` or `scc::HashMap`.
7. **Tracing event sink:** Emit `AgentEvent` variants as structured `tracing` spans/events.
8. **CLI agent:** Load `agent.toml` config, wire adapters into `DefaultAgentRuntime`, run a simple stdin/stdout REPL loop.

### 2.3 Non-Functional Goals

- **Performance:** The scheduler loop itself should add < 1ms overhead per turn step (LLM/tool latency dominates). Use `Arc<dyn Trait>` dispatch.
- **Reliability:** Loop guards guarantee termination under all conditions. Cancellation is cooperative and checked at every async boundary.
- **Testability:** Every scheduler transition is testable with mock ports. Adapter implementations have contract tests against real (or fake) backends.
- **Observability:** Every state transition emits a structured event via `EventSink`.

### 2.4 Out of Scope

- Streaming (`run_stream`) full implementation — scaffold the streaming path but the priority is the non-streaming `run()` loop.
- Skill loading via `agent-skills` — system prompt is static in v1; skill adapter is a fast-follow.
- Sub-agent composition — deferred to v1.1.
- Model catalog / model routing — v1 uses a simple `default_model` string config.
- Suspend/resume — deferred to v1.1.
- SQLite session store — in-memory and optional JSON file store only.
- NativeToolCallMode — v1 uses `JsonActionMode` exclusively.
- History condensation via LLM — v1 uses deterministic truncation only.

### 2.5 Assumptions

- `genai = "=0.6.0-beta.1"` provides `Client::exec_chat` and `Client::exec_chat_stream` with `ChatRequest`, `ChatResponse`, tool-related types.
- `rmcp = "0.16"` client API supports `ClientHandler`, `RunningService`, and `tools/list` + `tools/call` methods.
- The CLI binary is the sole composition root — no library-level composition API is needed yet.
- MCP stdio transport is the primary integration path for v1. HTTP/SSE transports are supported by `rmcp` feature flags but not explicitly tested.
- Users will provide a valid `agent.toml` config file at a known path (default `./agent.toml`).

---

## 3. Architecture Overview

### 3.1 System Context

```text
User ←→ CLI REPL ←→ DefaultAgentRuntime (scheduler FSM)
                          ├── LlmPort (GenAiLlmAdapter → genai → OpenAI/Anthropic/etc.)
                          ├── ToolPort (McpToolAdapter → rmcp → MCP servers)
                          ├── SessionStore (InMemorySessionStore)
                          └── EventSink (TracingEventSink → tracing)
```

The scheduler FSM is the core value. It:

1. Loads session state from `SessionStore`.
2. Builds a prompt with tool schemas from `ToolPort`.
3. Calls `LlmPort::complete` for inference.
4. Parses the `AgentAction` JSON response.
5. On `ToolCall`: executes via `ToolPort::call_tool`, appends result, loops.
6. On `Final`/`AskUser`: persists session, emits event, returns result.
7. On guard exhaustion: persists session, returns with `GuardExceeded` finish reason.

### 3.2 Key Design Principles

1. **Deterministic FSM:** Every state has exactly one entry and one or two exits. No implicit transitions.
2. **Guard-bounded execution:** The turn loop always terminates — `max_steps`, `max_tool_calls`, `turn_timeout_ms`, and cancellation all guarantee bounded execution.
3. **Port isolation:** The scheduler calls only trait methods — never concrete adapter types. All adapter selection happens in the composition root (`bin/cli-agent`).
4. **Event-driven observability:** Every state transition emits an `AgentEvent`. The scheduler never logs directly — it delegates to `EventSink`.
5. **Fail-fast on parse errors:** If the LLM produces invalid JSON, re-prompt once. If still invalid, return an error via `AgentError`.

### 3.3 Existing Components to Reuse

| Component | Location | How to Reuse |
| :--- | :--- | :--- |
| Domain types (all) | `crates/bob-core/src/types.rs` | Use directly — `AgentRequest`, `AgentResponse`, `AgentAction`, `ToolCall`, `ToolResult`, `TurnPolicy`, `GuardReason`, etc. |
| Port traits (4) | `crates/bob-core/src/ports.rs` | Implement in `bob-adapters`; consume in `bob-runtime` scheduler |
| Error types | `crates/bob-core/src/error.rs` | `AgentError`, `LlmError`, `ToolError`, `StoreError` — use as-is |
| `DefaultAgentRuntime` struct | `crates/bob-runtime/src/lib.rs` | Replace stub `run()` body with scheduler FSM |
| `AgentBootstrap` / `AgentRuntime` traits | `crates/bob-runtime/src/lib.rs` | Already defined — implement `AgentBootstrap` in CLI composition |
| Mock ports (tests) | `crates/bob-core/src/ports.rs`, `crates/bob-runtime/src/lib.rs` | Reuse mock/stub impls for scheduler unit tests |
| Workspace lint config | `Cargo.toml` | Keep as-is |
| Justfile tasks | `Justfile` | Use `just format`, `just lint`, `just test` workflow |

---

## 4. Detailed Design

### 4.1 Module Structure

**New files to create:**

```text
crates/bob-runtime/src/
  lib.rs                    # (existing) Add scheduler module
  scheduler.rs              # ← NEW: 6-state turn loop FSM
  prompt.rs                 # ← NEW: Prompt builder (system prompt + tools + history)
  action.rs                 # ← NEW: AgentAction JSON parser with re-prompt logic

crates/bob-adapters/src/
  lib.rs                    # (existing) Add module declarations + re-exports
  llm_genai.rs              # ← NEW: GenAiLlmAdapter (LlmPort via genai)
  mcp_rmcp.rs               # ← NEW: McpToolAdapter (ToolPort via rmcp)
  store_memory.rs           # ← NEW: InMemorySessionStore (SessionStore)
  observe.rs                # ← NEW: TracingEventSink (EventSink)

bin/cli-agent/src/
  main.rs                   # (existing) Replace stub with config + wiring + REPL
```

**Files to modify:**

```text
crates/bob-runtime/src/lib.rs    # Import scheduler, wire into DefaultAgentRuntime::run()
crates/bob-adapters/src/lib.rs   # Declare adapter modules with feature gates
crates/bob-adapters/Cargo.toml   # Add serde_json, scc dependencies
bin/cli-agent/Cargo.toml         # Add serde, toml dependencies
```

### 4.2 Data Structures & Types

All domain types already exist in `bob-core/src/types.rs`. New internal types needed:

**Scheduler internal state (in `bob-runtime/src/scheduler.rs`):**

```rust
/// Internal loop guard that tracks execution bounds.
struct LoopGuard {
    policy: TurnPolicy,
    steps: u32,
    tool_calls: u32,
    consecutive_errors: u32,
    start: tokio::time::Instant,
}

impl LoopGuard {
    fn new(policy: TurnPolicy) -> Self;
    fn can_continue(&self) -> bool;
    fn record_step(&mut self);
    fn record_tool_call(&mut self);
    fn record_error(&mut self);
    fn reset_errors(&mut self);
    fn reason(&self) -> GuardReason;
    fn timed_out(&self) -> bool;
}
```

**Prompt builder output (in `bob-runtime/src/prompt.rs`):**

```rust
/// System prompt template with tool schema injection.
fn build_llm_request(
    model: &str,
    session: &SessionState,
    tools: &[ToolDescriptor],
    system_instructions: &str,
) -> LlmRequest;

/// Build JSON action schema instructions for the LLM.
fn action_schema_prompt() -> String;

/// Build tool schema block for prompt injection.
fn tool_schema_block(tools: &[ToolDescriptor]) -> String;
```

**Action parser (in `bob-runtime/src/action.rs`):**

```rust
/// Parse AgentAction from LLM response content.
fn parse_action(content: &str) -> Result<AgentAction, ActionParseError>;

/// Error when LLM output is not valid AgentAction JSON.
#[derive(Debug, thiserror::Error)]
enum ActionParseError {
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("unknown action type: {0}")]
    UnknownType(String),
}
```

**CLI config (in `bin/cli-agent/src/main.rs` or a `config.rs` module):**

```rust
#[derive(serde::Deserialize)]
struct AgentConfig {
    runtime: RuntimeConfig,
    llm: Option<LlmConfig>,
    policy: Option<PolicyConfig>,
    mcp: Option<McpConfig>,
}

#[derive(serde::Deserialize)]
struct RuntimeConfig {
    default_model: String,
    max_steps: Option<u32>,
    turn_timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
struct McpConfig {
    servers: Vec<McpServerEntry>,
}

#[derive(serde::Deserialize)]
struct McpServerEntry {
    id: String,
    transport: String,
    command: String,
    args: Vec<String>,
    env: Option<HashMap<String, String>>,
    tool_timeout_ms: Option<u64>,
}
```

### 4.3 Interface Design

The public interfaces are already defined:

- `AgentRuntime::run(req) -> Result<AgentRunResult, AgentError>` — the main entry point.
- `AgentRuntime::run_stream(req) -> Result<AgentEventStream, AgentError>` — streaming variant (scaffolded, not full impl).
- `AgentRuntime::health() -> RuntimeHealth` — health check.
- `AgentBootstrap::register_mcp_server(cfg) -> Result<(), AgentError>` — register MCP server.
- `AgentBootstrap::build() -> Result<Arc<dyn AgentRuntime>, AgentError>` — freeze into runtime.

**Internal interfaces (new):**

- `scheduler::run_turn(llm, tools, store, events, req, policy) -> Result<AgentRunResult, AgentError>` — the core turn loop function.
- `prompt::build_llm_request(model, session, tools, system) -> LlmRequest` — prompt assembly.
- `action::parse_action(content) -> Result<AgentAction, ActionParseError>` — JSON action parsing.

### 4.4 Logic Flow

**Scheduler Turn Loop (6 states):**

```text
1. Start
   - Load session from SessionStore (or create default)
   - List tools from ToolPort
   - Initialize LoopGuard from TurnPolicy
   - Emit TurnStarted event
   - Append user message to session history

2. BuildPrompt
   - Assemble system prompt with:
     - Core instructions
     - JSON action schema contract
     - Tool schema block (from ToolDescriptor list)
   - Truncate history if > max_history_messages (default 50, drop oldest non-system)
   - Build LlmRequest with model, messages, tools

3. LlmInfer
   - Check cancellation token (tokio::select!)
   - Check guard timeout
   - Call LlmPort::complete(llm_request)
   - Record step in guard
   - Emit LlmCallStarted / LlmCallCompleted events
   - Append assistant message to session history

4. ParseAction
   - Parse AgentAction from LLM response content
   - If parse fails: build re-prompt message, go to BuildPrompt (once)
   - If parse fails twice: return AgentError

5. CallTool (if AgentAction::ToolCall)
   - Emit ToolCallStarted event
   - Call ToolPort::call_tool with timeout (tool_timeout_ms)
   - Record tool call in guard
   - Emit ToolCallCompleted event
   - Append tool result to session history
   - If guard exhausted: go to Done
   - Else: go to BuildPrompt

6. Done (if AgentAction::Final or AskUser or guard exhausted)
   - Save session to SessionStore
   - Emit TurnCompleted event
   - Return AgentRunResult::Finished(AgentResponse)
```

### 4.5 Configuration

**`agent.toml` (v1 minimal):**

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000

[llm]
retry_max = 2
stream_default = false

[policy]
deny_tools = []

[[mcp.servers]]
id = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
tool_timeout_ms = 15000
```

**Environment variable support:**

- `OPENAI_API_KEY` (or other provider keys) are read by `genai` directly from the environment.
- MCP server `env` map values support `${VAR_NAME}` interpolation from process env.

### 4.6 Error Handling

Error flow through the scheduler:

1. **LLM errors** (`LlmError`): Bubble up as `AgentError::Llm`. Transport/429 errors get bounded retry (max 2). Logic errors are not retried.
2. **Tool errors** (`ToolError`): Converted to `ToolResult { is_error: true }` and appended to history so the LLM can self-correct. Increment `consecutive_errors` in guard.
3. **Parse errors** (`ActionParseError`): First failure triggers a re-prompt. Second failure returns `AgentError::Internal`.
4. **Store errors** (`StoreError`): Bubble up as `AgentError::Store`. Session save failures are non-fatal for the response but logged.
5. **Guard exhaustion**: Returns `AgentRunResult::Finished` with `FinishReason::GuardExceeded` and a descriptive content message.
6. **Cancellation**: Returns `AgentRunResult::Finished` with `FinishReason::Cancelled`.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Testing

- **Action parser:** Test valid `AgentAction` variants, malformed JSON, missing fields, unknown types.
- **Prompt builder:** Test system prompt assembly, tool schema injection, history truncation at boundary.
- **Loop guard:** Test `can_continue()` transitions for each limit, timeout detection, reason reporting.
- **Scheduler FSM:** Test full turn with mock ports for: `Final` response, `ToolCall` → `Final`, guard exhaustion, cancellation, parse error + re-prompt.

### 5.2 Integration Testing

- **GenAi adapter contract test:** Verify `LlmRequest → genai ChatRequest` mapping, response normalization, error mapping. (Requires `OPENAI_API_KEY` env var — gated behind `#[ignore]` by default.)
- **MCP adapter contract test:** Spawn a real MCP server (e.g., `npx @modelcontextprotocol/server-filesystem`), list tools, call a tool, verify results. (Gated behind `#[ignore]`.)
- **In-memory store:** Roundtrip save/load, concurrent access.
- **Full turn e2e:** With mock LLM that returns canned `AgentAction` JSON + real in-memory store + mock tool port → verify complete turn execution.

### 5.3 Critical Path Verification (The "Harness")

| Verification Step | Command | Success Criteria |
| :--- | :--- | :--- |
| **VP-01** | `cargo check --workspace --all-features` | Compiles with no errors |
| **VP-02** | `cargo test --workspace --all-features` | All tests pass |
| **VP-03** | `just lint` | No clippy warnings, no format issues |
| **VP-04** | `cargo run --bin cli-agent -- --help` | Shows CLI help text |
| **VP-05** | `cargo run --bin cli-agent -- --config agent.toml` (with valid config + API key) | Starts REPL, accepts input, returns LLM response |

### 5.4 Validation Rules

| Test Case ID | Action | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | Send "Hello" with no tools configured | LLM returns a `Final` action, agent returns content | Unit test with mock LlmPort |
| **TC-02** | Send request triggering a tool call | LLM returns `ToolCall`, tool executes, LLM returns `Final` | Unit test with mock LlmPort + mock ToolPort |
| **TC-03** | LLM returns invalid JSON | Parser re-prompts once, then succeeds | Unit test with mock LlmPort returning bad then good JSON |
| **TC-04** | LLM returns invalid JSON twice | `AgentError` returned | Unit test |
| **TC-05** | Turn exceeds `max_steps` | Returns with `FinishReason::GuardExceeded` | Unit test with mock LlmPort always returning ToolCall |
| **TC-06** | Cancellation token triggered mid-turn | Returns with `FinishReason::Cancelled` | Unit test with immediate cancellation |
| **TC-07** | Tool call times out | `ToolResult { is_error: true }` appended, loop continues | Unit test with slow mock ToolPort |
| **TC-08** | Session persistence roundtrip | Load empty, run turn, save, load again → history present | Integration test with InMemorySessionStore |
| **TC-09** | MCP tool discovery | McpToolAdapter lists tools from a real MCP server | Integration test (gated `#[ignore]`) |
| **TC-10** | CLI REPL runs | `cli-agent --config agent.toml` starts, accepts input | Manual verification |

---

## 6. Implementation Plan

- [ ] **Phase 1: Scheduler Foundation** — Loop guard, action parser, prompt builder, core turn loop with mock ports.
- [ ] **Phase 2: Adapters** — GenAi LLM adapter, MCP tool adapter, in-memory session store, tracing event sink.
- [ ] **Phase 3: CLI Integration** — Config loading, adapter wiring, REPL loop, end-to-end demo.
- [ ] **Phase 4: Polish & Hardening** — Edge case handling, comprehensive tests, lint/format pass, documentation.

---

## 7. Cross-Functional Concerns

- **Security:** Tool execution honors deny-list from config. MCP server processes are managed with lifecycle cleanup. No secrets are logged — `TracingEventSink` redacts sensitive arguments.
- **Backward compatibility:** Not applicable — this is the initial implementation. The `AgentRuntime` and `AgentBootstrap` trait signatures are already defined and will not change.
- **Migration:** No data migration needed — in-memory store starts fresh each process.
- **Monitoring:** All scheduler events flow through `EventSink`. With `TracingEventSink`, operators can configure any `tracing` subscriber (stdout, OTLP, etc.) in the CLI binary.
