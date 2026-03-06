# Runtime DX Hooks Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Bob's interactive surface consistent and hook-friendly by routing the CLI through `AgentLoop`, exposing session usage commands, and adding fanout event delivery.

**Architecture:** Keep `bob-core` contract-first. Extend the event model just enough for practical sinks, add a concrete fanout sink in `bob-adapters`, and wire `cli-agent` through the existing `AgentLoop` facade instead of maintaining a separate command stack.

**Tech Stack:** Rust 2024, tokio, async-trait, serde, existing bob workspace crates.

---

## Task 1: Add failing tests for usage commands and event fanout

**Files:**

- Modify: `crates/bob-runtime/src/router.rs`
- Modify: `crates/bob-runtime/src/agent_loop.rs`
- Modify: `crates/bob-adapters/src/observe.rs`

**Step 1: Write the failing test**

- Add a router test for `/usage`.
- Add an `AgentLoop` test that expects `/usage` to render cumulative session usage from a stub store.
- Add an observability test that expects a fanout sink to deliver the same event to two child sinks.

**Step 2: Run test to verify it fails**

Run: `cargo test -p bob-runtime usage`
Expected: FAIL because `/usage` is not routed and `AgentLoop` has no store-backed usage rendering.

Run: `cargo test -p bob-adapters fanout`
Expected: FAIL because `FanoutEventSink` does not exist yet.

### Task 2: Extend runtime/core types for usage-aware hooks

**Files:**

- Modify: `crates/bob-core/src/types.rs`
- Modify: `crates/bob-adapters/src/observe.rs`
- Modify: `crates/bob-runtime/src/scheduler.rs`

**Step 1: Add minimal event metadata**

- Add `session_id` to turn lifecycle events.
- Add `session_id` and `step` to LLM/tool/error events.
- Add final `usage` to `TurnCompleted`.

**Step 2: Update existing sinks and scheduler emissions**

- Keep `TracingEventSink` behavior simple and structured.
- Update all scheduler emission sites and tests to compile against the richer event model.

**Step 3: Run targeted tests**

Run: `cargo test -p bob-runtime scheduler`
Expected: PASS after event emissions and tests are updated.

### Task 3: Add `FanoutEventSink`

**Files:**

- Modify: `crates/bob-adapters/src/observe.rs`

**Step 1: Write minimal implementation**

- Add a `FanoutEventSink` storing `Vec<Arc<dyn EventSink>>`.
- Add `new()` and `with_sink(...)`.
- Forward cloned events to each child sink.

**Step 2: Run targeted tests**

Run: `cargo test -p bob-adapters observe`
Expected: PASS with the new fanout coverage.

### Task 4: Upgrade `AgentLoop` and switch the CLI to use it

**Files:**

- Modify: `crates/bob-runtime/src/router.rs`
- Modify: `crates/bob-runtime/src/agent_loop.rs`
- Modify: `bin/cli-agent/src/bootstrap.rs`
- Modify: `bin/cli-agent/src/main.rs`

**Step 1: Write the failing test**

- Add or extend `AgentLoop` tests for `/usage`.
- Add CLI command parsing tests only where still needed after simplification.

**Step 2: Implement minimal code**

- Add `/usage` command routing and help text.
- Give `AgentLoop` optional access to `SessionStore`.
- Return/store the dependencies needed by the CLI bootstrap.
- Replace custom CLI command execution with `AgentLoop::handle_input(...)`.

**Step 3: Run targeted tests**

Run: `cargo test -p bob-runtime agent_loop`
Run: `cargo test -p cli-agent`
Expected: PASS.

### Task 5: Refresh documentation and verify workspace quality

**Files:**

- Modify: `README.md`
- Modify: `crates/bob-runtime/README.md`
- Modify: `crates/bob-core/README.md`

**Step 1: Update public docs**

- Fix version references to `0.2.1`.
- Document `AgentLoop`-driven CLI commands including `/usage`.
- Mention fanout event sinks as the recommended hook composition path.

**Step 2: Run full verification**

Run: `just format`
Run: `just lint`
Run: `just test`
Expected: all commands pass.
