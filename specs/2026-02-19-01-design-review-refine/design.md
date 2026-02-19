# Design Document: Design Review & Simplification

| Metadata | Details |
| :--- | :--- |
| **Author** | pb-plan agent |
| **Status** | Draft |
| **Created** | 2026-02-19 |
| **Reviewers** | Project maintainers |
| **Related Issues** | N/A |

## 1. Executive Summary

**Problem:** The current `docs/design.md` is an impressive and thorough technical blueprint, but it exhibits significant over-engineering for a v1 framework that has **zero lines of implementation**. It defines 10+ crates, 8 internal port traits, a full sub-agent system, a model catalog with ranking strategies, resume/checkpoint persistence with signed tokens, and history condensation via LLM — all before a single line of the scheduler loop has compiled. This level of upfront specification creates paralysis, massive implementation surface, and deferred learning from real usage.

**Solution:** Perform a dialectical review identifying what to **keep**, what to **simplify**, what to **defer**, and what to **fix**. Produce a refined design that preserves the hexagonal architecture's core value (testability, swappable adapters) while cutting the v1 scope to the minimum viable agent loop: LLM inference → action parsing → tool execution → loop. Ship that, learn from it, then grow.

---

## 2. Requirements & Goals

### 2.1 Problem Statement

The design document has accumulated 3 rounds of reviewer feedback and now carries the weight of every edge case considered upfront. Specific pain points:

1. **10+ crate layout** creates a massive compilation graph, complex dependency wiring, and high cognitive overhead before any value is delivered.
2. **8 internal port traits** (`LlmPort`, `ModelCatalogPort`, `ToolCatalogPort`, `SkillPort`, `SessionStorePort`, `TurnCheckpointStorePort`, `ArtifactStorePort`, `EventSinkPort`) is excessive for v1. Each trait means an adapter crate, mock implementations, contract tests, and wiring code.
3. **Sub-agent system** (Section 11) with depth guards, cycle detection, budget slicing, and async delegation is a v2+ feature masquerading as v1.
4. **Model catalog with ranking strategies** (`cost_first`, `latency_first`, `quality_first`) and `models.dev` integration adds complexity that no user needs before basic tool-calling works.
5. **Critical dependency issue**: `genai = "=0.6.0-beta.4"` does **not exist** on crates.io. Latest published is `0.5.3`.
6. **`models-dev` crate** uses `reqwest` internally (conflicts with `hpx`-first guideline), has 2021 edition, and only 545 total downloads.
7. **`agent-skills` crate** has only 44 total downloads and is purely synchronous parsing. The adapter layer must do all composition work.

### 2.2 Functional Goals

1. **Identify critical bugs** in the design (wrong dependency versions, impossible API contracts).
2. **Classify each design section** as Keep / Simplify / Defer for v1.
3. **Propose a minimal crate layout** that preserves hexagonal boundaries.
4. **Simplify the port trait surface** to what v1 actually needs.
5. **Align with Rust 2024 edition** idioms (native async traits where possible).
6. **Ensure the framework is easy to understand** for a developer reading the code for the first time.

### 2.3 Non-Functional Goals

- **Performance:** No unnecessary indirection layers. Dynamic dispatch (`Arc<dyn>`) is fine for orchestration-level code (LLM calls dwarf vtable lookups by 6+ orders of magnitude), but avoid gratuitous abstraction layers that add cognitive cost without performance benefit.
- **Compile time:** Fewer crates = faster incremental builds. Feature flags over separate crates where sensible.
- **Simplicity:** A new contributor should understand the full architecture by reading ~3 files, not 10+ crate READMEs.

### 2.4 Out of Scope

- Rewriting the design from scratch. The hexagonal architecture and turn-loop FSM are sound — we refine, not replace.
- Implementing any code. This review produces design refinements and tasks only.

### 2.5 Assumptions

- The framework's primary users are Rust developers building LLM-powered agents.
- v1 must demonstrate: configure → run agent → LLM calls → tool execution → return result.
- Sub-agent composition, model catalog routing, and cross-process resume are real needs, but for v1.1+.

---

## 3. Architecture Overview

### 3.1 System Context

Bob is an agent framework. Its value is the **turn loop** (scheduler FSM) that orchestrates LLM calls, tool execution, and policy enforcement. Everything else is plumbing to feed the loop.

