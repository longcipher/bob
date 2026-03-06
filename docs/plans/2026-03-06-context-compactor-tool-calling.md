# Context Compactor And Structured Tool Calling Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace Bob's hard-coded prompt history truncation with a pluggable compactor port, then enable correct native tool-calling by upgrading the persisted transcript model.

**Architecture:** Keep `bob-core` provider-neutral. Add a `ContextCompactorPort` with a deterministic default implementation, then refactor `SessionState` to hold structured transcript entries that can render into prompt messages. Only after transcript support is in place should the `genai` adapter advertise native tool-calling.

**Tech Stack:** Rust 2024, tokio, async-trait, serde, existing bob workspace crates, `genai`.

---

## Task 1: Add a failing test for pluggable prompt compaction

**Files:**

- Modify: `crates/bob-core/src/ports.rs`
- Modify: `crates/bob-runtime/src/prompt.rs`
- Modify: `crates/bob-runtime/src/lib.rs`

**Step 1: Write the failing test**

- Add prompt tests that expect prompt building to call an injected compactor rather than the hard-coded `truncate_history(...)`.
- Add runtime builder tests that expect a missing compactor to fall back to the default deterministic implementation.

**Step 2: Run test to verify it fails**

Run: `cargo test -p bob-runtime prompt`
Expected: FAIL because prompt building still truncates directly.

**Step 3: Write minimal implementation**

- Add `ContextCompactorPort` to `bob-core`.
- Add a default `WindowContextCompactor` in `bob-runtime`.
- Inject the compactor through `RuntimeBuilder` and runtime structs.
- Update prompt construction to use the port.

**Step 4: Run test to verify it passes**

Run: `cargo test -p bob-runtime prompt`
Expected: PASS.

### Task 2: Add failing tests for structured transcript compatibility

**Files:**

- Modify: `crates/bob-core/src/types.rs`
- Modify: `crates/bob-adapters/src/store_memory.rs`
- Modify: `crates/bob-adapters/src/store_file.rs`

**Step 1: Write the failing test**

- Add serde tests for legacy `{ role, content }` session payloads.
- Add transcript tests for assistant text, assistant tool-call batches, and tool-result entries.

**Step 2: Run test to verify it fails**

Run: `cargo test -p bob-core transcript`
Expected: FAIL because the structured transcript model does not exist yet.

**Step 3: Write minimal implementation**

- Introduce structured transcript types in `bob-core`.
- Keep compatibility deserialization for existing session snapshots.
- Update store tests where needed to use the new model helpers.

**Step 4: Run test to verify it passes**

Run: `cargo test -p bob-core transcript`
Expected: PASS.

### Task 3: Refactor prompt rendering and scheduler persistence to use structured transcript

**Files:**

- Modify: `crates/bob-runtime/src/prompt.rs`
- Modify: `crates/bob-runtime/src/scheduler.rs`
- Modify: `crates/bob-runtime/src/agent_loop.rs`

**Step 1: Write the failing test**

- Add prompt tests that render structured transcript entries into prompt messages.
- Add scheduler tests that expect tool calls and tool results to be persisted structurally during a native tool-calling turn.

**Step 2: Run test to verify it fails**

Run: `cargo test -p bob-runtime scheduler::tests::tc15_native_dispatch_mode_uses_llm_tool_calls`
Expected: FAIL because the transcript is still flattened.

**Step 3: Write minimal implementation**

- Replace direct `Message { role, content }` persistence in the scheduler with structured transcript entries.
- Update prompt rendering to map structured transcript entries into provider-neutral prompt messages.
- Keep `AgentLoop` tape behavior unchanged except for any type updates required by the new session model.

**Step 4: Run test to verify it passes**

Run: `cargo test -p bob-runtime scheduler`
Expected: PASS.

### Task 4: Enable native tool-calling in `llm_genai`

**Files:**

- Modify: `crates/bob-adapters/src/llm_genai.rs`

**Step 1: Write the failing test**

- Add adapter tests for:
  - `capabilities().native_tool_calling == true`,
  - request mapping with `ChatRequest::with_tools(...)`,
  - response mapping from `genai` tool calls into Bob `ToolCall`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p bob-adapters llm_genai`
Expected: FAIL because tools and tool calls are not mapped.

**Step 3: Write minimal implementation**

- Map Bob tool descriptors into `genai::chat::Tool`.
- Map provider tool calls back into Bob `ToolCall`.
- Advertise native tool-calling support.
- Keep streaming conservative if the runtime cannot carry streamed tool-call structure yet.

**Step 4: Run test to verify it passes**

Run: `cargo test -p bob-adapters llm_genai`
Expected: PASS.

### Task 5: Refresh docs and run full verification

**Files:**

- Modify: `README.md`
- Modify: `crates/bob-core/README.md`
- Modify: `crates/bob-runtime/README.md`
- Modify: `crates/bob-adapters/README.md`

**Step 1: Update docs**

- Document the new compactor port and default behavior.
- Document the structured transcript model and backward compatibility.
- Document native tool-calling support boundaries.

**Step 2: Run full verification**

Run: `just format`
Run: `just lint`
Run: `just test`
Expected: all commands pass.
