# bob-runtime

[![crates.io](https://img.shields.io/crates/v/bob-runtime.svg)](https://crates.io/crates/bob-runtime)
[![docs.rs](https://docs.rs/bob-runtime/badge.svg)](https://docs.rs/bob-runtime)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Runtime orchestration layer for the [Bob Agent Framework](https://github.com/longcipher/bob).

## Overview

`bob-runtime` provides the orchestration layer that coordinates agent execution:

- **Scheduler**: Finite state machine for agent turn execution
- **Action Parser**: Parses LLM responses into structured actions
- **Prompt Builder**: Constructs prompts with tool definitions and context
- **Composite Tool Port**: Aggregates multiple tool sources

This crate depends **only** on `bob-core` port traits — never on concrete adapters.

## Architecture

```text
┌─────────────────────────────────────────┐
│         AgentRuntime (trait)            │
├─────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌───────┐ │
│  │Scheduler │→ │Prompt    │→ │Action │ │
│  │  FSM     │  │Builder   │  │Parser │ │
│  └──────────┘  └──────────┘  └───────┘ │
└─────────────────────────────────────────┘
         ↓ uses ports from bob-core
```

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
bob-runtime = "0.1"
```

### Example: Creating a Runtime

```rust
use bob_runtime::{DefaultAgentRuntime, AgentRuntime};
use bob_core::{
    ports::{LlmPort, ToolPort, SessionStore, EventSink},
    types::{AgentRequest, TurnPolicy},
};
use std::sync::Arc;

async fn create_runtime(
    llm: Arc<dyn LlmPort>,
    tools: Arc<dyn ToolPort>,
    store: Arc<dyn SessionStore>,
    events: Arc<dyn EventSink>,
) -> Arc<dyn AgentRuntime> {
    Arc::new(DefaultAgentRuntime {
        llm,
        tools,
        store,
        events,
        default_model: "openai:gpt-4o-mini".to_string(),
        policy: TurnPolicy::default(),
    })
}
```

### Running an Agent Turn

```rust
use bob_core::types::AgentRequest;

let request = AgentRequest {
    input: "Write a hello world program".to_string(),
    session_id: "session-123".to_string(),
    model: None,
    metadata: Default::default(),
    cancel_token: None,
};

let result = runtime.run(request).await?;
```

## Features

- **Finite State Machine**: Robust turn execution with state tracking
- **Streaming Support**: Real-time event streaming via `run_stream()`
- **Tool Composition**: Aggregate multiple MCP servers or tool sources
- **Turn Policies**: Configurable limits for steps, timeouts, and retries
- **Health Monitoring**: Built-in health check endpoints

## Modules

- **`scheduler`**: Core FSM implementation for agent execution
- **`action`**: Action types and parser for LLM responses
- **`prompt`**: Prompt construction and tool definition formatting
- **`composite`**: Multi-source tool aggregation

## Documentation

Full API documentation is available at [docs.rs/bob-runtime](https://docs.rs/bob-runtime).

## Related Crates

- **[bob-core](https://crates.io/crates/bob-core)** - Domain types and ports
- **[bob-adapters](https://crates.io/crates/bob-adapters)** - Concrete implementations
- **[cli-agent](https://github.com/longcipher/bob)** - CLI application

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE.md) for details.