The current design correctly identifies this but buries it under layers of abstraction that aren't needed until the loop itself is proven.

### 3.2 Key Design Principles

1. **YAGNI aggressively.** Don't abstract what you haven't implemented twice.
2. **One crate until it hurts.** Split crates when compilation time or dependency isolation demands it, not preemptively.
3. **Traits for boundaries, not for everything.** Trait-ify external I/O (LLM, tools, storage). Don't trait-ify internal coordination.
4. **Ship the loop first.** The scheduler FSM is the product. Everything else is infrastructure.
5. **Grow by addition, not by prediction.** The hexagonal port system makes future adapters trivial to add — that's its purpose. Don't pre-build adapters for systems you haven't integrated yet.

### 3.3 Existing Components to Reuse

| Component | Location | How to Reuse |
| :--- | :--- | :--- |
| `common` crate | `crates/common/` | Repurpose as `bob-core` or remove if empty |
| Workspace lint config | `Cargo.toml` | Keep as-is — excellent pedantic setup |
| Justfile tasks | `Justfile` | Keep `format`, `lint`, `test` workflow |

---

## 4. Detailed Design

### 4.1 CRITICAL ISSUE: `genai` Version Pin

**Current:** `genai = "=0.6.0-beta.4"`
**Reality:** This version does not exist on crates.io. Latest published is `=0.6.0-beta.1`.

**Action required:** Pin to `=0.6.0-beta.1`

### 4.2 CRITICAL ISSUE: `async_trait` vs Native Async Traits

**Current design:** Uses `#[async_trait]` on all 8 port traits, intended for `Arc<dyn Trait>` dispatch.

**Rust 2024 reality:** Native `async fn` in traits is stable since Rust 1.75, but native async trait methods return opaque types that are **not dyn-compatible**. For `dyn Trait` usage, you still need either:

- `async_trait` (adds heap allocation per call), or
- Manual desugaring: `fn foo(&self) -> Pin<Box<dyn Future<Output = R> + Send + '_>>`, or
- The `trait-variant` crate for auto-generating dyn-compatible variants.

**Recommendation:** Keep `async_trait` for port traits that need `dyn` dispatch. This is pragmatic. The heap allocation is negligible compared to LLM round-trips. But **reduce the number of port traits** so the cost is contained.

### 4.3 Section-by-Section Review

#### Section 2: Design Principles — **KEEP**

Sound. The 7 principles are good. No changes needed.

#### Section 3: Version Baseline — **FIX**

| Dependency | Current Pin | Issue | Recommended |
| :--- | :--- | :--- | :--- |
| `genai` | `=0.6.0-beta.4` | Does not exist | `"=0.6.0-beta.1"` |
| `rmcp` | `0.16` | OK | Keep |
| `agent-skills` | `0.2` | 44 downloads, very early | Keep but note risk |
| `models-dev` | `0.1.1` | Uses `reqwest`, 2021 edition | **Defer** to v1.1 |
| `async-trait` | `0.1` | Needed for dyn dispatch | Keep |

#### Section 4: Workspace Layout — **SIMPLIFY**

**Current:** 10 crates + 2 binaries.

```text
agent-core/
agent-runtime/
adapter-llm-genai/
adapter-models-catalog/
adapter-mcp-rmcp/
adapter-skills-agent/
adapter-store-memory/
adapter-store-sqlite/
adapter-observe/
```

**Problem:** This is 10 crates with zero lines of code. Each crate means:

- `Cargo.toml` boilerplate
- `lib.rs` with exports
- dependency wiring
- mock implementations for testing
- mental overhead to locate code

**Proposed v1 layout (4 crates + 1 binary):**

```text
crates/
  bob-core/              # Domain types, port traits, error types, policies, scheduler FSM
  bob-runtime/           # DefaultAgentRuntime, Bootstrap, orchestrator
  bob-adapters/          # All adapter implementations behind feature flags
    src/
      llm_genai.rs       # genai adapter
      mcp_rmcp.rs        # rmcp adapter
      skills_agent.rs    # agent-skills adapter
      store_memory.rs    # in-memory stores
      observe_tracing.rs # tracing + events
      lib.rs             # feature-gated re-exports
bin/
  cli-agent/             # CLI example
```

**Rationale:**

