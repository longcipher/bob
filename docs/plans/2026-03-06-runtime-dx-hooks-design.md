# Runtime DX Hooks Design

## Goal

Improve Bob's developer experience without weakening its hexagonal boundaries by:

- making the CLI use the existing high-level `AgentLoop`,
- exposing session usage information through first-class commands,
- introducing a fanout event sink for hook-style integrations,
- refreshing docs so public guidance matches the implementation.

## Non-Goals

- No stateful `Agent` object that owns conversation history in-memory like `agent-io`.
- No automatic LLM-based context compaction in this change.
- No cost-pricing service that fetches remote pricing data.

## Rationale

Bob already has stronger runtime boundaries than `agent-io`: session persistence, checkpoint/artifact ports, progressive tool views, and a composition-root builder. The missing piece is a high-level flow that developers can actually consume without stitching together parallel UX paths.

The existing CLI bypasses `AgentLoop`, so slash-command support, tape features, and system-prompt override logic are effectively split across two orchestration layers. This creates drift and weakens the intended framework surface.

## Proposed Changes

### 1. Make `AgentLoop` the canonical interactive facade

Extend `AgentLoop` so it can optionally load session state and render usage statistics. Then switch the CLI to use `AgentLoop::handle_input(...)` instead of manually routing commands and calling `runtime.run(...)`.

This keeps `RuntimeBuilder` as the low-level composition primitive while letting `AgentLoop` become the high-level "easy path".

### 2. Add usage-aware session commands

Add a `/usage` slash command that reads cumulative session usage from `SessionStore`. The output should include:

- prompt tokens,
- completion tokens,
- total tokens.

This does not require pricing or remote model metadata. It uses the data Bob already persists and avoids introducing billing heuristics that can silently drift.

### 3. Add fanout event delivery for hooks

Introduce a `FanoutEventSink` adapter that forwards each `AgentEvent` to multiple child sinks. This provides the minimal runtime infrastructure needed for hooks, tracing + metrics combinations, or test taps, while preserving the current `EventSink` port.

This should use a small fluent builder API:

- `FanoutEventSink::new()`
- `.with_sink(...)`

### 4. Enrich event metadata where it directly helps hooks

Keep the event model compact, but add enough runtime context to make sinks useful:

- `session_id` on turn-level events,
- `session_id` and `step` on LLM/tool/error events,
- final turn usage on `TurnCompleted`.

This avoids turning the event model into a giant transport schema while still making downstream consumers practical to implement.

### 5. Refresh docs to match public behavior

Update workspace and crate READMEs so:

- versions match the current `0.2.1` release line,
- the CLI examples reflect `AgentLoop`-driven command support,
- usage visibility and event fanout are documented.

## Deferred Follow-Up

Context compaction should be added later as a dedicated port-based subsystem, likely centered around a `ContextCompactorPort` or similar interface. That work should use session/tape/checkpoint data and token budgets, not only ephemeral tool-result deletion.
