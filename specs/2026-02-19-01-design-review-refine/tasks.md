# Design Review & Simplification — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-02-19-01-design-review-refine/design.md |
| **Owner** | Project maintainers |
| **Start Date** | 2026-02-19 |
| **Target Date** | 2026-03-07 |
| **Status** | Planning |

## Summary & Phasing

This task list implements the design review findings: fix critical issues, simplify the architecture for v1, scaffold the reduced crate layout, and produce a revised design document that serves as the implementation-ready v1 spec.

- **Phase 1: Critical Fixes** — Fix broken dependency pins, validate all deps compile
- **Phase 2: Architecture Simplification** — Revise design.md with reduced crate layout and port surface
- **Phase 3: Scaffold & Validate** — Create crate structure, define trait contracts, compile
- **Phase 4: Polish & Cleanup** — Update docs, remove dead references, finalize v1 scope

---

## Phase 1: Critical Fixes

### Task 1.1: Fix genai Version Pin

> **Context:** The design document pins `genai = "=0.6.0-beta.4"` but this version does not exist on crates.io. The latest published version is `0.5.3`. This blocks all LLM adapter implementation.
> **Verification:** `cargo add genai@0.5 --workspace` succeeds and `cargo check` compiles.

- **Priority:** P0
- **Scope:** Dependency fix
- **Status:** � DONE

- [x] **Step 1:** Update `docs/design.md` Section 3 to pin `genai = "=0.6.0-beta.1"` instead of `"=0.6.0-beta.4"`
- [x] **Step 2:** Review genai `=0.6.0-beta.1` API (struct-based: `Client::exec_chat`, `Client::exec_chat_stream`) and update Section 8 code samples to match actual API
- [x] **Step 3:** Verify the `=0.6.0-beta.1` API supports tool calling with `Tool`, `ToolCall`, `ToolResponse` types
- [x] **Step 4:** Add `genai = "=0.6.0-beta.1"` to workspace `Cargo.toml`
- [x] **Verification:** `cargo check --workspace` succeeds with genai `=0.6.0-beta.1`

---

### Task 1.2: Defer models-dev Dependency

> **Context:** `models-dev` (v0.1.1) uses `reqwest` internally (conflicts with `hpx`-first project guideline), is 2021 edition, has only 545 downloads. The entire `ModelCatalogPort` + routing system depends on it but is not needed for v1.
> **Verification:** All references to `models-dev` are moved to a "Future Work" section in design.md.

- **Priority:** P0
- **Scope:** Design scope reduction
- **Status:** � DONE

- [x] **Step 1:** Remove `models-dev` from the v1 dependency list in design.md Section 3
- [x] **Step 2:** Remove `ModelCatalogPort` trait from Section 5.2
- [x] **Step 3:** Remove `adapter-models-catalog/` from workspace layout (Section 4)
- [x] **Step 4:** Remove Section 8.5 (models.dev catalog integration) and Section 8.6 (catalog-to-genai mapper) from v1 scope — move to a new "Future Work" appendix
- [x] **Step 5:** Replace model routing with simple model string config: `default_model = "openai:gpt-4o-mini"` passed directly to genai
- [x] **Verification:** Design doc has no v1 dependency on `models-dev`; model specification is a simple string config

---

### Task 1.3: Validate Remaining Dependencies

> **Context:** Must verify that `rmcp = "0.16"`, `agent-skills = "0.2"`, and other pinned deps actually exist and are compatible with Rust 2024 edition.
> **Verification:** A minimal `Cargo.toml` with all v1 deps compiles.

- **Priority:** P0
- **Scope:** Dependency validation
- **Status:** � DONE

- [x] **Step 1:** Create a test branch and add all v1 dependencies to workspace `Cargo.toml`
- [x] **Step 2:** Verify `rmcp = { version = "0.16", features = ["client", "transport-child-process", "transport-streamable-http-client"] }` compiles (NOTE: `transport-sse-client` and `transport-streamable-http` are invalid feature names; corrected to `transport-streamable-http-client`)
- [x] **Step 3:** Verify `agent-skills = "0.2"` compiles
- [x] **Step 4:** Note: rmcp feature flags corrected in both `Cargo.toml` and `docs/design.md`
- [x] **Verification:** `cargo check --workspace` compiles with all v1 deps declared