- `bob-core` = the hexagonal "inside" (types + ports). Zero external dependencies except `serde`, `thiserror`, `async-trait`.
- `bob-runtime` = the application shell. Depends on `bob-core` traits only.
- `bob-adapters` = all adapters in one crate with feature flags. This is the Rust convention (see `sqlx`, `tower`, `tonic`). Split into separate crates only when compile time or dependency conflict forces it.
- Hexagonal boundary is preserved: `bob-runtime` depends on `bob-core`, not on `bob-adapters`. Wiring happens in `bin/`.

**Feature flags for `bob-adapters`:**

```toml
[features]
default = ["llm-genai", "store-memory", "observe-tracing"]
llm-genai = ["dep:genai"]
mcp-rmcp = ["dep:rmcp"]
skills = ["dep:agent-skills"]
store-memory = []
store-sqlite = ["dep:sqlx"]     # v1.1+
observe-tracing = ["dep:tracing"]
```

This preserves the ability to split later while avoiding premature crate explosion.

#### Section 5: Unified Trait Strategy — **SIMPLIFY**

**`AgentBootstrap` + `AgentRuntime` split — KEEP.** This is a good pattern (builder/execution separation). Well-established in Rust (e.g., `hyper::Server::bind().serve()`).

**8 internal port traits — REDUCE TO 4 for v1:**

| Keep for v1 | Rationale |
| :--- | :--- |
| `LlmPort` | Core: LLM access is the fundamental capability |
| `ToolPort` | Core: tool execution is the primary agent action (rename from `ToolCatalogPort`) |
| `SessionStore` | Needed: conversation history management (rename, simplify) |
| `EventSink` | Needed: observability is essential from day 1 |

| Defer to v1.1+ | Rationale |
| :--- | :--- |
| `ModelCatalogPort` | Over-engineering: pass model string directly to genai for v1 |
| `SkillPort` | Simplify: load skills at bootstrap time, pass as config, don't need runtime trait |
| `TurnCheckpointStorePort` | Defer: suspend/resume across processes is v1.1+ |
| `ArtifactStorePort` | Defer: store tool results inline in session history |

**Simplified port traits for v1:**

```rust
/// LLM completion port
#[async_trait]
pub trait LlmPort: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError>;
}

/// Tool discovery and execution port
#[async_trait]
pub trait ToolPort: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, ToolError>;
    async fn call_tool(&self, call: ToolCall) -> Result<ToolResult, ToolError>;
}

/// Session persistence port
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn load(&self, id: &SessionId) -> Result<Option<SessionState>, StoreError>;
    async fn save(&self, id: &SessionId, state: &SessionState) -> Result<(), StoreError>;
}

/// Observability event sink
#[async_trait]
pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}
```

**Key simplifications:**

- `ToolPort` drops `TurnContext` parameter. For v1, policies are applied in the scheduler before calling the port, not inside the port.
- `SessionStore` drops optimistic concurrency (`expected_version`). Single-process v1 doesn't need CAS. Add it when multi-replica is needed.
- `EventSink::emit` is sync (fire-and-forget to a channel). Don't make observability async.
- `LlmPort` drops `capabilities()` and `estimate_tokens()`. For v1, prompt budget is managed by the caller using model config. Add these when multi-model routing is needed.

#### Section 6: Domain Model — **SIMPLIFY**

**Keep:** `AgentRequest`, `AgentResponse`, `AgentRunResult`, `AgentAction`, `SessionState`, `ToolDescriptor`, `ToolCall`, `ToolResult`.

**Simplify:**

- `AgentRunResult`: Remove `Suspended` variant for v1. The turn either finishes or errors. Add suspend/resume in v1.1.
- Drop `ResumeRequest`, `ResumeToken`, `TurnCheckpoint`, `VersionedSessionState` for v1.
- Drop `ModelProfile`, `ModelSelector`, `ModelPreference` for v1. Use a direct model string (`"openai:gpt-4o-mini"`).
- `TurnContext` can be a simple struct with `session_id`, `trace_id`, `policies` — not a god-context.

**Simplified `AgentRunResult`:**

```rust
pub enum AgentRunResult {
    Finished(AgentResponse),
}
```

Yes, this looks trivially simple. That's the point. When you need `Suspended`, the enum is trivially extensible. Don't carry the resume machinery until then.

