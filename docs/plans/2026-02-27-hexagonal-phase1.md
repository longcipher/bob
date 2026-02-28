# Hexagonal Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Introduce finer-grained framework extension points (tool catalog/execution split, policy and approval ports, capability metadata) without breaking current CLI behavior.

**Architecture:** Keep `bob-core` as contract-first crate and extend it with new ports/types. Wire defaults in `bob-runtime` so existing composition roots keep working. Enforce new behavior in scheduler with explicit policy and approval checks.

**Tech Stack:** Rust 2024, tokio, async-trait, serde, existing bob workspace crates.

---

## Task 1: Add failing tests for new policy/approval behavior

**Files:**

- Modify: `crates/bob-runtime/src/scheduler.rs`

**Step 1: Write failing tests**

- Add a test that expects denied approval to block tool execution and produce an error tool result.
- Add a test that expects policy denial to block tool execution before adapter call.

**Step 2: Run targeted tests to confirm failures**

- Run: `cargo test -p bob-runtime scheduler::tests::...`
- Expected: compile/test failure because policy/approval ports are not wired yet.

### Task 2: Introduce granular ports and domain types in bob-core

**Files:**

- Modify: `crates/bob-core/src/types.rs`
- Modify: `crates/bob-core/src/ports.rs`
- Modify: `crates/bob-core/src/lib.rs`

**Step 1: Add minimal types**

- `LlmCapabilities`
- `ApprovalContext`
- `ApprovalDecision`

**Step 2: Split tool concerns into ports**

- Add `ToolCatalogPort` and `ToolExecutorPort`.
- Redefine `ToolPort` as composition trait over both.

**Step 3: Add policy and approval ports**

- Add `ToolPolicyPort` with default static implementation behavior.
- Add `ApprovalPort` contract for per-tool gating.

**Step 4: Run `bob-core` tests**

- Run: `cargo test -p bob-core`

### Task 3: Wire defaults and scheduler integration in bob-runtime

**Files:**

- Modify: `crates/bob-runtime/src/lib.rs`
- Modify: `crates/bob-runtime/src/scheduler.rs`

**Step 1: Wire builder defaults**

- Add optional fields for `ToolPolicyPort` and `ApprovalPort`.
- Provide default runtime-safe implementations when not configured.

**Step 2: Enforce policy + approval in scheduler**

- Check policy before execution.
- Check approval asynchronously before tool invocation.

**Step 3: Make tests pass**

- Run targeted scheduler tests and fix compile/runtime issues.

### Task 4: Update adapters to new tool port split and verify workspace

**Files:**

- Modify: `crates/bob-adapters/src/mcp_rmcp.rs`
- Modify: `crates/bob-runtime/src/composite.rs`
- Modify: `crates/bob-runtime/src/tooling.rs`
- Modify: other tool-port implementers as required by compiler

**Step 1: Implement new trait split where needed**

- Ensure each tool adapter implements both `ToolCatalogPort` and `ToolExecutorPort`.

**Step 2: Run full verification**

- Run: `cargo test -p bob-core`
- Run: `cargo test -p bob-runtime`
- Run: `cargo test -p bob-adapters`

**Step 3: Run workspace smoke verification**

- Run: `cargo test`
- Confirm all relevant crates remain green.