---

## Phase 2: Architecture Simplification

### Task 2.1: Reduce Crate Layout (10 → 4)

> **Context:** The current design specifies 10 crates for a framework with zero implementation. This creates unnecessary compilation overhead, wiring complexity, and cognitive load. The hexagonal architecture can be preserved with fewer crates using feature flags for adapter selection (following Rust ecosystem conventions like sqlx, tower).
> **Verification:** design.md Section 4 shows 4 crates + 1 binary with clear dependency boundaries.

- **Priority:** P0
- **Scope:** Architecture restructure in design doc
- **Status:** � DONE

- [x] **Step 1:** Rewrite Section 4 layout from 10 crates to:
  - `bob-core` (domain types, port traits, error types, policies, scheduler FSM)
  - `bob-runtime` (DefaultAgentRuntime, Bootstrap, orchestrator)
  - `bob-adapters` (all adapters behind feature flags)
  - `bin/cli-agent` (CLI binary)
- [x] **Step 2:** Define feature flags for `bob-adapters`: `llm-genai`, `mcp-rmcp`, `skills`, `store-memory`, `observe-tracing`
- [x] **Step 3:** Preserve hexagonal boundary rule: `bob-runtime` depends only on `bob-core`, never on `bob-adapters`
- [x] **Step 4:** Update dependency boundary rule text in Section 4
- [x] **Step 5:** Remove `adapter-store-sqlite`, `adapter-models-catalog` entirely from v1 layout
- [x] **Verification:** Layout is 4 crates; dependency arrows form a clean DAG with no circularity

---

### Task 2.2: Reduce Port Traits (8 → 4)

> **Context:** 8 internal port traits means 8 mock implementations, 8 adapter implementations, and 8 wiring points. For v1, only 4 ports represent actual external boundaries: LLM, Tools, Storage, Observability. Skills are static config. Model catalog, checkpoints, and artifacts are deferred.
> **Verification:** design.md Section 5.2 defines exactly 4 port traits.

- **Priority:** P0
- **Scope:** Trait API design in design doc
- **Status:** � DONE

- [x] **Step 1:** Keep `LlmPort` — remove `capabilities()` and `estimate_tokens()` methods (use config-driven approach for v1)
- [x] **Step 2:** Rename `ToolCatalogPort` → `ToolPort`, remove `TurnContext` parameter (policy enforced by scheduler, not port)
- [x] **Step 3:** Simplify `SessionStorePort` → `SessionStore`, remove `expected_version` CAS (single-process v1)
- [x] **Step 4:** Simplify `EventSinkPort` → `EventSink`, make `emit()` synchronous (fire-and-forget)
- [x] **Step 5:** Move `SkillPort`, `ModelCatalogPort`, `TurnCheckpointStorePort`, `ArtifactStorePort` to "Future Work" appendix
- [x] **Verification:** Section 5.2 has exactly 4 trait definitions with minimal method surfaces

---

### Task 2.3: Simplify Domain Model

> **Context:** The domain model carries types for suspend/resume, model catalog, and artifact storage that are not needed for v1. These types create implementation obligations and complicate the scheduler FSM.
> **Verification:** Domain model section lists only types used in the v1 turn loop.

- **Priority:** P1
- **Scope:** Type design in design doc
- **Status:** � DONE