#### Section 7: Scheduler FSM — **KEEP CORE, SIMPLIFY STATES**

The FSM concept is the right approach. But the 11-state machine is over-specified for v1.

**Proposed v1 states (6 states):**

```text
Start → BuildPrompt → LlmInfer → ParseAction
  → Final → Done
  → ToolCall → AppendResult → GuardCheck → BuildPrompt (loop)
  → GuardExceeded → Done
```

**Removed for v1:**

- `LoadSessionOrCheckpoint` → just `Load` (no checkpoint)
- `SelectSkillsOnce` → skills are pre-loaded at bootstrap, injected into prompt template
- `MaybeCondenseHistory` → simple truncation (drop oldest messages) for v1
- `MaybeReselectSkills` → no reselection for v1
- `Suspend` → deferred to v1.1

**Keep from Section 7:**

- Loop guard defaults (max_steps=12, max_tool_calls=8, etc.) — good.
- Cancellation via `CancellationToken` — good.
- The pseudocode structure is sound, just remove the skill reselection and condensation branches.

#### Section 8: LLM Adapter (genai) — **FIX + SIMPLIFY**

**Fix:** Align with genai 0.5.x API (not 0.6.x).

**Keep:**

- Dual mode (`NativeToolCallMode` / `JsonActionMode`) — this is genuinely valuable.
- Retry policy (transport + 429 only) — good.
- `JsonActionMode` prompt contract — good.

**Simplify:**

- Drop `ProviderRouter` for v1. genai already handles provider resolution via model string format (`"openai:gpt-4o-mini"`).
- Drop `capability-aware prompting` as a formal system. For v1, use a config flag per model profile. A hashmap of `model_id → ModelConfig { supports_tools: bool, supports_system: bool, max_tokens: usize }` is sufficient.
- Drop `estimate_tokens` from `LlmPort`. Use a simple heuristic (chars / 4) for v1 prompt budgeting. Exact token counting is model-specific and not available in genai.

#### Section 8.5-8.6: models.dev Catalog — **DEFER ENTIRELY**

**Rationale:**

- `models-dev` crate uses `reqwest` (conflicts with project guidelines)
- Only 545 downloads, 2021 edition
- The full catalog routing system (`ModelCatalogPort`, `ModelSelector`, `ModelPreference`, ranking strategies) is the largest single source of complexity in the design
- For v1, a model string + a small config struct is sufficient

**v1 alternative:** Simple model configuration in TOML:

```toml
[models.default]
id = "openai:gpt-4o-mini"
max_context_tokens = 128000
supports_tools = true

[models.summarizer]
id = "openai:gpt-4o-mini"
max_context_tokens = 128000
supports_tools = false
```

Add `ModelCatalogPort` and `models.dev` integration in v1.1 when users actually need dynamic model routing.

#### Section 9: MCP Adapter (rmcp) — **KEEP, MINOR SIMPLIFY**

This section is well-designed. MCP integration is a core v1 feature.

**Keep:**

- Connection pool concept
- Namespace convention (`mcp/<server_id>/<tool_name>`)
- Stdio transport safety
- Reconnect/circuit breaker policies

**Simplify:**

- Drop "per-tenant isolation" for v1 (single-tenant framework use)
- Drop "serverless mode" (ephemeral pool per request) for v1
- `McpConnectionPool` doesn't need to be process-wide in v1. An instance-scoped pool is simpler.

**Important note on rmcp:** `ServerHandler` and `ClientHandler` are **not dyn-compatible** (they use RPITIT). The adapter must use concrete types or `into_dyn()`. The design should note this constraint.

#### Section 10: Skills Adapter — **SIMPLIFY**

`agent-skills` is a pure parsing library (sync, no runtime features). The elaborate `SkillPort` trait with selection algorithms, token budgeting, and reselection policies is building a runtime on top of a parser.

**v1 approach:**

1. Load skills from directory at bootstrap time using `agent-skills`.
2. Store parsed skills in a `Vec<Skill>` on the runtime.
3. Inject selected skill content into the system prompt via config.
4. No runtime trait needed. Skills are static configuration, not a dynamic port.

**Defer to v1.1:**

- Dynamic skill selection and scoring
- Token budget management for skills
- Skill reselection during turn loop
- `SkillPort` trait

