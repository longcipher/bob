# Context Compactor And Structured Tool Calling Design

## Goal

Improve Bob's runtime ergonomics and correctness by:

- replacing the hard-coded prompt history window with a pluggable context compaction port,
- upgrading persisted session messages from flat role/text entries to a structured transcript,
- enabling native provider tool-calling only after the runtime can represent assistant tool calls and tool results losslessly.

## Non-Goals

- No provider-specific chat types in `bob-core`.
- No automatic LLM summarizer as the default compaction strategy in this change.
- No streaming native tool-call protocol in this first pass.

## Problem Statement

Bob currently has two architecture gaps:

1. Prompt construction hard-codes a `MAX_HISTORY = 50` truncation rule in `bob-runtime`.
   This is not configurable, not token-aware, and not injectable for tests or future strategies.
2. Session history stores only `{ role, content }`. That is enough for prompt-guided JSON actions,
   but it is not enough for native tool-calling because assistant tool calls and tool responses need
   structured data and stable call identifiers.

If Bob enables native provider tool-calling without fixing the transcript model first, it will
either lose information or rely on brittle textual shims.

## Proposed Changes

### 1. Add `ContextCompactorPort`

Add a new `ContextCompactorPort` to `bob-core`:

- input: session transcript,
- output: compacted prompt-ready transcript.

The default implementation should preserve current behavior:

- keep all system messages,
- keep the most recent 50 non-system messages,
- drop the oldest non-system messages first.

This keeps current semantics stable while moving the policy behind a port.

### 2. Separate persisted transcript from prompt rendering

Session state should store a structured transcript instead of flat role/text messages.

The new transcript model should represent:

- system messages,
- user messages,
- assistant text messages,
- assistant tool-call batches,
- tool results with `call_id` and `tool_name`.

Prompt building should render this transcript into provider-neutral `LlmRequestMessage` values.
Adapters then translate those prompt messages into provider-specific request payloads.

### 3. Preserve backward compatibility for stored sessions

Existing session snapshots contain:

- `messages: [{ role, content }]`

The new transcript model must deserialize those old snapshots without data loss for the supported
legacy cases:

- `system` -> system text message,
- `user` -> user text message,
- `assistant` -> assistant text message,
- `tool` -> legacy tool result text entry with unknown `call_id`.

This keeps file and memory session stores usable without a migration step.

### 4. Enable native tool-calling only after transcript support exists

Once the runtime can persist and replay structured tool calls:

- `llm_genai` should declare `native_tool_calling = true`,
- request mapping should pass available tools through `ChatRequest::with_tools(...)`,
- response mapping should extract provider tool calls into Bob `ToolCall` values.

For this pass, streaming should remain conservative:

- if native tool-calling is active, the runtime should use non-streaming `complete()` for that turn,
  unless and until Bob's streaming protocol can carry tool-call chunks explicitly.

This avoids a half-implemented streaming path that silently loses tool-call structure.

## Alternatives Considered

### A. Keep flat session messages and only add compactor port

This is a smaller change, but it leaves native tool-calling built on top of an insufficient transcript
model. The next tool-calling change would still require a large session model refactor.

### B. Store provider-native chat payloads in `SessionState`

This would make adapters easier to write, but it would couple `bob-core` to provider semantics and
break the current hexagonal architecture.

### C. Add LLM summarization as the first compactor implementation

This would look more advanced, but it adds unstable behavior before the compaction boundary is even
properly abstracted. A stable deterministic window compactor is the right first step.

## Recommended Rollout

### Phase 1: Compactor Port

- Introduce `ContextCompactorPort`.
- Inject it through `RuntimeBuilder`.
- Replace direct `truncate_history(...)` usage in prompt building with the port.
- Keep the default deterministic 50-message window behavior.

### Phase 2: Structured Transcript

- Replace `SessionState.messages: Vec<Message>` with `Vec<TranscriptMessage>`.
- Add compatibility deserialization for existing stored sessions.
- Update prompt rendering and scheduler persistence.

### Phase 3: Native Tool Calling

- Upgrade `llm_genai` request/response mapping.
- Turn on `native_tool_calling`.
- Use non-streaming complete for native tool-calling turns until streaming protocol support exists.

## Testing Strategy

- Compactor tests must prove the default port preserves current truncation behavior.
- Transcript tests must prove legacy session payloads still deserialize.
- Scheduler tests must prove assistant tool calls and tool results are persisted structurally.
- Adapter tests must prove `genai` tool schemas and tool calls are mapped correctly.