- [x] **Step 1:** Remove `Suspended` variant from `AgentRunResult` for v1
- [x] **Step 2:** Remove `ResumeRequest`, `ResumeToken`, `TurnCheckpoint`, `SuspendReason` types from v1
- [x] **Step 3:** Remove `VersionedSessionState` — use plain `SessionState` for v1
- [x] **Step 4:** Remove `ModelProfile`, `ModelSelector`, `ModelPreference` — use `String` model ID for v1
- [x] **Step 5:** Keep core types: `AgentRequest`, `AgentResponse`, `AgentRunResult`, `AgentAction`, `SessionState`, `ToolDescriptor`, `ToolCall`, `ToolResult`, `AgentEvent`, `AgentStreamEvent`
- [x] **Step 6:** Simplify `TurnContext` to a plain struct: `{ session_id, trace_id, policy }`
- [x] **Verification:** No references to deferred types remain in v1 trait signatures or scheduler pseudocode

---

### Task 2.4: Simplify Scheduler FSM (11 → 6 states)

> **Context:** The current FSM has 11 states including skill reselection, history condensation, and suspend. For v1 the loop is: build prompt → infer → parse action → (final | tool call + loop). Skills are pre-loaded. History truncation is deterministic.
> **Verification:** FSM diagram and pseudocode show 6 states; all transition tests are self-contained.

- **Priority:** P1
- **Scope:** Scheduler design in design doc
- **Status:** � DONE

- [x] **Step 1:** Rewrite Section 7.1 states to: `Start`, `BuildPrompt`, `LlmInfer`, `ParseAction`, `CallTool`, `Done`
- [x] **Step 2:** Update Mermaid diagram (Section 7.2) to match 6 states
- [x] **Step 3:** Simplify pseudocode (Section 7.4): remove skill reselection, remove LLM-based condensation (use deterministic truncation), remove suspend path
- [x] **Step 4:** Keep: loop guard defaults, cancellation via CancellationToken, streaming semantics
- [x] **Step 5:** Move full 11-state FSM with skill reselection and condensation to "Future Work" section
- [x] **Verification:** Pseudocode compiles conceptually with only 4 port trait dependencies

---

### Task 2.5: Defer Sub-Agent System

> **Context:** Section 11 (Sub-agent system) is ~15% of the design document. It depends on a stable single-agent runtime that doesn't exist yet. The hexagonal architecture makes adding this later trivial (implement `ToolPort` for a sub-agent).
> **Verification:** Section 11 is moved entirely to "Future Work" appendix.

- **Priority:** P1
- **Scope:** Scope reduction in design doc
- **Status:** � DONE

- [x] **Step 1:** Move entire Section 11 to a "Future Work: Sub-Agent System" appendix
- [x] **Step 2:** Remove `SubAgentToolAdapter`, `SubAgentProfile`, `SubAgentId` types from v1 domain model
- [x] **Step 3:** Remove sub-agent config section from Section 15 (`[[subagents]]` TOML block)
- [x] **Step 4:** Add a one-sentence forward reference in the main design: "Sub-agent composition via the `ToolPort` interface is planned for v1.1. See Future Work appendix."
- [x] **Verification:** No sub-agent types, config, or logic remain in v1 sections

---

### Task 2.6: Simplify Configuration Surface

> **Context:** The example `agent.toml` has ~60 lines with many knobs that correspond to deferred features (resume, model catalog, sub-agents). A minimal v1 config should have ~20 lines covering runtime, LLM, MCP servers, and skills.
> **Verification:** Section 15 shows a minimal config with only v1 knobs.

- **Priority:** P2
- **Scope:** Config design in design doc
- **Status:** � DONE

- [x] **Step 1:** Rewrite `agent.toml` example to v1 minimal surface: `[runtime]`, `[llm]`, `[policy]`, `[[mcp.servers]]`, `[[skills.sources]]`
- [x] **Step 2:** Remove: `[runtime.resume]`, `[models_catalog]`, `[mcp.pool.scope]`, `[[subagents]]`, `[prompt]` ratio configs
- [x] **Step 3:** Keep environment interpolation (`${VAR_NAME}`) and config loading order
- [x] **Verification:** Config example is < 25 lines with only v1 features

---

## Phase 3: Scaffold & Validate

### Task 3.1: Scaffold Crate Structure