#### Section 11: Sub-Agent System — **DEFER ENTIRELY**

**Rationale:** This is a v2 feature. The core agent loop doesn't exist yet. Sub-agent composition depends on a stable, tested single-agent runtime. Building sub-agent depth guards, cycle detection, and budget slicing before the single-agent turn loop works is premature.

**v1.1 approach:** When the single-agent loop is proven, add `SubAgentToolAdapter` that wraps an `AgentRuntime` as a `ToolPort` entry. The hexagonal design makes this a natural extension.

#### Section 12: Policy and Guardrails — **KEEP, SIMPLIFY**

**Keep:**

- Tool allowlist/denylist pattern
- Risk levels (low/medium/high)

**Simplify:**

- Drop "human approval requirement" for v1 (requires suspend/resume)
- Drop argument schema validation for v1 (MCP servers already validate)
- Policy is a struct, not a trait for v1

```rust
pub struct TurnPolicy {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_consecutive_errors: u32,
    pub turn_timeout: Duration,
    pub tool_timeout: Duration,
    pub allowed_tools: Option<Vec<String>>,
    pub denied_tools: Vec<String>,
}
```

#### Section 13: Persistence and Memory — **SIMPLIFY**

**v1:** In-memory `SessionStore` only. `HashMap<SessionId, SessionState>`.

**Defer to v1.1:**

- File-JSON store
- SQLite store
- Optimistic concurrency
- `TurnCheckpointStorePort`
- `ArtifactStorePort`
- History condensation via LLM (use simple truncation for v1)

#### Section 14: Observability — **KEEP**

This section is well-designed and appropriately scoped. Structured events via `tracing` is the Rust standard.

**Minor simplification:** Start with `tracing` spans and events only. OpenTelemetry exporter can be a feature flag, not a separate adapter crate.

#### Section 15: Configuration — **SIMPLIFY**

The example `agent.toml` is good but too many knobs for v1.

**v1 config surface:**

```toml
[runtime]
max_steps = 12
turn_timeout_ms = 90000
default_model = "openai:gpt-4o-mini"

[llm]
stream_default = false

[policy]
deny_tools = []

[[mcp.servers]]
id = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[skills.sources]]
path = "./skills"
recursive = true
```

Drop: model catalog config, resume config, prompt budgeting ratios, sub-agent config, ranking strategy config. These return when their features return.

#### Section 16: Error Taxonomy — **KEEP, MINOR SIMPLIFY**

Good design with `thiserror`. For v1, reduce variants:

```rust
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),
    #[error("Policy violation: {0}")]
    Policy(String),
    #[error("Store error: {0}")]
    Store(#[from] StoreError),
    #[error("Timeout")]
    Timeout,
    #[error("Guard exceeded: {0:?}")]
    GuardExceeded(GuardReason),
    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}
```

Drop `SkillError`, `ModelCatalogError` variants for v1 (no skill port or catalog port).

#### Sections 17-21: Testing & Implementation Plan — **REALIGN**

The testing strategy and phased implementation are sound in structure but need to be re-scoped against the simplified v1 design. See `tasks.md` for revised implementation plan.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Testing

- Scheduler FSM transitions with mock `LlmPort` and `ToolPort`
- Action parsing (`JsonActionMode` JSON → `AgentAction`)
- Loop guard behavior (max steps, timeout, cancellation)
- Policy enforcement (allowlist/denylist)

### 5.2 Integration Testing

- Full turn loop: user input → LLM → tool call → LLM → final response (using `TapeLlmAdapter`)
- MCP tool execution against a local test server
- Error scenarios: LLM failure, tool timeout, malformed action

### 5.3 Critical Path Verification (The "Harness")

| Verification Step | Command | Success Criteria |
| :--- | :--- | :--- |
| **VP-01** | `cargo check --workspace` | All crates compile |
| **VP-02** | `cargo test --workspace` | All tests pass |
| **VP-03** | `just lint` | No clippy warnings |
| **VP-04** | `cargo run -p cli-agent -- --help` | CLI starts and shows help |
| **VP-05** | `cargo run -p cli-agent -- "What is 2+2?"` | Agent completes turn with LLM response |

### 5.4 Validation Rules