> **Context:** Create the 4-crate layout in the workspace. This validates that the dependency boundaries work and the feature flag approach compiles.
> **Verification:** `cargo check --workspace` succeeds with empty crates.

- **Priority:** P0
- **Scope:** Project scaffolding
- **Status:** � DONE

- [x] **Step 1:** Rename/repurpose `crates/common/` to `crates/bob-core/` with updated `Cargo.toml` (name = "bob-core")
- [x] **Step 2:** Create `crates/bob-runtime/` with dependency on `bob-core`
- [x] **Step 3:** Create `crates/bob-adapters/` with feature-gated dependencies on `genai`, `rmcp`, `agent-skills`, `tracing`
- [x] **Step 4:** Create `bin/cli-agent/` with dependencies on `bob-runtime` and `bob-adapters`
- [x] **Step 5:** Update workspace `Cargo.toml` members
- [x] **Step 6:** Add v1 workspace dependencies: `genai`, `rmcp`, `agent-skills`, `async-trait`, `tokio`, `serde`, `serde_json`, `thiserror`, `tracing`, `futures-core`, `tokio-util`
- [x] **Verification:** `cargo check --workspace` compiles; `cargo test --workspace` passes

---

### Task 3.2: Define Port Traits in bob-core

> **Context:** Define the 4 v1 port traits with their minimal method signatures. This is the hexagonal contract that all adapters must implement.
> **Verification:** Traits compile; mock implementations can be written inline.

- **Priority:** P0
- **Scope:** Trait API implementation
- **Status:** � DONE

- [x] **Step 1:** Define `LlmPort` trait: `complete()`, `complete_stream()`
- [x] **Step 2:** Define `ToolPort` trait: `list_tools()`, `call_tool()`
- [x] **Step 3:** Define `SessionStore` trait: `load()`, `save()`
- [x] **Step 4:** Define `EventSink` trait: `emit()`
- [x] **Step 5:** Define all domain types: `AgentRequest`, `AgentResponse`, `AgentRunResult`, `AgentAction`, `SessionState`, `ToolDescriptor`, `ToolCall`, `ToolResult`, `AgentError`, `LlmError`, `ToolError`, `StoreError`
- [x] **Step 6:** Define `TurnPolicy` struct with guard defaults
- [x] **Step 7:** Write mock implementations for all 4 traits (in `#[cfg(test)]` module)
- [x] **Verification:** `cargo test -p bob-core` passes with mock-based tests

---

### Task 3.3: Define Bootstrap + Runtime Traits

> **Context:** Define `AgentBootstrap` and `AgentRuntime` in `bob-runtime`. These are the external API that framework users interact with.
> **Verification:** Traits compile; a dummy implementation compiles in `bob-runtime`.

- **Priority:** P0
- **Scope:** Public API implementation
- **Status:** � DONE

- [x] **Step 1:** Define `AgentBootstrap` trait in `bob-runtime`: `register_mcp_server()`, `build()`
- [x] **Step 2:** Define `AgentRuntime` trait: `run()`, `run_stream()`, `health()`
- [x] **Step 3:** Create `DefaultAgentRuntime` struct that holds `Arc<dyn LlmPort>`, `Arc<dyn ToolPort>`, `Arc<dyn SessionStore>`, `Arc<dyn EventSink>`
- [x] **Step 4:** Implement stub `AgentRuntime` for `DefaultAgentRuntime` (returns placeholder results)
- [x] **Verification:** `cargo check -p bob-runtime` compiles

---

## Phase 4: Polish & Cleanup

### Task 4.1: Write Future Work Appendix

> **Context:** Deferred features (model catalog, sub-agents, suspend/resume, SQLite, skill runtime selection) should be preserved as future work reference, not discarded.
> **Verification:** All deferred content is in a clearly labeled "Future Work" appendix in design.md.

- **Priority:** P2
- **Scope:** Documentation
- **Status:** � DONE