| Test Case ID | Action | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | Run agent with valid model | Turn completes with `AgentRunResult::Finished` | Unit test |
| **TC-02** | Run agent, LLM returns tool call | Tool is executed, result appended, loop continues | Unit test with mock |
| **TC-03** | Run agent, exceed max_steps | Returns `GuardExceeded(MaxSteps)` | Unit test |
| **TC-04** | Run agent, cancel mid-turn | Returns cancelled result | Unit test with `CancellationToken` |
| **TC-05** | Run agent with denied tool | Returns `PolicyViolation` | Unit test |

---

## 6. Implementation Plan

- [ ] **Phase 1: Foundation** — Fix dependency pins, scaffold 4 crates, define port traits and domain types
- [ ] **Phase 2: Core Logic** — Implement scheduler FSM with mock ports, cancellation, and guard logic
- [ ] **Phase 3: Adapters** — Implement genai adapter, MCP adapter, in-memory store, tracing events
- [ ] **Phase 4: Integration** — Wire CLI binary, run end-to-end turn loop, add skills loading
- [ ] **Phase 5: Polish** — Tests (unit + integration + tape replay), docs, error messages, CI

---

## 7. Cross-Functional Concerns

### 7.1 Migration from Current Design Doc

This review does **not** invalidate the full design document. It proposes a staged implementation strategy:

- v1: Simplified scope as described here
- v1.1: Add `SessionStore` with optimistic concurrency, skill runtime selection, model catalog, suspend/resume
- v2: Sub-agent system, multi-tenant, advanced routing

The full `docs/design.md` serves as the north-star architecture. This spec defines the v1 subset.

### 7.2 Summary of Review Verdicts

| Design Section | Verdict | Action |
| :--- | :--- | :--- |
| §2 Design Principles | **Keep** | No changes |
| §3 Version Baseline | **Fix** | genai pin → 0.5, defer models-dev |
| §4 Workspace Layout | **Simplify** | 10 crates → 4 crates |
| §5 Trait Strategy | **Simplify** | 8 ports → 4 ports |
| §6 Domain Model | **Simplify** | Drop Suspended, Resume, Checkpoint types |
| §7 Scheduler FSM | **Keep core, simplify** | 11 states → 6 states |
| §8 LLM Adapter | **Fix + Simplify** | Align to genai 0.5.x, drop catalog routing |
| §8.5-8.6 Model Catalog | **Defer entirely** | v1.1+ feature |
| §9 MCP Adapter | **Keep** | Minor tenant/serverless simplification |
| §10 Skills Adapter | **Simplify** | Static loading at bootstrap, no runtime trait |
| §11 Sub-Agent System | **Defer entirely** | v2 feature |
| §12 Policy/Guardrails | **Keep, simplify** | Struct-based policy, no approval flow |
| §13 Persistence | **Simplify** | In-memory only for v1 |
| §14 Observability | **Keep** | tracing spans + events |
| §15 Configuration | **Simplify** | Minimal TOML surface |
| §16 Error Taxonomy | **Keep, minor simplify** | Fewer error variants |
| §17-21 Testing/Plan | **Realign** | Rescope to v1 surface |

### 7.3 Dialectical Assessment

**What the design gets RIGHT:**

- Hexagonal architecture is the correct choice for a framework with pluggable external systems
- Bootstrap/Runtime trait split is clean and idiomatic
- Turn loop FSM with guard bounds is the core value proposition
- `AgentAction` as a provider-neutral action protocol is elegant
- `JsonActionMode` as default with native tool-calling opt-in is pragmatic
- Dual mode (streaming/non-streaming) sharing the same FSM is well-reasoned
- Cancellation via `CancellationToken` + `tokio::select!` is idiomatic Rust
- Error taxonomy with `thiserror` follows Rust conventions

**What the design gets WRONG:**

- Designs for scale before proving the core loop works
- Specifies production hardening (signed resume tokens, CAS concurrency, multi-tenant isolation) before a single integration test exists
- Creates adapter crates for systems not yet integrated
- The `models.dev` integration alone is ~20% of the design document for a feature that can be a simple string config
- Sub-agent system adds ~15% of the document for a feature that depends on stable single-agent execution

**The key insight:** The hexagonal architecture makes all deferred features *trivially addable later*. That's the whole point of ports and adapters. You don't need to design the SQLite adapter upfront — when you need it, you implement `SessionStore` and wire it in. The architecture enables growth without upfront specification.