- [x] **Step 1:** Create "Appendix A: Future Work" section at end of design.md
- [x] **Step 2:** Move models.dev catalog integration (§8.5, §8.6) to appendix
- [x] **Step 3:** Move sub-agent system (§11) to appendix
- [x] **Step 4:** Move suspend/resume types and checkpoint store to appendix
- [x] **Step 5:** Move SQLite adapter, artifact store, optimistic concurrency to appendix
- [x] **Step 6:** Move skill runtime selection and token budget management to appendix
- [x] **Step 7:** Add version targets for each deferred feature (v1.1 vs v2)
- [x] **Verification:** Main design sections contain only v1 scope; appendix preserves all deferred content

---

### Task 4.2: Update Implementation Phase Plan

> **Context:** Section 18 (Implementation Plan) needs to be realigned with the simplified v1 scope. The 10-phase plan should be reduced to match the 4-crate layout.
> **Verification:** Implementation plan matches the task list in this document.

- **Priority:** P2
- **Scope:** Documentation
- **Status:** � DONE

- [x] **Step 1:** Rewrite Section 18 with 5 phases: Foundation → Scheduler → Adapters → CLI Integration → Polish
- [x] **Step 2:** Remove phases for: model catalog, sub-agents, SQLite, observability dashboards
- [x] **Step 3:** Update Section 19 (API freeze criteria) to reference only v1 traits
- [x] **Step 4:** Update Section 21 (Next Steps) to match phase 1 of revised plan
- [x] **Verification:** Implementation plan is achievable in 2-3 weeks, not 2-3 months

---

### Task 4.3: Add rmcp Dyn-Compatibility Note

> **Context:** `rmcp` 0.16.x `ServerHandler` and `ClientHandler` use RPITIT and are **not dyn-compatible**. The design assumes `Arc<dyn>` dispatch everywhere but the MCP adapter must use concrete types or `into_dyn()`. This constraint must be documented.
> **Verification:** Section 9 notes the constraint and describes the adapter strategy.

- **Priority:** P1
- **Scope:** Design constraint documentation
- **Status:** � DONE

- [x] **Step 1:** Add a note in Section 9 that `rmcp` handler traits are not dyn-compatible
- [x] **Step 2:** Document that the MCP adapter uses concrete types internally and exposes them through `ToolPort` (which IS dyn-compatible via `async_trait`)
- [x] **Step 3:** Note `RunningService::into_dyn()` for managing heterogeneous MCP server connections
- [x] **Verification:** Design doc acknowledges the rmcp dispatch constraint with a concrete workaround

---

### Task 4.4: Run Quality Checks

> **Context:** After all design doc changes, verify the workspace compiles and passes all quality checks.
> **Verification:** `just ci` passes.

- **Priority:** P1
- **Scope:** Quality assurance
- **Status:** � DONE

- [x] **Step 1:** Run `just format`
- [x] **Step 2:** Run `just lint`
- [x] **Step 3:** Run `just test`
- [x] **Step 4:** Verify no Chinese characters: `just check-cn` returns no results
- [x] **Verification:** All commands pass cleanly

---

## Summary & Timeline

| Phase | Tasks | Target Date |
| :--- | :---: | :--- |
| **1. Critical Fixes** | 3 | 02-20 |
| **2. Architecture Simplification** | 6 | 02-24 |
| **3. Scaffold & Validate** | 3 | 02-28 |
| **4. Polish & Cleanup** | 4 | 03-07 |
| **Total** | **16** | |

## Definition of Done

1. [ ] **Linted:** `just lint` passes with no errors or warnings.
2. [ ] **Tested:** `just test` passes; mock-based trait tests exist for all 4 ports.
3. [ ] **Formatted:** `just format` applied.
4. [ ] **Verified:** Each task's specific Verification criterion met.
5. [ ] **English only:** No Chinese in comments or docs.
6. [ ] **Design doc updated:** `docs/design.md` reflects v1 scope with Future Work appendix.
7. [ ] **Compiles:** `cargo check --workspace` succeeds with the 4-crate layout.
